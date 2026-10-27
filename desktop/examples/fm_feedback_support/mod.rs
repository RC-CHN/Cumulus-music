use cumulus_core::{
    Error, Result,
    account::{AccountSession, credentials::Credentials, profiles::ProfileRegistry},
    client::MusicClient,
    fm::{FmBlacklist, FmService, FmSource},
    protocol::{
        api::{Api, ApiFuture},
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
    storage::StateStore,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Serialize, Deserialize)]
pub struct Plan {
    owner: String,
    pub song_id: String,
    pub artist_id: String,
    pub reason_key: String,
    pub entry_id: String,
    trash: BTreeSet<String>,
    blacklist: FmBlacklist,
}
pub fn save(path: &Path, value: &Value) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    fs::write(path, bytes)?;
    fs::File::open(path)?.sync_all()?;
    Ok(())
}
struct Guard {
    api: Arc<dyn Api>,
    plan: Mutex<Option<Plan>>,
    calls: Mutex<Vec<Value>>,
    output: PathBuf,
    lose_artist: bool,
    lost: AtomicBool,
}
impl Api for Guard {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let mutation = matches!(
                path,
                "/api/radio/trash/add"
                    | "/api/radio/trash/del/batch"
                    | "/api/music-blacklist/add"
                    | "/api/music-blacklist/delete"
            );
            let permitted = if mutation {
                let plan = self.plan.lock().unwrap();
                plan.as_ref().is_some_and(|p| match path {
                    "/api/radio/trash/add" => {
                        payload["songId"] == p.song_id && !p.trash.contains(&p.song_id)
                    }
                    "/api/radio/trash/del/batch" => {
                        payload["songIds"] == json!([p.song_id]) && !p.trash.contains(&p.song_id)
                    }
                    _ => {
                        payload["contentType"] == "artist"
                            && payload["contentIdList"] == json!([p.artist_id])
                            && !p.blacklist.artists.contains(&p.artist_id)
                    }
                })
            } else {
                matches!(
                    path,
                    "/api/w/nuser/account/get"
                        | "/api/user/playlist"
                        | "/api/song/like/get"
                        | "/api/v3/discovery/recommend/songs"
                        | "/api/v3/song/detail"
                        | "/api/song/enhance/player/url/v1"
                        | "/api/song/lyric/v1"
                        | "/api/music-vip-membership/client/vip/info"
                        | "/api/v1/radio/get"
                        | "/api/v3/radio/trash/get"
                        | "/api/music/dislike/get"
                        | "/api/personalized/dislike/reason"
                )
            };
            if !permitted {
                save(
                    &self.output.join("blocked-endpoint.json"),
                    &json!({"path":path}),
                )?;
                return Err(Error::State("FM feedback proof request outside plan"));
            }
            let logged = if mutation {
                let mut v = payload.clone();
                if let Some(o) = v.as_object_mut()
                    && let Some(alg) = o.remove("alg")
                {
                    o.insert(
                        "algorithm_present".into(),
                        json!(!alg.as_str().unwrap_or_default().is_empty()),
                    );
                }
                v
            } else {
                Value::Null
            };
            let index = {
                let mut calls = self.calls.lock().unwrap();
                let index = calls.len();
                calls
                    .push(json!({"path":path,"params":logged,"mutation":mutation,"response":null}));
                save(&self.output.join("api-calls.json"), &json!(*calls))?;
                index
            };
            let result = self.api.request(path, payload, cache).await;
            let inject = self.lose_artist
                && path == "/api/music-blacklist/add"
                && result.as_ref().is_ok_and(|v| v["code"] == 200)
                && !self.lost.swap(true, Ordering::AcqRel);
            {
                let mut calls = self.calls.lock().unwrap();
                calls[index]["response"] = result
                    .as_ref()
                    .map(|v| v["code"].clone())
                    .unwrap_or(Value::Null);
                calls[index]["simulated_lost_reply"] = json!(inject);
                save(&self.output.join("api-calls.json"), &json!(*calls))?;
            }
            if inject {
                return Err(Error::Transport(
                    "proof: successful artist response withheld".into(),
                ));
            }
            result
        })
    }
}
pub struct Proof {
    session: Arc<AccountSession>,
    fm: Arc<FmService>,
    api: Arc<Guard>,
    root: PathBuf,
}
impl Proof {
    pub fn open(
        root: &Path,
        output: &Path,
        profile: &Path,
        lose_artist: bool,
        restore: bool,
    ) -> Result<(Arc<Self>, MusicClient)> {
        let credentials = Arc::new(Credentials::open(
            Arc::new(StateStore::open(profile)?),
            ApiOrigin::Pc,
        )?);
        let plan = if restore {
            Some(serde_json::from_slice::<Plan>(&fs::read(
                root.join("plan.json"),
            )?)?)
        } else {
            None
        };
        let api = Arc::new(Guard {
            api: Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?),
            plan: Mutex::new(plan),
            calls: Mutex::default(),
            output: output.into(),
            lose_artist,
            lost: AtomicBool::new(false),
        });
        let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc)?;
        let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
        let fm = Arc::new(FmService::new(session.clone())?);
        let client = MusicClient::with_session(session.clone(), registry);
        Ok((
            Arc::new(Self {
                session,
                fm,
                api,
                root: root.into(),
            }),
            client,
        ))
    }
    pub async fn prepare(&self, entry: &str) -> Result<Plan> {
        let token = self.session.token()?;
        let owner = token
            .uid
            .clone()
            .ok_or(Error::State("FM proof requires account"))?;
        let reasons = self
            .fm
            .reason_choices_for(&token, &FmSource::Personal, entry)
            .await?;
        let reason = reasons
            .options
            .iter()
            .find(|r| r.artist_id.is_some())
            .ok_or(Error::State("no artist reason for proof"))?;
        let trash = self.fm.refresh_trash_for(&token).await?.ids;
        let blacklist = self.fm.refresh_blacklist_for(&token).await?;
        let artist = reason.artist_id.clone().unwrap();
        if trash.contains(&reasons.song_id)
            || blacklist.artists.contains(&artist)
            || blacklist.songs.contains(&reasons.song_id)
        {
            return Err(Error::State("proof target already blocked"));
        }
        let plan = Plan {
            owner,
            song_id: reasons.song_id,
            artist_id: artist,
            reason_key: reason.key.clone(),
            entry_id: entry.into(),
            trash,
            blacklist,
        };
        save(&self.root.join("plan.json"), &serde_json::to_value(&plan)?)?;
        #[cfg(unix)]
        fs::File::open(&self.root)?.sync_all()?;
        *self.api.plan.lock().unwrap() = Some(plan.clone());
        Ok(plan)
    }
    pub async fn observe(&self) -> Result<Value> {
        let token = self.session.token()?;
        let trash = self.fm.refresh_trash_for(&token).await?;
        let blacklist = self.fm.refresh_blacklist_for(&token).await?;
        Ok(json!({"trash":trash.ids,"blacklist":blacklist,
            "song_operations":self.fm.trash_operations_for(&token)?.iter().map(|r|json!({"id":r.id,"ids":r.song_ids,"artist":r.artist_id,"phase":r.phase,"desired":r.desired})).collect::<Vec<_>>(),
            "artist_operations":self.fm.artist_operations_for(&token)?}))
    }
    pub async fn cleanup(&self) -> Result<Value> {
        let plan = self.api.plan.lock().unwrap().clone();
        let Some(plan) = plan else {
            return Ok(json!({"ok":true,"armed":false}));
        };
        self.session.refresh().await?;
        let token = self.session.token()?;
        let owner = token
            .uid
            .clone()
            .ok_or(Error::State("FM proof requires account"))?;
        if owner != plan.owner {
            return Err(Error::State("FM proof account changed"));
        }
        let trash = self.fm.refresh_trash_for(&token).await?;
        if trash.ids.contains(&plan.song_id) {
            for r in self.fm.trash_operations_for(&token)? {
                if r.song_ids.contains(&plan.song_id) && r.phase.pending() {
                    self.fm.reconcile_trash_for(&token, &r.id).await?;
                }
            }
            self.fm
                .restore_trash_for(&token, vec![plan.song_id.clone()])
                .await?;
        }
        let blacklist = self.fm.refresh_blacklist_for(&token).await?;
        if blacklist.artists.contains(&plan.artist_id) {
            for r in self.fm.artist_operations_for(&token)? {
                if r.artist_id == plan.artist_id && r.phase.pending() {
                    self.fm.reconcile_artist_for(&token, &r.id).await?;
                }
            }
            self.fm
                .change_artist_for(&token, &plan.artist_id, false)
                .await?;
        }
        let after_trash = self.fm.refresh_trash_for(&token).await?;
        let after_blacklist = self.fm.refresh_blacklist_for(&token).await?;
        let ok = after_trash.ids == plan.trash && after_blacklist == plan.blacklist;
        let report = json!({"ok":ok,"armed":true,"trash_restored":after_trash.ids==plan.trash,"blacklist_restored":after_blacklist==plan.blacklist,"after_trash":after_trash.ids,"after_blacklist":after_blacklist});
        save(&self.api.output.join("baseline-verified.json"), &report)?;
        if !ok {
            return Err(Error::State("FM proof baseline differs"));
        }
        Ok(report)
    }
}
