//! A real-account proof may mutate only its acknowledged, single-member NEW_ROOM.
use cumulus_core::{
    Error, Result,
    account::{AccountSession, credentials::Credentials, profiles::ProfileRegistry},
    listen_together::{Room, TogetherApi, TogetherPhase, TogetherService},
    protocol::{
        api::{Api, ApiFuture, identifier},
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

pub fn save(root: &Path, name: &str, value: &Value) -> Result<()> {
    let mut file = fs::File::create(root.join(name))?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    Ok(())
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
struct State {
    sequence: usize,
    writes: usize,
    owner: Option<String>,
    room: Option<Room>,
    absent: bool,
    attempted_create: bool,
}
pub struct Guard {
    inner: Arc<dyn Api>,
    root: PathBuf,
    restoring: bool,
    state: Mutex<State>,
    pub drop_play: AtomicBool,
    pub drop_end: AtomicBool,
}
impl Guard {
    pub fn proof_counts(&self) -> Value {
        let state = self.state.lock().unwrap();
        json!({"requests":state.sequence,"writes":state.writes})
    }
}
impl Api for Guard {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let (n, write) = {
                let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
                let read = matches!(
                    path,
                    "/api/w/nuser/account/get"
                        | "/api/user/playlist"
                        | "/api/song/like/get"
                        | "/api/v3/discovery/recommend/songs"
                        | "/api/v3/song/detail"
                        | "/api/song/enhance/player/url/v1"
                        | "/api/song/lyric/v1"
                        | "/api/listen/together/status/get"
                );
                let write = if path == "/api/listen/together/room/create" {
                    if self.restoring
                        || !state.absent
                        || state.owner.is_none()
                        || state.attempted_create
                    {
                        return Err(Error::State(
                            "proof cannot create over existing/unknown room",
                        ));
                    }
                    state.attempted_create = true;
                    true
                } else if !read {
                    let room = state
                        .room
                        .as_ref()
                        .ok_or(Error::State("proof has no acknowledged isolated room"))?;
                    let allowed = matches!(
                        path,
                        "/api/listen/together/sync/playlist/get"
                            | "/api/listen/together/sync/list/command/report"
                            | "/api/listen/together/play/command/report"
                            | "/api/listen/together/heartbeat"
                            | "/api/listen/together/end/v2"
                            | "/api/middle/im/token/get"
                    );
                    if !allowed
                        || (path != "/api/middle/im/token/get" && payload["roomId"] != room.room_id)
                    {
                        return Err(Error::State("proof attempted an unapproved room API"));
                    }
                    if self.restoring
                        && !matches!(
                            path,
                            "/api/listen/together/end/v2"
                                | "/api/listen/together/sync/playlist/get"
                        )
                    {
                        return Err(Error::State("restoration cannot repeat business commands"));
                    }
                    !matches!(
                        path,
                        "/api/listen/together/sync/playlist/get" | "/api/middle/im/token/get"
                    )
                } else {
                    false
                };
                if write {
                    state.writes += 1;
                    if state.writes > 50 {
                        return Err(Error::State("proof write limit"));
                    }
                }
                state.sequence += 1;
                (state.sequence, write)
            };
            let prefix = if self.restoring { "restore" } else { "call" };
            save(
                &self.root,
                &format!("{prefix}-{n:03}-request.json"),
                &json!({"path":path,"payload":payload,"cache":cache,"write":write}),
            )?;
            let result = self.inner.request(path, payload, cache).await;
            // Only room replies are retained in full. Media URLs, credentials,
            // nickname and the remaining account-resource responses are omitted.
            save(
                &self.root,
                &format!("{prefix}-{n:03}-response.json"),
                &match &result {
                    Ok(v) if path.starts_with("/api/listen/together/") => v.clone(),
                    Ok(v) => json!({"code":v["code"],"ok":true}),
                    Err(_) => json!({"ok":false}),
                },
            )?;
            let response = result?;
            let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
            if path == "/api/w/nuser/account/get" && response["code"] == 200 {
                let owner = identifier(&response["profile"]["userId"])?;
                if state.owner.as_ref().is_some_and(|old| old != &owner) {
                    return Err(Error::State("proof account changed"));
                }
                state.owner = Some(owner);
            }
            if path == "/api/listen/together/status/get" && response["code"] == 200 {
                state.absent = response["data"]["inRoom"] == false;
                if response["data"]["inRoom"] == true {
                    let found = Room::from_official(&response["data"]["roomInfo"])?;
                    if state
                        .room
                        .as_ref()
                        .is_none_or(|r| r.room_id != found.room_id)
                        || found.members.len() != 1
                        || Some(&found.creator_id) != state.owner.as_ref()
                    {
                        return Err(Error::State("proof found an existing or changed room"));
                    }
                }
            }
            if path == "/api/listen/together/room/create"
                && response["code"] == 200
                && response["data"]["type"] == "NEW_ROOM"
            {
                let room = Room::from_official(&response["data"]["roomInfo"])?;
                if Some(&room.creator_id) != state.owner.as_ref() || room.members.len() != 1 {
                    return Err(Error::State("proof room not isolated"));
                }
                // This file is fsynced before the product sees its success reply.
                save(
                    &self.root,
                    "cleanup-intent.json",
                    &json!({"owner":state.owner,"room":room,"new_room_acknowledged":true}),
                )?;
                state.room = Some(room);
                state.absent = false;
            }
            let drop = if path == "/api/listen/together/play/command/report"
                && response["code"] == 200
                && response["data"]["result"] == true
            {
                self.drop_play.swap(false, Ordering::SeqCst)
            } else if path == "/api/listen/together/end/v2"
                && response["code"] == 200
                && response["data"]["success"] == true
            {
                self.drop_end.swap(false, Ordering::SeqCst)
            } else {
                false
            };
            if drop {
                save(
                    &self.root,
                    &format!("injected-{n:03}.json"),
                    &json!({"sequence":n,"path":path,"server_success":true}),
                )?;
                return Err(Error::Transport(
                    "proof dropped a successful acknowledgement".into(),
                ));
            }
            Ok(response)
        })
    }
}
pub struct Harness {
    pub root: PathBuf,
    pub session: Arc<AccountSession>,
    pub registry: Arc<ProfileRegistry>,
    pub guard: Arc<Guard>,
}
impl Harness {
    pub async fn open(root: &Path, profile: &Path, restoring: bool) -> Result<Self> {
        let saved = if restoring {
            Some(serde_json::from_slice::<Value>(&fs::read(
                root.join("cleanup-intent.json"),
            )?)?)
        } else {
            None
        };
        let credentials = Arc::new(Credentials::open(
            Arc::new(StateStore::open(profile)?),
            ApiOrigin::Pc,
        )?);
        let guard = Arc::new(Guard {
            inner: Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?),
            root: root.into(),
            restoring,
            state: Mutex::new(State {
                sequence: 0,
                writes: 0,
                owner: saved
                    .as_ref()
                    .and_then(|v| v["owner"].as_str())
                    .map(str::to_owned),
                room: saved
                    .as_ref()
                    .map(|v| serde_json::from_value(v["room"].clone()))
                    .transpose()?,
                absent: false,
                attempted_create: restoring,
            }),
            drop_play: AtomicBool::new(false),
            drop_end: AtomicBool::new(false),
        });
        let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc)?;
        let session = Arc::new(AccountSession::new(guard.clone(), Some(registry.store())));
        session.refresh().await?;
        let token = session.token()?;
        session
            .run_for(&token, |api, _| async move {
                TogetherApi::new(api).status().await
            })
            .await?;
        if !restoring {
            save(
                root,
                "preflight.json",
                &json!({"no_existing_room":true,"account_verified":true}),
            )?;
        }
        Ok(Self {
            root: root.into(),
            session,
            registry,
            guard,
        })
    }
    pub async fn restore(&self) -> Result<()> {
        let token = self.session.token()?;
        let service = TogetherService::new(self.session.clone())?;
        let intended = self
            .guard
            .state
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .room
            .clone();
        let mut errors = vec![];
        for op in service.operations_for(&token)? {
            if op.phase.pending() && service.reconcile_for(&token, &op.id, now()).await.is_err() {
                errors.push(op.id);
            }
        }
        let found = self
            .session
            .run_for(&token, |api, _| async move {
                TogetherApi::new(api).status().await
            })
            .await?;
        if let Some(room) = found {
            if intended.as_ref().is_none_or(|r| r.room_id != room.room_id)
                || room.members.len() != 1
            {
                return Err(Error::State("cleanup cannot touch another room"));
            }
            if service
                .end_for(&token, &room.room_id, None, now())
                .await?
                .phase
                != TogetherPhase::Confirmed
            {
                return Err(Error::MutationUncertain);
            }
        }
        for _ in 0..2 {
            if self
                .session
                .run_for(&token, |api, _| async move {
                    TogetherApi::new(api).status().await
                })
                .await?
                .is_some()
            {
                return Err(Error::State("room cleanup not confirmed"));
            }
        }
        let pending = service
            .operations_for(&token)?
            .iter()
            .filter(|op| op.phase.pending())
            .count();
        save(
            &self.root,
            "restored.json",
            &json!({"no_room":true,"two_reads":true,"pending":pending,"reconciliation_errors":errors}),
        )?;
        if pending > 0 {
            return Err(Error::MutationUncertain);
        }
        Ok(())
    }
}
