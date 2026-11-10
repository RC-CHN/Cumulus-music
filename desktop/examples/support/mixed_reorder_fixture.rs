//! Same local WAV and account as the mixed membership proof, but a clean
//! playlist and a closed two-write server for reorder.
use cumulus_core::{
    Error, Result,
    protocol::api::{
        Api, ApiFuture, GuardedPlaylistAdd, GuardedPlaylistRemove, PlaylistAddPermission,
        identifier,
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;

const TARGET: &str = "11";
const REORDER: &str = "/api/v1/playlist/manipulate/tracks";
const SOURCE: &str = "/api/song/enhance/player/url/v1";
const JOURNAL: &str = "playlist-local-reorders-v1";
const REMOVE_JOURNAL: &str = "playlist-local-removals-v1";
const ADD_JOURNAL: &str = "playlist-local-additions-v1";

pub const STAGES: &[&str] = &[
    "ux_01_outside_ready",
    "ux_02_outside_marker",
    "ux_03_outside_cancelled",
    "ux_04_local_ready",
    "ux_05_local_marker",
    "ux_06_sending",
    "ux_07_acknowledged",
    "ux_08_online_ready",
    "ux_09_online_marker",
    "ux_10_uncertain",
    "ux_11_readback",
];

pub const REMOVE_STAGES: &[&str] = &[
    "ux_01_selection_ready",
    "ux_02_remove_menu",
    "ux_03_confirm",
    "ux_04_cancelled",
    "ux_05_uncertain",
    "ux_06_reopened",
    "ux_07_readback",
];

pub const ADD_STAGES: &[&str] = &[
    "ux_01_source",
    "ux_02_picker",
    "ux_03_uncertain",
    "ux_04_earlier",
    "ux_05_readback",
    "ux_06_target",
];

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Reorder,
    Remove,
    Add,
    BatchAdd,
}
impl Mode {
    fn adding(self) -> bool {
        matches!(self, Self::Add | Self::BatchAdd)
    }
}

struct Server {
    order: [&'static str; 2],
    removed: bool,
    added: usize,
    writes: Vec<Value>,
    reads: usize,
    unexpected: usize,
    audio_checks: Vec<String>,
}

impl Server {
    fn visible_order(&self) -> &[&'static str] {
        if self.removed {
            &self.order[1..]
        } else if self.added == 2 {
            &["703", "704", "701", "702"]
        } else if self.added == 1 {
            &["703", "701", "702"]
        } else {
            &self.order
        }
    }
}

pub struct Fixture {
    base: super::mixed_playlist_fixture::Fixture,
    mode: Mode,
    server: Mutex<Server>,
    acknowledge: Notify,
    entered: Notify,
}

impl Fixture {
    pub fn new(root: &Path, mode: Mode) -> std::io::Result<Self> {
        Ok(Self {
            base: super::mixed_playlist_fixture::Fixture::new(root)?,
            mode,
            server: Mutex::new(Server {
                order: ["701", "702"],
                removed: false,
                added: 0,
                writes: Vec::new(),
                reads: 0,
                unexpected: 0,
                audio_checks: Vec::new(),
            }),
            acknowledge: Notify::new(),
            entered: Notify::new(),
        })
    }

    pub async fn first_send(&self) -> bool {
        if !self.server.lock().unwrap().writes.is_empty() {
            return true;
        }
        tokio::time::timeout(Duration::from_secs(5), self.entered.notified())
            .await
            .is_ok()
    }

    pub fn release(&self) {
        assert_eq!(self.server.lock().unwrap().writes.len(), 1);
        self.acknowledge.notify_one();
    }

    pub fn receipt(&self, store: &StateStore) -> Result<Value> {
        let kind = match self.mode {
            Mode::Reorder => JOURNAL,
            Mode::Remove => REMOVE_JOURNAL,
            Mode::Add | Mode::BatchAdd => ADD_JOURNAL,
        };
        let journal = store.get("2", kind, TARGET)?.unwrap_or(Value::Null);
        let server = self.server.lock().unwrap();
        Ok(json!({
            "membership":self.base.receipt(store)?,
            "writes":server.writes,
            "detailReads":server.reads,
            "onlineOrder":server.visible_order(),
            "unexpected":server.unexpected,
            "audioChecks":server.audio_checks,
            "journal":{
                "phase":journal["phase"],
                "operation":journal["operation"],
                "evidence":journal["evidence"],
                "generation":journal["generation"],
                "beforeOnline":journal["before_online"],
                "desiredOnline":journal["desired_online"],
                "desiredPosition":journal["desired_local"][0]["position"]
            }
        }))
    }

    async fn update(&self, args: Value, cache: bool) -> Result<Value> {
        if self.mode != Mode::Reorder {
            let (op, ids): (&str, &[&str]) = match self.mode {
                Mode::Remove => ("del", &["701"]),
                Mode::Add => ("add", &["703"]),
                Mode::BatchAdd => ("add", &["703", "704"]),
                Mode::Reorder => unreachable!(),
            };
            let mut server = self.server.lock().unwrap();
            if cache
                || !server.writes.is_empty()
                || args != json!({"pid":TARGET,"op":op,"trackIds":serde_json::to_string(ids)?})
            {
                server.unexpected += 1;
                return Err(Error::Response(
                    "unsafe or replayed synthetic mixed removal",
                ));
            }
            server.writes.push(args);
            server.removed = self.mode == Mode::Remove;
            server.added = if self.mode.adding() { ids.len() } else { 0 };
            return Err(Error::Transport(
                "synthetic mixed edit lost response".into(),
            ));
        }
        let step = {
            let mut server = self.server.lock().unwrap();
            let (wanted, step) = match server.writes.len() {
                0 => (["701", "702"], 1),
                1 => (["702", "701"], 2),
                _ => {
                    server.unexpected += 1;
                    return Err(Error::Response("unexpected synthetic reorder replay"));
                }
            };
            if cache
                || args
                    != json!({
                        "pid": TARGET, "op": "update", "trackIds": serde_json::to_string(&wanted)?
                    })
            {
                server.unexpected += 1;
                return Err(Error::Response("invalid synthetic reorder projection"));
            }
            server.writes.push(args);
            if step == 2 {
                // Apply remotely, then deliberately lose the response.
                server.order = wanted;
            }
            step
        };
        if step == 1 {
            self.entered.notify_one();
            self.acknowledge.notified().await;
            Ok(json!({"code":"200"}))
        } else {
            Err(Error::Transport(
                "synthetic mixed reorder lost response".into(),
            ))
        }
    }
}

fn playlist(id: u64, owner: u64, special: u8, name: &str, order: &[&str]) -> Value {
    json!({"id":id,"userId":owner,"creator":{"userId":owner,"nickname":"合成歌单用户"},
        "name":name,"privacy":0,"specialType":special,"trackCount":order.len(),"tags":[],
        "trackIds":order.iter().map(|id|json!({"id":id,"v":0})).collect::<Vec<_>>()})
}

impl Api for Fixture {
    fn local_collection_scope_revision(&self) -> Result<Option<String>> {
        Ok((self.mode == Mode::Remove).then(|| "synthetic-mixed-remove".into()))
    }

    fn mixed_playlist_add_scope_revision(&self) -> Result<Option<String>> {
        Ok(self.mode.adding().then(|| "synthetic-mixed-add".into()))
    }

    fn request_guarded_mixed_playlist_add<'a>(
        &'a self,
        write: GuardedPlaylistAdd,
        revision: &'a str,
        permission: &'a PlaylistAddPermission<'a>,
        dispatched: &'a AtomicBool,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if !self.mode.adding() || revision != "synthetic-mixed-add" {
                return Err(Error::CredentialChanged);
            }
            let (path, args) = write.into_request()?;
            permission()?;
            dispatched.store(true, Ordering::Release);
            self.request(path, args, false).await
        })
    }

    fn request_guarded_playlist_remove<'a>(
        &'a self,
        write: GuardedPlaylistRemove,
        revision: &'a str,
        permission: &'a PlaylistAddPermission<'a>,
        dispatched: &'a AtomicBool,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if self.mode != Mode::Remove || revision != "synthetic-mixed-remove" {
                return Err(Error::CredentialChanged);
            }
            let (path, args) = write.into_request()?;
            permission()?;
            dispatched.store(true, Ordering::Release);
            self.request(path, args, false).await
        })
    }

    fn request<'a>(&'a self, path: &'a str, args: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            match path {
                "/api/user/playlist" => {
                    let server = self.server.lock().unwrap();
                    Ok(json!({"code":200,"more":false,"playlist":[
                        playlist(10,2,5,"我喜欢的音乐",&[]),
                        playlist(11,2,0,"合成自建混排歌单",server.visible_order()),
                        if self.mode.adding() { playlist(12,2,0,"合成收藏来源",&["703","704"]) }
                        else { playlist(12,3,0,"合成收藏歌单",&[]) }
                    ]}))
                }
                "/api/user/playlist/v2" if self.mode.adding() => {
                    let server = self.server.lock().unwrap();
                    Ok(json!({"code":200,"playlist":[
                        playlist(11,2,0,"合成自建混排歌单",server.visible_order()),
                        playlist(12,2,0,"合成收藏来源",&["703","704"])
                    ],"more":false}))
                }
                "/api/v6/playlist/detail" if self.mode.adding() && args["id"] == "12" => Ok(
                    json!({"code":200,"playlist":playlist(12,2,0,"合成收藏来源",&["703","704"])}),
                ),
                "/api/v6/playlist/detail"
                    if !cache && args == json!({"id":TARGET,"n":100000,"s":0}) =>
                {
                    let mut server = self.server.lock().unwrap();
                    server.reads += 1;
                    Ok(
                        json!({"code":200,"playlist":playlist(11,2,0,"合成自建混排歌单",server.visible_order())}),
                    )
                }
                REORDER => self.update(args, cache).await,
                "/api/v3/song/detail" if self.mode.adding() => {
                    let ids = serde_json::from_str::<Vec<Value>>(
                        args["c"]
                            .as_str()
                            .ok_or(Error::Response("missing synthetic c field"))?,
                    )?
                    .iter()
                    .map(|row| identifier(&row["id"]))
                    .collect::<Result<Vec<_>>>()?;
                    if !ids
                        .iter()
                        .all(|id| matches!(id.as_str(), "701" | "702" | "703" | "704"))
                    {
                        return Err(Error::Response("unknown synthetic catalog song"));
                    }
                    Ok(json!({"code":200,"songs":ids.iter().map(|id|
                        json!({"id":id,"name":format!("合成线上曲目 {id}"),"ar":[{"name":"Cumulus"}],
                            "al":{"name":"合成专辑"},"dt":12000,"fee":0})).collect::<Vec<_>>(),
                        "privileges":ids.iter().map(|id|json!({"id":id,"subp":1})).collect::<Vec<_>>() }))
                }
                path if path.starts_with("/api/song/enhance/player/") => {
                    let valid = args["ids"]
                        .as_str()
                        .and_then(|packed| serde_json::from_str::<Vec<Value>>(packed).ok())
                        .and_then(|ids| match ids.as_slice() {
                            [id] => identifier(id).ok(),
                            _ => None,
                        })
                        .filter(|id| matches!(id.as_str(), "701" | "702"));
                    let mut server = self.server.lock().unwrap();
                    if path == SOURCE
                        && !cache
                        && let Some(id) = valid
                    {
                        server.audio_checks.push(id);
                        Ok(json!({"code":200,"data":[]}))
                    } else {
                        server.unexpected += 1;
                        Err(Error::Response("synthetic reorder refuses unbound audio"))
                    }
                }
                _ => self.base.request(path, args, cache).await,
            }
        })
    }
}
