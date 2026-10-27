//! Real-account proof guard and durable isolated cleanup. Not linked into the app.
use cumulus_core::{
    Error, Result,
    account::{AccountSession, SessionToken, credentials::Credentials, profiles::ProfileRegistry},
    client::MusicClient,
    playlist::{cover::*, *},
    protocol::{
        api::{Api, ApiFuture},
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

pub fn save(path: &Path, value: &Value) -> Result<()> {
    use std::io::Write;
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    Ok(())
}
struct Guard {
    inner: Arc<dyn Api>,
    root: PathBuf,
    output: PathBuf,
    name: String,
    id: Mutex<Option<String>>,
    sequence: AtomicUsize,
}
impl Api for Guard {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let allowed = match path {
                "/api/w/nuser/account/get"
                | "/api/music-vip-membership/client/vip/info"
                | "/api/user/playlist"
                | "/api/song/like/get"
                | "/api/v3/discovery/recommend/songs"
                | "/api/v3/song/detail"
                | "/api/v6/playlist/detail"
                | "/api/playlist/catalogue" => true,
                "/api/playlist/create" => payload["name"] == self.name && payload["privacy"] == 10,
                "/api/nos/token/alloc" => {
                    payload
                        == json!({"filename":"cover.jpg","ext":"jpg","type":"image","bucket":"yyimgs"})
                        && self.id.lock().unwrap().is_some()
                }
                "/api/playlist/cover/update" => self
                    .id
                    .lock()
                    .unwrap()
                    .as_deref()
                    .is_some_and(|id| payload["id"] == id),
                "/api/playlist/delete" => self
                    .id
                    .lock()
                    .unwrap()
                    .as_deref()
                    .is_some_and(|id| payload["pid"] == id),
                _ => false,
            };
            if !allowed {
                return Err(Error::State("endpoint outside isolated cover UI proof"));
            }
            if matches!(
                path,
                "/api/playlist/create"
                    | "/api/nos/token/alloc"
                    | "/api/playlist/cover/update"
                    | "/api/playlist/delete"
            ) {
                // create_new persists the once-only guard across process restarts.
                save(
                    &self
                        .root
                        .join(format!("sent-{}.json", path.replace('/', "-"))),
                    &json!({"path":path,"payload":payload}),
                )?;
            }
            if path == "/api/playlist/cover/update" {
                let images = fs::read_dir(self.root.join("state"))?
                    .collect::<std::io::Result<Vec<_>>>()?
                    .into_iter()
                    .filter(|entry| {
                        entry
                            .file_name()
                            .to_string_lossy()
                            .starts_with("playlist-cover-")
                            && entry.path().extension().is_some_and(|v| v == "jpg")
                    })
                    .collect::<Vec<_>>();
                if images.len() != 1 {
                    return Err(Error::State(
                        "expected one encoded cover in isolated profile",
                    ));
                }
                fs::copy(images[0].path(), self.root.join("uploaded.jpg"))?;
            }
            let n = self.sequence.fetch_add(1, Ordering::SeqCst);
            save(
                &self.output.join(format!("api-{n:03}-request.json")),
                &json!({"path":path,"payload":payload}),
            )?;
            let result = self.inner.request(path, payload, cache).await;
            let mut observed = result
                .as_ref()
                .cloned()
                .unwrap_or(json!({"transport_error":true}));
            for key in ["result", "data"] {
                if observed[key].get("token").is_some() {
                    observed[key]["token"] = json!("<redacted>");
                }
            }
            save(
                &self.output.join(format!("api-{n:03}-response.json")),
                &observed,
            )?;
            result
        })
    }
}
pub struct Proof {
    pub session: Arc<AccountSession>,
    pub store: Arc<StateStore>,
    pub output: PathBuf,
    pub plan: Value,
    guard: Arc<Guard>,
}
fn ids(list: &PlaylistList) -> Vec<String> {
    list.playlists.iter().map(|p| p.id.clone()).collect()
}
async fn confirmed(
    service: &PlaylistService,
    token: &SessionToken,
    command: PlaylistCommand,
) -> Result<PlaylistMutation> {
    let mut record = service.execute(token, command).await?;
    for _ in 0..3 {
        if !record.phase.pending() {
            break;
        }
        record = service.reconcile(token, &record.operation_id).await?;
    }
    if !matches!(
        record.phase,
        PlaylistPhase::Confirmed | PlaylistPhase::Unchanged
    ) {
        return Err(Error::MutationUncertain);
    }
    Ok(record)
}
impl Proof {
    pub async fn open(
        root: PathBuf,
        output: PathBuf,
        profile: &Path,
        existing: bool,
    ) -> Result<(Arc<Self>, MusicClient)> {
        let mut plan = if existing {
            serde_json::from_slice(&fs::read(root.join("plan.json"))?)?
        } else {
            json!({"name":format!("CumulusCoverUI{}", root.file_name().unwrap().to_string_lossy().chars().rev().take(12).collect::<String>())})
        };
        let credentials = Arc::new(Credentials::open(
            Arc::new(StateStore::open(profile)?),
            ApiOrigin::Pc,
        )?);
        let guard = Arc::new(Guard {
            inner: Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?),
            root: root.clone(),
            output: output.clone(),
            name: plan["name"]
                .as_str()
                .ok_or(Error::State("missing name"))?
                .into(),
            id: Mutex::new(plan["id"].as_str().map(str::to_owned)),
            sequence: AtomicUsize::new(0),
        });
        let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc)?;
        let store = registry.store();
        let session = Arc::new(AccountSession::new(guard.clone(), Some(store.clone())));
        session.refresh().await?;
        let token = session.token()?;
        let owner = token
            .uid
            .as_deref()
            .ok_or(Error::State("authenticated proof account required"))?;
        let service = PlaylistService::new(session.clone())?;
        if existing {
            if plan["owner"] != owner {
                return Err(Error::State("proof owner changed"));
            }
        } else {
            plan["owner"] = json!(owner);
            plan["ids"] = json!(ids(&service.list(&token).await?));
            save(&root.join("plan.json"), &plan)?;
            let created = confirmed(
                &service,
                &token,
                PlaylistCommand::Create {
                    name: guard.name.clone(),
                    private: true,
                },
            )
            .await?;
            let id = created
                .playlist_id
                .ok_or(Error::Response("create missing id"))?;
            *guard.id.lock().unwrap() = Some(id.clone());
            plan["id"] = json!(id);
            save(&root.join("created.json"), &plan)?;
        }
        // plan.json deliberately stays the pre-write cleanup plan. Discover by
        // owner/name/privacy and absence from its baseline if the ack was lost.
        if existing && plan["id"].is_null() && root.join("created.json").is_file() {
            let created: Value = serde_json::from_slice(&fs::read(root.join("created.json"))?)?;
            if created["owner"] != plan["owner"]
                || created["name"] != plan["name"]
                || created["ids"] != plan["ids"]
            {
                return Err(Error::State("cleanup plans differ"));
            }
            plan = created;
            *guard.id.lock().unwrap() = plan["id"].as_str().map(str::to_owned);
        }
        let client = MusicClient::with_session(session.clone(), registry);
        Ok((
            Arc::new(Self {
                session,
                store,
                output,
                plan,
                guard,
            }),
            client,
        ))
    }
    pub async fn observe(&self, value: &mut Value) -> Result<()> {
        let token = self.session.token()?;
        let covers = PlaylistCovers::new(self.session.clone())?;
        value["nativeTasks"] = serde_json::to_value(covers.list(&token)?)?;
        if let Some(id) = self.plan["id"].as_str() {
            value["nativeDetail"] = serde_json::to_value(
                PlaylistService::new(self.session.clone())?
                    .detail(&token, id)
                    .await?,
            )?;
        }
        Ok(())
    }
    pub async fn cleanup(&self) -> Result<()> {
        let token = self.session.token()?;
        let covers = PlaylistCovers::new(self.session.clone())?;
        let service = PlaylistService::new(self.session.clone())?;
        for task in covers.list(&token)? {
            let task = if task.phase == CoverPhase::Binding {
                covers.reconcile(&token, &task.id).await?
            } else {
                task
            };
            if task.phase == CoverPhase::Binding {
                return Err(Error::MutationUncertain);
            }
            if !task.phase.terminal() {
                covers.abandon(&token, &task.id)?;
            }
            covers.cleanup(&token, &task.id)?;
        }
        for op in service
            .operations(&token)?
            .into_iter()
            .filter(|v| v.phase.pending())
        {
            if service
                .reconcile(&token, &op.operation_id)
                .await?
                .phase
                .pending()
            {
                return Err(Error::MutationUncertain);
            }
        }
        let baseline: Vec<String> = serde_json::from_value(self.plan["ids"].clone())?;
        let current = service.list(&token).await?;
        let candidates: Vec<_> = current
            .playlists
            .iter()
            .filter(|p| {
                p.owner == self.plan["owner"]
                    && p.name == self.plan["name"]
                    && p.private
                    && !p.favorite
                    && !baseline.contains(&p.id)
            })
            .collect();
        if candidates.len() > 1 {
            return Err(Error::State("ambiguous cleanup candidate"));
        }
        if let Some(p) = candidates.first() {
            let detail = service.detail(&token, &p.id).await?;
            if !detail.tracks.is_empty()
                || detail.summary.owner != self.plan["owner"]
                || detail.summary.name != self.plan["name"]
                || !detail.summary.private
                || detail.summary.favorite
            {
                return Err(Error::State("cleanup target changed"));
            }
            *self.guard.id.lock().unwrap() = Some(p.id.clone());
            confirmed(
                &service,
                &token,
                PlaylistCommand::Delete {
                    id: p.id.clone(),
                    revision: detail.revision,
                },
            )
            .await?;
        }
        let after = service.list(&token).await?;
        if ids(&after) != baseline {
            return Err(Error::State("baseline order not restored"));
        }
        save(
            &self.output.join("cleanup.json"),
            &json!({"ok":true,"ids":ids(&after),"remoteImageDeleted":false,"orphanPossible":true}),
        )?;
        Ok(())
    }
}
