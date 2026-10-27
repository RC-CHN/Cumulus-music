//! Shared isolated-room proof guard and durable cleanup responsibility.
use cumulus_core::{
    Error, Result,
    account::{AccountSession, credentials::Credentials},
    listen_together::*,
    protocol::{
        api::{Api, ApiFuture},
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

struct GuardState {
    sequence: usize,
    owner: Option<String>,
    room: Option<Room>,
    alone: bool,
    created: bool,
    drop_ack: bool,
}
pub(super) struct Guard {
    inner: Arc<dyn Api>,
    root: PathBuf,
    restoring: bool,
    state: Mutex<GuardState>,
}
pub(super) fn save(root: &Path, name: &str, value: &Value) -> Result<()> {
    let mut file = fs::File::create(root.join(name))?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    Ok(())
}
impl Api for Guard {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let n = {
                let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
                let read = [
                    "/api/w/nuser/account/get",
                    "/api/v3/song/detail",
                    "/api/listen/together/status/get",
                ];
                if path == "/api/listen/together/room/create" {
                    if self.restoring || !state.alone || state.owner.is_none() || state.created {
                        return Err(Error::State(
                            "proof cannot create over existing/unknown room",
                        ));
                    }
                    state.created = true;
                } else if !read.contains(&path) {
                    let room = state
                        .room
                        .as_ref()
                        .ok_or(Error::State("proof has no acknowledged isolated room"))?;
                    let allowed = [
                        "/api/listen/together/room/check",
                        "/api/listen/together/sync/playlist/get",
                        "/api/listen/together/sync/list/command/report",
                        "/api/listen/together/play/command/report",
                        "/api/listen/together/heartbeat",
                        "/api/listen/together/end/v2",
                        "/api/middle/im/token/get",
                        "/api/listen/together/yunxin/token/get",
                        "/api/listen/together/sync/notice",
                    ];
                    if !allowed.contains(&path)
                        || (path != "/api/middle/im/token/get" && payload["roomId"] != room.room_id)
                    {
                        return Err(Error::State("proof attempted an unapproved room API"));
                    }
                    if self.restoring
                        && ![
                            "/api/listen/together/end/v2",
                            "/api/listen/together/sync/playlist/get",
                        ]
                        .contains(&path)
                    {
                        return Err(Error::State("restoration cannot repeat business commands"));
                    }
                }
                state.sequence += 1;
                state.sequence
            };
            let prefix = if self.restoring { "restore" } else { "call" };
            save(
                &self.root,
                &format!("{prefix}-{n:03}-request.json"),
                &json!({"path":path,"payload":payload,"cache":cache}),
            )?;
            let response = self.inner.request(path, payload, cache).await?;
            let mut redacted = response.clone();
            if path.ends_with("token/get") && response["data"].is_object() {
                redacted["data"]["token"] = json!("[redacted]");
            }
            save(
                &self.root,
                &format!("{prefix}-{n:03}-response.json"),
                &redacted,
            )?;
            let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
            if path == "/api/w/nuser/account/get" && response["code"] == 200 {
                let uid = cumulus_core::protocol::api::identifier(&response["profile"]["userId"])?;
                if state.owner.as_ref().is_some_and(|old| old != &uid) {
                    return Err(Error::State("restoration account differs"));
                }
                state.owner = Some(uid);
            }
            if path == "/api/listen/together/status/get" && response["code"] == 200 {
                state.alone = response["data"]["inRoom"] == false;
                if response["data"]["inRoom"] == true {
                    let r = Room::from_official(&response["data"]["roomInfo"])?;
                    if state.room.as_ref().is_some_and(|old| {
                        old.room_id != r.room_id
                            || r.members.len() != 1
                            || Some(&r.creator_id) != state.owner.as_ref()
                    }) {
                        return Err(Error::State(
                            "isolated room changed or acquired a participant",
                        ));
                    }
                }
            }
            if path == "/api/listen/together/room/create"
                && response["code"] == 200
                && response["data"]["type"] == "NEW_ROOM"
            {
                let room = Room::from_official(&response["data"]["roomInfo"])?;
                if Some(&room.creator_id) != state.owner.as_ref() || room.members.len() != 1 {
                    return Err(Error::Response("proof did not create isolated own room"));
                }
                save(
                    &self.root,
                    "cleanup-intent.json",
                    &json!({"owner":state.owner,"room":room,"new_room_acknowledged":true}),
                )?;
                state.room = Some(room);
                state.alone = false;
            }
            if path == "/api/listen/together/play/command/report"
                && response["code"] == 200
                && response["data"]["result"] == true
                && state.drop_ack
            {
                state.drop_ack = false;
                save(
                    &self.root,
                    "injected-lost-ack.json",
                    &json!({"sequence":n,"server_success":true}),
                )?;
                return Err(Error::Transport(
                    "proof injected lost successful play acknowledgement".into(),
                ));
            }
            Ok(response)
        })
    }
}
pub(super) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
pub(super) async fn setup(
    profile: &Path,
    root: &Path,
    restoring: bool,
    drop_ack: bool,
) -> Result<(Arc<AccountSession>, Arc<Guard>)> {
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
        state: Mutex::new(GuardState {
            sequence: 0,
            owner: saved
                .as_ref()
                .and_then(|v| v["owner"].as_str())
                .map(str::to_owned),
            room: saved
                .as_ref()
                .map(|v| serde_json::from_value(v["room"].clone()))
                .transpose()?,
            alone: false,
            created: restoring,
            drop_ack: drop_ack && !restoring,
        }),
    });
    let session = Arc::new(AccountSession::new(
        guard.clone(),
        Some(Arc::new(StateStore::open(root.join("state"))?)),
    ));
    session.refresh().await?;
    Ok((session, guard))
}
pub(super) async fn cleanup(session: Arc<AccountSession>, guard: &Guard) -> Result<()> {
    let token = session.token()?;
    let service = TogetherService::new(session.clone())?;
    let intended = guard
        .state
        .lock()
        .map_err(|_| Error::LockPoisoned)?
        .room
        .clone();
    let Some(intended) = intended else {
        return Ok(());
    };
    let mut reconciliation_errors = Vec::new();
    for op in service.operations_for(&token)? {
        if op.phase.pending()
            && let Err(error) = service.reconcile_for(&token, &op.id, now()).await
        {
            reconciliation_errors.push(error.to_string());
        }
    }
    let found = session
        .run_for(&token, |api, _| async move {
            TogetherApi::new(api).status().await
        })
        .await?;
    if let Some(room) = found {
        if room.room_id != intended.room_id || room.members.len() != 1 {
            return Err(Error::State(
                "proof cleanup cannot touch another room/participant",
            ));
        }
        let result = service
            .end_for(&token, &intended.room_id, None, now())
            .await?;
        if result.phase != TogetherPhase::Confirmed {
            return Err(Error::MutationUncertain);
        }
    }
    for _ in 0..2 {
        if session
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
        .filter(|o| o.phase.pending())
        .count();
    save(
        &guard.root,
        "restored.json",
        &json!({"no_room":true,"two_reads":true,"pending":pending,"reconciliation_errors":reconciliation_errors}),
    )?;
    if pending != 0 {
        return Err(Error::MutationUncertain);
    }
    Ok(())
}
