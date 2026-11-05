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
        api::ApiFuture,
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
}
struct ReadOnly {
    backend: ReqwestBackend,
    observed: Mutex<Observations>,
}
fn permitted(plan: &RequestPlan) -> Result<(String, bool)> {
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
    let allowed = match path.as_str() {
        ACCOUNT => true,
        ASSIGNMENT_PATH => args["expNames"] == assignment_arguments()["expNames"],
        LEGACY | V3 | TABS => args["keyword"] == QUERY,
        _ => false,
    };
    if !allowed {
        return Err(Error::State("outside signed search read proof"));
    }
    Ok((path, plan.decrypt_response))
}
impl HttpBackend for ReadOnly {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            let (path, encrypted) = match permitted(&plan) {
                Ok(value) => value,
                Err(error) => {
                    self.observed
                        .lock()
                        .map_err(|_| Error::LockPoisoned)?
                        .denied += 1;
                    return Err(Error::RequestNotSent(Box::new(error)));
                }
            };
            let response = self.backend.post(plan).await?;
            let bytes = if encrypted {
                eapi::decrypt_response(&response.body).ok()
            } else {
                None
            };
            let parsed =
                serde_json::from_slice::<Value>(bytes.as_deref().unwrap_or(&response.body)).ok();
            let mut observed = self.observed.lock().map_err(|_| Error::LockPoisoned)?;
            observed.calls.push(json!({"path":path,"httpStatus":response.status,"code":parsed.as_ref().and_then(|v|v["code"].as_i64())}));
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
        Error::CredentialChanged | Error::StaleResponse => "identity_changed",
        Error::State("fresh SDK token required; read proof stopped") => "sdk_required_not_sent",
        Error::Transport(_) => "transport_failed",
        Error::Service { .. } | Error::HttpStatus { .. } => "service_rejected",
        _ => "unavailable",
    }
}
async fn verify(source: &Path, output: &Path) -> Result<Value> {
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
    // Reuse an existing installation identity; do not invent a fresh primary ID.
    if store.get("device", "identity", "client_sign")?.is_none() {
        return Err(Error::State("saved installation identity unavailable"));
    }
    let facts = collect_linux_identity(Path::new("/sys"))?;
    let native = IdentityService::new(store.clone())
        .resolve(&facts)?
        .profile(ClientProfile::windows_compatibility(
            WINDOWS_COMPATIBILITY_DESCRIPTION,
        )?)?;
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
    match client.restore_account().await {
        Ok(account) if account.status == SessionStatus::Authenticated => {
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
                        report["status"] = json!("ok");
                        report["songCount"] = json!(songs);
                        report["hintCount"] = json!(hints);
                        report["reasonCount"] = json!(reasons);
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
#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let report = match args.as_slice() {
        [] => json!({"status":"skipped","reads":[],"writes":0}),
        [flag, source, output] if flag == "--existing-pc-state" => {
            match verify(&PathBuf::from(source), &PathBuf::from(output)).await {
                Ok(value) => value,
                Err(error) => json!({"status":"setup_failed","failure":failure(&error),"writes":0}),
            }
        }
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
        assert!(permitted(&plan(ASSIGNMENT_PATH, assignment_arguments())).is_ok());
        assert!(permitted(&plan(ACCOUNT, json!({}))).is_ok());
        for path in [LEGACY, V3, TABS] {
            assert!(permitted(&plan(path, json!({"keyword":QUERY}))).is_ok());
            assert!(permitted(&plan(path, json!({"keyword":"different"}))).is_err());
        }
        assert!(
            permitted(&plan(
                ASSIGNMENT_PATH,
                json!({"expNames":"PH-PC-search-result"})
            ))
            .is_err()
        );
        for path in [
            "/api/login/token/refresh/v1",
            "/api/nuser/account/refresh",
            "/api/logout",
            "/api/resource/comments/add",
            "/api/song/enhance/player/url/v1",
        ] {
            assert!(permitted(&plan(path, json!({}))).is_err());
        }
        let mut wrong_origin = plan(ACCOUNT, json!({}));
        wrong_origin.url = ApiOrigin::Interface
            .request_url(WireRoute::Eapi, ACCOUNT)
            .unwrap();
        assert!(permitted(&wrong_origin).is_err());
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
        let count: u64 = original
            .query_row("SELECT count(*) FROM evidence", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
}
