//! Explicit existing-PC-state read proof. No arguments: no profile or network access.
use cumulus_core::{
    Error, Result,
    account::SessionStatus,
    client::{
        MusicClient,
        dto::{SearchCategory, SearchItem, SearchResultQuery},
    },
    identity::{IdentityService, WINDOWS_COMPATIBILITY_DESCRIPTION, collect_linux_identity},
    protocol::{
        api::{ApiFuture, identifier},
        context::{RequestContext, TokenProvider},
        http::{
            APP_VERSION, ApiOrigin, ClientProfile, HttpBackend, HttpReply, RequestPlan,
            ReqwestBackend, WireRoute,
        },
        mconfig::{ConfigBackend, ConfigUrl, PcConfig},
        pc_experiments::{ASSIGNMENT_PATH, PcExperimentGroups, assignment_arguments},
        wire::eapi,
    },
    storage::{StateStore, private_directory_builder},
};
use rusqlite::{Connection, OpenFlags, ffi};
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

const ACCOUNT: &str = "/api/w/nuser/account/get";
const LEGACY: &str = "/api/search/pc/complex/page";
const V3: &str = "/api/search/pc/complex/page/v3";
const TABS: &str = "/api/search/pc/result/tab";
const PLAYLISTS: &str = "/api/user/playlist";
const LIKED: &str = "/api/song/like/get";
const PAGE_SIZE: u64 = 200;
const MAX_PLAYLIST_PAGES: usize = 16;
const QUERY: &str = "夜空中最亮的星";
const KEYS: [&str; 4] = [
    "PH-PC-searchHot",
    "PH-PC-searchMore2",
    "PH-PC-searchPageEfficiency",
    "PH-PC-search-result",
];

#[derive(Default)]
struct DenyAuxiliary {
    tokens: AtomicUsize,
    configs: AtomicUsize,
}
impl TokenProvider for DenyAuxiliary {
    fn token<'a>(&'a self, _: &'a str) -> ApiFuture<'a, String> {
        self.tokens.fetch_add(1, Ordering::Relaxed);
        Box::pin(async { Err(Error::State("fresh SDK token required; read proof stopped")) })
    }
}
impl ConfigBackend for DenyAuxiliary {
    fn get<'a>(&'a self, _: &'a ConfigUrl) -> ApiFuture<'a, Value> {
        self.configs.fetch_add(1, Ordering::Relaxed);
        Box::pin(async {
            Err(Error::State(
                "configuration download disabled in read proof",
            ))
        })
    }
}

#[derive(Default)]
struct Observations {
    calls: Vec<Value>,
    groups: Option<[bool; 4]>,
    denied: usize,
    account_matched: bool,
    playlist_next: Option<u64>,
    playlist_pages: usize,
}
struct ReadOnly {
    backend: ReqwestBackend,
    observed: Mutex<Observations>,
    expected_uid: String,
    include_resources: bool,
}
struct Admitted {
    path: String,
    encrypted: bool,
    playlist_offset: Option<u64>,
}

fn exact_decimal(value: &Value) -> Option<u64> {
    let raw = value.as_str()?;
    let parsed: u64 = raw.parse().ok()?;
    (parsed.to_string() == raw).then_some(parsed)
}

fn permitted(plan: &RequestPlan, resource_uid: Option<&str>) -> Result<Admitted> {
    if plan.route != WireRoute::Eapi {
        return Err(Error::State("read proof requires EAPI"));
    }
    ApiOrigin::Pc.require_url(&plan.url)?;
    let mut form = url::form_urlencoded::parse(&plan.body);
    let (key, encrypted) = form.next().ok_or(Error::State("missing EAPI form"))?;
    if key != "params" || form.next().is_some() {
        return Err(Error::State("unexpected EAPI form"));
    }
    let (path, args) = eapi::unwrap(&encrypted)?;
    if ApiOrigin::Pc.request_url(WireRoute::Eapi, &path)? != plan.url {
        return Err(Error::State("read proof route mismatch"));
    }
    let offset = exact_decimal(&args["offset"]).filter(|_| path == PLAYLISTS);
    let allowed = match path.as_str() {
        ACCOUNT => true,
        ASSIGNMENT_PATH => args["expNames"] == assignment_arguments()["expNames"],
        LEGACY | V3 | TABS => args["keyword"] == QUERY,
        PLAYLISTS => {
            resource_uid.is_some_and(|uid| args["uid"].as_str() == Some(uid))
                && args["includeVideo"] == "true"
                && exact_decimal(&args["limit"]) == Some(PAGE_SIZE)
                && offset.is_some()
        }
        LIKED => resource_uid.is_some_and(|uid| args["uid"].as_str() == Some(uid)),
        _ => false,
    };
    if !allowed {
        return Err(Error::State("outside signed search read proof"));
    }
    Ok(Admitted {
        playlist_offset: offset,
        path,
        encrypted: plan.decrypt_response,
    })
}

impl Observations {
    fn next(&mut self, read: &Admitted) -> Result<usize> {
        let path = read.path.as_str();
        let allowed = match path {
            ACCOUNT => self.calls.is_empty(),
            _ if !self.account_matched => false,
            PLAYLISTS => {
                self.playlist_pages < MAX_PLAYLIST_PAGES
                    && self.playlist_next.is_some()
                    && self.playlist_next == read.playlist_offset
            }
            _ => !self.calls.iter().any(|seen| {
                seen["path"] == path
                    || (matches!(path, LEGACY | V3)
                        && matches!(seen["path"].as_str(), Some(LEGACY | V3)))
            }),
        };
        if !allowed {
            self.denied += 1;
            return Err(Error::RequestNotSent(Box::new(Error::State(
                "unexpected read sequence",
            ))));
        }
        if path == PLAYLISTS {
            self.playlist_pages += 1;
            self.playlist_next = None;
        }
        let index = self.calls.len();
        self.calls.push(json!({"path":path,"outcome":"dispatched"}));
        Ok(index)
    }

    fn playlist_reply(&mut self, offset: u64, reply: &Value) -> Result<()> {
        let count = reply["playlist"]
            .as_array()
            .map(Vec::len)
            .and_then(|count| u64::try_from(count).ok())
            .filter(|count| *count <= PAGE_SIZE)
            .ok_or(Error::Response("invalid playlist page size"))?;
        let flag = reply.get("more").or_else(|| reply.get("hasMore"));
        let more = match flag {
            None | Some(Value::Null) => count == PAGE_SIZE,
            Some(Value::Bool(value)) => *value,
            _ => return Err(Error::Response("invalid playlist page flag")),
        };
        if more && count == 0 {
            return Err(Error::Response("playlist page made no progress"));
        }
        self.playlist_next = if more {
            offset.checked_add(count)
        } else {
            None
        };
        Ok(())
    }
}
impl HttpBackend for ReadOnly {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            let resource_uid = self.include_resources.then_some(self.expected_uid.as_str());
            let read = match permitted(&plan, resource_uid) {
                Ok(value) => value,
                Err(error) => {
                    self.observed
                        .lock()
                        .map_err(|_| Error::LockPoisoned)?
                        .denied += 1;
                    return Err(Error::RequestNotSent(Box::new(error)));
                }
            };
            let index = self
                .observed
                .lock()
                .map_err(|_| Error::LockPoisoned)?
                .next(&read)?;
            let path = read.path.as_str();
            let response = match self.backend.post(plan).await {
                Ok(response) => response,
                Err(error) => {
                    self.observed.lock().map_err(|_| Error::LockPoisoned)?.calls[index]["failure"] =
                        json!(failure(&error));
                    return Err(error);
                }
            };
            let bytes = if read.encrypted {
                eapi::decrypt_response(&response.body).ok()
            } else {
                None
            };
            let parsed =
                serde_json::from_slice::<Value>(bytes.as_deref().unwrap_or(&response.body)).ok();
            let mut observed = self.observed.lock().map_err(|_| Error::LockPoisoned)?;
            let code = parsed.as_ref().and_then(|v| v["code"].as_i64());
            observed.calls[index] = json!({"path":path,"httpStatus":response.status,"code":code});
            if response.status == 200 && code == Some(200) {
                let body = parsed
                    .as_ref()
                    .ok_or(Error::Response("missing read reply"))?;
                if path == ACCOUNT {
                    if identifier(&body["profile"]["userId"]).ok().as_deref()
                        != Some(self.expected_uid.as_str())
                    {
                        return Err(Error::CredentialChanged);
                    }
                    observed.account_matched = true;
                    observed.playlist_next = self.include_resources.then_some(0);
                }
                if let Some(offset) = read.playlist_offset {
                    observed.playlist_reply(offset, body)?;
                }
            }
            if path == ASSIGNMENT_PATH
                && let Some(groups) = parsed
                    .as_ref()
                    .and_then(|v| PcExperimentGroups::refreshed(v).ok())
            {
                observed.groups = Some(KEYS.map(|key| groups.group(key) == Some("t1")));
            }
            Ok(response)
        })
    }
}

/// SQLite's backup API includes committed WAL content. Source connections remain
/// read-only; no credential/profile file is opened with write/create flags.
fn snapshot_database(source: &Path, destination: &Path) -> Result<()> {
    if !fs::symlink_metadata(source)?.is_file() || destination.exists() {
        return Err(Error::State("expected existing database and new snapshot"));
    }
    let from = Connection::open_with_flags(
        source,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    let to = Connection::open(destination)?;
    // SAFETY: both connections live in this stack frame, use distinct SQLite
    // handles on this thread, and no SQL runs on either until backup_finish.
    let backup = unsafe {
        ffi::sqlite3_backup_init(
            to.handle(),
            c"main".as_ptr(),
            from.handle(),
            c"main".as_ptr(),
        )
    };
    if backup.is_null() {
        return Err(Error::State("SQLite snapshot initialization failed"));
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    let result = loop {
        // SAFETY: the initialized backup is uniquely owned here until finish.
        let code = unsafe { ffi::sqlite3_backup_step(backup, 128) };
        match code {
            ffi::SQLITE_DONE => break Ok(()),
            ffi::SQLITE_OK if Instant::now() < deadline => (),
            ffi::SQLITE_BUSY | ffi::SQLITE_LOCKED if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10))
            }
            _ => break Err(Error::State("SQLite snapshot did not complete")),
        }
    };
    // SAFETY: exactly one finish for the successfully initialized backup.
    let finished = unsafe { ffi::sqlite3_backup_finish(backup) };
    result?;
    if finished != ffi::SQLITE_OK {
        return Err(Error::State("SQLite snapshot finalization failed"));
    }
    Ok(())
}

fn failure(error: &Error) -> &'static str {
    if error.is_service_code(301) {
        return "session_unavailable";
    }
    match error.root() {
        Error::State("saved installation identity unavailable") => {
            "saved_installation_identity_missing"
        }
        Error::State("saved native PC config unavailable") => "saved_native_pc_config_missing",
        Error::CredentialChanged | Error::StaleResponse => "identity_changed",
        Error::State("fresh SDK token required; read proof stopped") => "sdk_required_not_sent",
        Error::Transport(_) => "transport_failed",
        Error::Service { .. } | Error::HttpStatus { .. } => "service_rejected",
        _ => "unavailable",
    }
}
fn native_profile(
    store: Arc<StateStore>,
    output: &Path,
    fresh: bool,
    facts: &cumulus_core::identity::HardwareFacts,
) -> Result<ClientProfile> {
    let identity = if fresh {
        // A separate, initially empty installation store. Never overwrite the
        // copied primary identity or borrow another installation's record.
        Arc::new(StateStore::open(output.join("fresh-local-identity"))?)
    } else {
        if store.get("device", "identity", "client_sign")?.is_none() {
            return Err(Error::State("saved installation identity unavailable"));
        }
        store
    };
    IdentityService::new(identity)
        .resolve(facts)?
        .profile(ClientProfile::windows_compatibility(
            WINDOWS_COMPATIBILITY_DESCRIPTION,
        )?)
}
async fn resource_counts(client: &MusicClient, epoch: &str, report: &mut Value) -> bool {
    let playlists = match client.playlists().await {
        Ok(rows) if rows.epoch == epoch => {
            report["playlistCount"] = json!(rows.value.len());
            report["ownedPlaylistCount"] = json!(rows.value.iter().filter(|row| row.owned).count());
            report["hasOwnedFavoritePlaylist"] =
                json!(rows.value.iter().any(|row| row.favorite && row.owned));
            true
        }
        Ok(_) => {
            report["playlistFailure"] = json!("identity_changed");
            false
        }
        Err(error) => {
            report["playlistFailure"] = json!(failure(&error));
            false
        }
    };
    let liked = match client.liked_ids().await {
        Ok(rows) if rows.epoch == epoch => {
            report["likedCount"] = json!(rows.value.len());
            true
        }
        Ok(_) => {
            report["likedFailure"] = json!("identity_changed");
            false
        }
        Err(error) => {
            report["likedFailure"] = json!(failure(&error));
            false
        }
    };
    playlists && liked
}

fn saved_account_uid(store: &StateStore, selection: &str) -> Result<String> {
    let profiles = store.values("transport", "saved-profiles-v1")?;
    let mut matching = profiles
        .iter()
        .filter(|(_, record)| record["session_id"].as_str() == Some(selection));
    match (matching.next(), matching.next()) {
        (Some((uid, record)), None) if record["uid"].as_str() == Some(uid.as_str()) => {
            identifier(&record["uid"])
        }
        _ => Err(Error::State("selected saved account is not uniquely bound")),
    }
}

async fn verify(source: &Path, output: &Path, fresh: bool, resources: bool) -> Result<Value> {
    if !cfg!(target_os = "linux") || !fs::symlink_metadata(source)?.is_dir() {
        return Err(Error::State("existing Linux desktop state required"));
    }
    private_directory_builder().create(output)?;
    let state = output.join("state");
    private_directory_builder().create(&state)?;
    snapshot_database(&source.join("core.sqlite3"), &state.join("core.sqlite3"))?;
    let store = Arc::new(StateStore::open(&state)?);
    let selected = store
        .get("transport", "profiles", "active")?
        .ok_or(Error::State("no selected profile"))?;
    let id = selected["session_id"]
        .as_str()
        .filter(|s| {
            s.len() == 32
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
        .ok_or(Error::State("invalid selected profile"))?;
    let expected_uid = saved_account_uid(&store, id)?;
    for parent in [source.join("sessions"), source.join("sessions").join(id)] {
        if !fs::symlink_metadata(parent)?.is_dir() {
            return Err(Error::State("selected profile directory is redirected"));
        }
    }
    let profile = state.join("sessions").join(id);
    private_directory_builder()
        .recursive(true)
        .create(&profile)?;
    snapshot_database(
        &source.join("sessions").join(id).join("core.sqlite3"),
        &profile.join("core.sqlite3"),
    )?;
    // Reuse-only mode still checks before collecting hardware, preserving its
    // original explicit no-network preflight boundary.
    if !fresh && store.get("device", "identity", "client_sign")?.is_none() {
        return Err(Error::State("saved installation identity unavailable"));
    }
    let facts = collect_linux_identity(Path::new("/sys"))?;
    let native = native_profile(store.clone(), output, fresh, &facts)?;
    let denied_auxiliary = Arc::new(DenyAuxiliary::default());
    let config = PcConfig::new(store, denied_auxiliary.clone())?;
    let saved = config.snapshot()?;
    if saved.version == 0 || saved.data.is_none() {
        return Err(Error::State("saved native PC config unavailable"));
    }
    let context = Arc::new(RequestContext::from_pc(
        config.clone(),
        APP_VERSION,
        denied_auxiliary.clone(),
    )?);
    let transport = Arc::new(ReadOnly {
        backend: ReqwestBackend::new(Duration::from_secs(15))?,
        observed: Mutex::default(),
        expected_uid,
        include_resources: resources,
    });
    let client = MusicClient::open_with_request_context(
        &state,
        Some(&profile),
        native,
        Some(transport.clone()),
        Some(context),
    )?
    .with_official_search_experiment();
    let mut report = json!({"status":"unavailable","sourceCommit":option_env!("CUMULUS_READ_PROOF_COMMIT"),"writes":0,"sdkRequests":0,"renewalRequests":0,"savedNativePcConfig":true});
    report["identityMode"] = json!(if fresh {
        "fresh_local_initialization"
    } else {
        "restored_installation"
    });
    match client.restore_account().await {
        Ok(account) if account.status == SessionStatus::Authenticated => {
            let resources_ok =
                !resources || resource_counts(&client, &account.epoch, &mut report).await;
            match client
                .official_pc_ordinary_skip_assignment(&account.epoch)
                .await
            {
                Ok(_) => match client
                    .search_results(
                        &account.epoch,
                        SearchResultQuery {
                            keyword: QUERY.into(),
                            category: SearchCategory::All,
                            offset: 0,
                            limit: 6,
                            correct: true,
                            channel: None,
                        },
                    )
                    .await
                {
                    Ok(result) => {
                        let mut songs = 0;
                        let mut hints = 0;
                        let mut reasons = 0;
                        for section in &result.value.sections {
                            for item in &section.items {
                                if let SearchItem::Song {
                                    hint, reasons: row, ..
                                } = item
                                {
                                    songs += 1;
                                    hints += usize::from(hint.is_some());
                                    reasons += row.len();
                                }
                            }
                        }
                        report["status"] = json!(if resources_ok {
                            "ok"
                        } else {
                            "account_resources_incomplete"
                        });
                        report["songCount"] = json!(songs);
                        report["hintCount"] = json!(hints);
                        report["reasonCount"] = json!(reasons);
                        let footer = transport
                            .observed
                            .lock()
                            .map_err(|_| Error::LockPoisoned)?
                            .groups
                            .map(|groups| groups[3]);
                        report["footerProjectionConsistent"] =
                            json!(footer.is_some_and(|enabled| enabled || hints + reasons == 0));
                    }
                    Err(error) => report["searchFailure"] = json!(failure(&error)),
                },
                Err(error) => report["assignmentFailure"] = json!(failure(&error)),
            }
        }
        Ok(_) => report["accountFailure"] = json!("session_unavailable"),
        Err(error) => report["accountFailure"] = json!(failure(&error)),
    }
    config.close()?;
    let observed = transport.observed.lock().map_err(|_| Error::LockPoisoned)?;
    report["reads"] = json!(observed.calls);
    report["groups"] = json!(observed.groups);
    report["expectedSavedAccountMatched"] = json!(observed.account_matched);
    report["accountResourcesRequested"] = json!(resources);
    report["deniedRequests"] = json!(observed.denied);
    report["sdkTokenDemands"] = json!(denied_auxiliary.tokens.load(Ordering::Relaxed));
    report["configDownloadsDenied"] = json!(denied_auxiliary.configs.load(Ordering::Relaxed));
    if observed.denied > 0 {
        report["status"] = json!("scope_rejected");
    }
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}

fn modes(flags: &[OsString]) -> Option<(bool, bool)> {
    let (mut fresh, mut resources) = (false, false);
    for flag in flags {
        if flag == "--fresh-local-identity" && !fresh {
            fresh = true;
        } else if flag == "--include-account-resources" && !resources {
            resources = true;
        } else {
            return None;
        }
    }
    Some((fresh, resources))
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let report = match args.as_slice() {
        [] => json!({"status":"skipped","reads":[],"writes":0}),
        [flag, source, output, flags @ ..] if flag == "--existing-pc-state" => match modes(flags) {
            Some((fresh, resources)) => {
                match verify(
                    &PathBuf::from(source),
                    &PathBuf::from(output),
                    fresh,
                    resources,
                )
                .await
                {
                    Ok(value) => value,
                    Err(error) => {
                        json!({"status":"setup_failed","failure":failure(&error),"writes":0})
                    }
                }
            }
            None => json!({"status":"invalid_arguments","writes":0}),
        },
        _ => json!({"status":"invalid_arguments","writes":0}),
    };
    println!("{report}");
    if report["status"] == "ok" || report["status"] == "skipped" {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(path: &str, args: Value) -> RequestPlan {
        let args = cumulus_core::protocol::wire::serialization::official_data(&args).unwrap();
        let params = eapi::encrypt(path, &args).unwrap();
        RequestPlan {
            url: ApiOrigin::Pc.request_url(WireRoute::Eapi, path).unwrap(),
            headers: Default::default(),
            body: url::form_urlencoded::Serializer::new(String::new())
                .append_pair("params", &params)
                .finish()
                .into_bytes(),
            route: WireRoute::Eapi,
            decrypt_response: false,
        }
    }

    #[test]
    fn allowlist_accepts_only_full_assignment_and_fixed_search_not_refresh_or_writes() {
        assert!(permitted(&plan(ASSIGNMENT_PATH, assignment_arguments()), None).is_ok());
        assert!(permitted(&plan(ACCOUNT, json!({})), None).is_ok());
        for path in [LEGACY, V3, TABS] {
            assert!(permitted(&plan(path, json!({"keyword":QUERY})), None).is_ok());
            assert!(permitted(&plan(path, json!({"keyword":"different"})), None).is_err());
        }
        assert!(
            permitted(
                &plan(ASSIGNMENT_PATH, json!({"expNames":"PH-PC-search-result"})),
                None,
            )
            .is_err()
        );
        for path in [
            "/api/login/token/refresh/v1",
            "/api/nuser/account/refresh",
            "/api/logout",
            "/api/resource/comments/add",
            "/api/song/enhance/player/url/v1",
        ] {
            assert!(permitted(&plan(path, json!({})), None).is_err());
        }
        let mut wrong_origin = plan(ACCOUNT, json!({}));
        wrong_origin.url = ApiOrigin::Interface
            .request_url(WireRoute::Eapi, ACCOUNT)
            .unwrap();
        assert!(permitted(&wrong_origin, None).is_err());
    }

    #[test]
    fn own_resources_cannot_run_early_for_another_account_or_replay_pages() {
        let playlist = |offset| {
            plan(
                PLAYLISTS,
                json!({"uid":"42","includeVideo":true,"limit":200,"offset":offset}),
            )
        };
        assert!(permitted(&playlist(0), None).is_err());
        assert!(permitted(&playlist(0), Some("43")).is_err());
        let noncanonical = plan(
            PLAYLISTS,
            json!({"uid":"42","includeVideo":true,"limit":200,"offset":"00"}),
        );
        assert!(permitted(&noncanonical, Some("42")).is_err());
        let first = permitted(&playlist(0), Some("42")).unwrap();
        let account = permitted(&plan(ACCOUNT, json!({})), Some("42")).unwrap();
        let liked_plan = plan(LIKED, json!({"uid":"42"}));
        assert!(permitted(&liked_plan, Some("43")).is_err());
        let liked = permitted(&liked_plan, Some("42")).unwrap();
        let mut state = Observations::default();
        assert!(state.next(&liked).is_err());
        assert!(state.next(&first).is_err());
        state.next(&account).unwrap();
        assert!(state.next(&account).is_err());
        state.account_matched = true;
        state.playlist_next = Some(0);
        state.next(&first).unwrap();
        assert!(state.next(&first).is_err());
        assert!(
            state
                .playlist_reply(0, &json!({"playlist":[],"more":true}))
                .is_err()
        );
        state
            .playlist_reply(0, &json!({"playlist":[{},{}],"more":true}))
            .unwrap();
        assert!(state.next(&first).is_err());
        state
            .next(&permitted(&playlist(2), Some("42")).unwrap())
            .unwrap();
        state
            .playlist_reply(2, &json!({"playlist":[{}],"more":false}))
            .unwrap();
        assert!(
            state
                .next(&permitted(&playlist(3), Some("42")).unwrap())
                .is_err()
        );
        state.next(&liked).unwrap();
        assert!(state.next(&liked).is_err());
    }

    #[test]
    fn saved_uid_requires_one_matching_record_with_the_same_key() {
        let root = tempfile::tempdir().unwrap();
        let state = StateStore::open(root.path()).unwrap();
        assert!(saved_account_uid(&state, "selected").is_err());
        for uid in ["42", "43"] {
            state
                .put(
                    "transport",
                    "saved-profiles-v1",
                    uid,
                    &json!({"uid":uid,"session_id":"selected"}),
                )
                .unwrap();
            if uid == "42" {
                assert_eq!(saved_account_uid(&state, "selected").unwrap(), "42");
            }
        }
        assert!(saved_account_uid(&state, "selected").is_err());
    }

    #[test]
    fn backup_captures_committed_live_wal_without_modifying_source_rows() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.sqlite3");
        let original = Connection::open(&source).unwrap();
        original.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE evidence(value TEXT); INSERT INTO evidence VALUES ('synthetic committed WAL');").unwrap();
        assert!(PathBuf::from(format!("{}-wal", source.display())).is_file());
        let destination = root.path().join("snapshot.sqlite3");
        snapshot_database(&source, &destination).unwrap();
        let copy =
            Connection::open_with_flags(destination, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let value: String = copy
            .query_row("SELECT value FROM evidence", [], |row| row.get(0))
            .unwrap();
        assert_eq!(value, "synthetic committed WAL");
        let count: i64 = original
            .query_row("SELECT count(*) FROM evidence", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn fresh_identity_uses_its_own_store_and_preserves_existing_primary() {
        use cumulus_core::identity::{HardwareFacts, IdentitySource};
        let root = tempfile::tempdir().unwrap();
        let copied = Arc::new(StateStore::open(root.path().join("copied-installation")).unwrap());
        let original = json!({"persistent_id":"a".repeat(64)});
        copied
            .put("device", "identity", "client_sign", &original)
            .unwrap();
        let facts = HardwareFacts::from_encoded(
            [
                "synthetic-local-mac".into(),
                "synthetic-local-disk".into(),
                "synthetic-board".into(),
            ],
            b"synthetic-local-hardware".to_vec(),
            b"synthetic-local-host".to_vec(),
            IdentitySource {
                platform: "synthetic-test".into(),
                interface: String::new(),
                disk_device: String::new(),
                host_description_available: true,
            },
        )
        .unwrap();
        let native = native_profile(copied.clone(), root.path(), true, &facts).unwrap();
        assert!(native.client_sign.as_ref().is_some_and(|s| !s.is_empty()));
        assert_eq!(
            copied.get("device", "identity", "client_sign").unwrap(),
            Some(original)
        );
        let fresh = StateStore::open(root.path().join("fresh-local-identity")).unwrap();
        assert_eq!(
            fresh
                .get("device", "identity", "client_sign")
                .unwrap()
                .unwrap()["persistent_id"],
            facts.derived_persistent_id()
        );
    }
}
