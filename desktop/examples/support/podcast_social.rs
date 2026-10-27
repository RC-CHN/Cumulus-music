//! Limits real-account proof writes to three explicitly recorded resources.
use cumulus_core::{
    Error, Result,
    account::{AccountSession, credentials::Credentials, profiles::ProfileRegistry},
    podcast::{LibraryKind, PodcastAction, PodcastApi, PodcastService},
    protocol::{
        api::{Api, ApiFuture, identifier},
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
    storage::StateStore,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
#[derive(Clone, Serialize, Deserialize)]
pub struct Plan {
    pub owner: String,
    pub radio: String,
    pub voice: String,
    pub resume_radio: String,
    pub resume_before: bool,
    pub podcasts: Vec<String>,
    pub books: Vec<String>,
    pub liked: Vec<String>,
}
pub fn save(root: &Path, name: &str, value: &Value) -> Result<()> {
    let mut f = fs::File::create(root.join(name))?;
    f.write_all(&serde_json::to_vec_pretty(value)?)?;
    f.sync_all()?;
    Ok(())
}
pub struct Guard {
    inner: Arc<dyn Api>,
    root: PathBuf,
    radio: String,
    voice: String,
    resume_radio: String,
    armed: AtomicBool,
    drop_like_ack: AtomicBool,
    calls: Mutex<Vec<Value>>,
}
impl Api for Guard {
    fn request<'a>(&'a self, path: &'a str, p: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let write = match path {
                "/api/djradio/sub" | "/api/djradio/unsub" => {
                    if p["id"] != self.radio {
                        return Err(Error::State("subscription outside isolated target"));
                    }
                    true
                }
                "/api/resource/like" | "/api/resource/unlike" => {
                    if p["threadId"] != format!("A_DJ_1_{}", self.voice) {
                        return Err(Error::State("like outside isolated target"));
                    }
                    true
                }
                "/api/dj/user/setting/resumeplay/update" => {
                    if p["radioId"] != self.resume_radio
                        || !matches!(p["resumePlayValue"].as_u64(), Some(0 | 1))
                    {
                        return Err(Error::State("resume outside isolated target"));
                    }
                    true
                }
                "/api/w/nuser/account/get"
                | "/api/user/playlist"
                | "/api/song/like/get"
                | "/api/v3/discovery/recommend/songs"
                | "/api/v3/song/detail"
                | "/api/v1/cloud/get"
                | "/api/my/podcast/page/list"
                | "/api/content/my/liked/voice"
                | "/api/djradio/v3/get"
                | "/api/v6/dj/program/byradio"
                | "/api/dj/program/detail"
                | "/api/dj/play/url/check"
                | "/api/dj/user/setting/resumeplay/get"
                | "/api/song/enhance/player/url/v1"
                | "/api/song/lyric/v1" => false,
                _ => return Err(Error::State("proof API is not allowed")),
            };
            if write && !self.armed.load(Ordering::SeqCst) {
                return Err(Error::State("restore intent not durable"));
            }
            let sequence = {
                let mut calls = self.calls.lock().map_err(|_| Error::LockPoisoned)?;
                if write && calls.iter().filter(|v| v["write"] == true).count() >= 8 {
                    return Err(Error::State("proof mutation limit"));
                }
                calls.push(json!({"path":path,"payload":p,"write":write}));
                calls.len()
            };
            save(
                &self.root,
                &format!("request-{sequence:03}.json"),
                &json!({"path":path,"payload":p,"write":write}),
            )?;
            let response = self.inner.request(path, p, cache).await;
            let dropped = path == "/api/resource/like"
                && response.as_ref().is_ok_and(|v| v["code"] == 200)
                && self.drop_like_ack.swap(false, Ordering::SeqCst);
            save(
                &self.root,
                &format!("result-{sequence:03}.json"),
                &match &response {
                    Ok(v) => json!({"code":v["code"],"ok":true,"ack_dropped":dropped}),
                    Err(_) => json!({"ok":false}),
                },
            )?;
            if dropped {
                Err(Error::State("injected loss after server accepted like"))
            } else {
                response
            }
        })
    }
}
pub struct Harness {
    pub session: Arc<AccountSession>,
    pub registry: Arc<ProfileRegistry>,
    pub plan: Plan,
    pub root: PathBuf,
}
impl Harness {
    pub async fn open(
        root: &Path,
        profile: &Path,
        radio: &str,
        voice: &str,
        resume_radio: &str,
        restore: bool,
    ) -> Result<Self> {
        let previous: Option<Plan> = if restore {
            Some(serde_json::from_slice(&fs::read(
                root.join("restore-intent.json"),
            )?)?)
        } else {
            None
        };
        let (radio, voice, resume_radio) = if let Some(p) = &previous {
            (p.radio.clone(), p.voice.clone(), p.resume_radio.clone())
        } else {
            (
                identifier(&json!(radio))?,
                identifier(&json!(voice))?,
                identifier(&json!(resume_radio))?,
            )
        };
        let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc)?;
        let credentials = Arc::new(Credentials::open(
            Arc::new(StateStore::open(profile)?),
            ApiOrigin::Pc,
        )?);
        let guard = Arc::new(Guard {
            inner: Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?),
            root: root.join(if restore { "restore-calls" } else { "calls" }),
            radio: radio.clone(),
            voice: voice.clone(),
            resume_radio: resume_radio.clone(),
            armed: AtomicBool::new(false),
            drop_like_ack: AtomicBool::new(
                !restore && std::env::var_os("CUMULUS_PROOF_DROP_ACK").is_some(),
            ),
            calls: Mutex::new(vec![]),
        });
        fs::create_dir_all(&guard.root)?;
        let session = Arc::new(AccountSession::new(guard.clone(), Some(registry.store())));
        session.refresh().await?;
        let token = session.token()?;
        let owner = token
            .uid
            .clone()
            .ok_or(Error::State("proof requires an account"))?;
        let plan = if let Some(p) = previous {
            if p.owner != owner {
                return Err(Error::StaleResponse);
            }
            p
        } else {
            let facts = session
                .run_for(&token, |api, _| async {
                    let a = PodcastApi::new(api);
                    Ok((
                        a.my_library(LibraryKind::Podcast).await?,
                        a.my_library(LibraryKind::Audiobook).await?,
                        a.liked_voices().await?,
                        a.radio(&json!(radio)).await?,
                        a.episode(&json!(voice)).await?,
                        a.resume_enabled(&json!(resume_radio)).await?,
                    ))
                })
                .await?;
            let podcasts = facts
                .0
                .ids
                .iter()
                .map(identifier)
                .collect::<Result<Vec<_>>>()?;
            let books = facts
                .1
                .ids
                .iter()
                .map(identifier)
                .collect::<Result<Vec<_>>>()?;
            let liked = facts.2.iter().map(|v| v.id.clone()).collect::<Vec<_>>();
            if podcasts.contains(&radio)
                || books.contains(&radio)
                || facts.3.subscribed != Some(false)
                || liked.contains(&voice)
                || facts.4.radio.as_ref().map(|r| r.id.as_str()) != Some(&resume_radio)
            {
                return Err(Error::State(
                    "test target is not isolated from existing account resources",
                ));
            }
            let plan = Plan {
                owner,
                radio,
                voice,
                resume_radio,
                resume_before: facts.5,
                podcasts,
                books,
                liked,
            };
            let mut file = fs::File::create_new(root.join("restore-intent.json"))?;
            file.write_all(&serde_json::to_vec_pretty(&plan)?)?;
            file.sync_all()?;
            plan
        };
        guard.armed.store(true, Ordering::SeqCst);
        Ok(Self {
            session,
            registry,
            plan,
            root: root.into(),
        })
    }
    pub async fn restore(&self) -> Result<Value> {
        let token = self.session.token()?;
        if token.uid.as_deref() != Some(&self.plan.owner) {
            return Err(Error::StaleResponse);
        }
        let service = PodcastService::new(self.session.clone())?;
        for (action, id, before) in [
            (PodcastAction::Subscribe, &self.plan.radio, false),
            (PodcastAction::Like, &self.plan.voice, false),
            (
                PodcastAction::Resume,
                &self.plan.resume_radio,
                self.plan.resume_before,
            ),
        ] {
            if service
                .operation_for(&token, action, id)?
                .is_some_and(|v| v.phase.pending())
            {
                let _ = service.reconcile_for(&token, action, id).await;
            }
            if service.state_for(&token, action, id).await? != before {
                service.change_for(&token, action, id, before).await?;
            }
            for _ in 0..2 {
                if service.state_for(&token, action, id).await? != before {
                    return Err(Error::State("baseline setting did not settle"));
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
        let (podcasts, books, liked) = self
            .session
            .run_for(&token, |api, _| async {
                let a = PodcastApi::new(api);
                Ok((
                    a.my_library(LibraryKind::Podcast).await?,
                    a.my_library(LibraryKind::Audiobook).await?,
                    a.liked_voices().await?,
                ))
            })
            .await?;
        let podcasts = podcasts
            .ids
            .iter()
            .map(identifier)
            .collect::<Result<Vec<_>>>()?;
        let books = books
            .ids
            .iter()
            .map(identifier)
            .collect::<Result<Vec<_>>>()?;
        let liked = liked.iter().map(|v| v.id.clone()).collect::<Vec<_>>();
        let pending = service
            .operations_for(&token)?
            .iter()
            .filter(|r| r.phase.pending())
            .count();
        let value = json!({"subscription_ids_and_order_restored":podcasts==self.plan.podcasts&&books==self.plan.books,"liked_ids_and_order_restored":liked==self.plan.liked,"resume_restored":true,"pending":pending,"podcasts":podcasts.len(),"audiobooks":books.len(),"liked":liked.len()});
        save(&self.root, "baseline-verified.json", &value)?;
        if value["subscription_ids_and_order_restored"] != true
            || value["liked_ids_and_order_restored"] != true
            || pending != 0
        {
            return Err(Error::State("baseline verification remains unresolved"));
        }
        Ok(value)
    }
}
