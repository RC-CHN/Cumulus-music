//! Same local WAV and account as the mixed membership proof, but a clean
//! playlist and a closed two-write server for reorder.
use cumulus_core::{
    Error, Result,
    protocol::api::{Api, ApiFuture, identifier},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{path::Path, sync::Mutex, time::Duration};
use tokio::sync::Notify;

const TARGET: &str = "11";
const REORDER: &str = "/api/v1/playlist/manipulate/tracks";
const SOURCE: &str = "/api/song/enhance/player/url/v1";
const JOURNAL: &str = "playlist-local-reorders-v1";

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

struct Server {
    order: [&'static str; 2],
    writes: Vec<Value>,
    reads: usize,
    unexpected: usize,
    audio_checks: Vec<String>,
}

pub struct Fixture {
    base: super::mixed_playlist_fixture::Fixture,
    server: Mutex<Server>,
    acknowledge: Notify,
    entered: Notify,
}

impl Fixture {
    pub fn new(root: &Path) -> std::io::Result<Self> {
        Ok(Self {
            base: super::mixed_playlist_fixture::Fixture::new(root)?,
            server: Mutex::new(Server {
                order: ["701", "702"],
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
        let journal = store.get("2", JOURNAL, TARGET)?.unwrap_or(Value::Null);
        let server = self.server.lock().unwrap();
        Ok(json!({
            "membership":self.base.receipt(store)?,
            "writes":server.writes,
            "detailReads":server.reads,
            "onlineOrder":server.order,
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
        "name":name,"privacy":0,"specialType":special,"trackCount":order.len(),
        "trackIds":order.iter().map(|id|json!({"id":id,"v":0})).collect::<Vec<_>>()})
}

impl Api for Fixture {
    fn request<'a>(&'a self, path: &'a str, args: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            match path {
                "/api/user/playlist" => {
                    let order = self.server.lock().unwrap().order;
                    Ok(json!({"code":200,"more":false,"playlist":[
                        playlist(10,2,5,"我喜欢的音乐",&[]),
                        playlist(11,2,0,"合成自建混排歌单",&order),
                        playlist(12,3,0,"合成收藏歌单",&[])
                    ]}))
                }
                "/api/v6/playlist/detail"
                    if !cache && args == json!({"id":TARGET,"n":100000,"s":0}) =>
                {
                    let mut server = self.server.lock().unwrap();
                    server.reads += 1;
                    Ok(
                        json!({"code":200,"playlist":playlist(11,2,0,"合成自建混排歌单",&server.order)}),
                    )
                }
                REORDER => self.update(args, cache).await,
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
