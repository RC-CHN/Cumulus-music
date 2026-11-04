use cumulus_core::{
    Error, Result,
    account::AccountSession,
    download::{
        offline::{OfflineLibrary, PublishedDownload},
        service::{
            DownloadDestination, DownloadOptions, DownloadPhase, DownloadService, DownloadStatus,
        },
    },
    media::integrity::inspect_file,
    protocol::api::{Api, ApiFuture},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;

const LIST: &str = "/api/sub/artist/new/works/song-mv/list/v2";
const LATEST: &str = "/api/sub/artist/new/works/song/playall";
const SWITCH: &str = "/api/sub/artist/publish/switch";
const FIRST_TIME: u64 = 1_700_000_000_000;
const NEXT_TIME: u64 = FIRST_TIME - 9 * 86_400_000;

pub struct Library {
    base: Arc<dyn Api>,
    calls: Mutex<Vec<(String, Value)>>,
    first_release: Notify,
    offline_head: bool,
}
impl Library {
    pub fn new(base: Arc<dyn Api>, offline_head: bool) -> Self {
        Self {
            base,
            calls: Mutex::default(),
            first_release: Notify::new(),
            offline_head,
        }
    }
    pub fn allow_first_page(&self) {
        self.first_release.notify_one();
    }
    pub fn seed_offline_head(
        session: &Arc<AccountSession>,
        store: &StateStore,
        sample: &Path,
        target: &Path,
    ) -> Result<()> {
        let token = session.token()?;
        let mut task = DownloadService::new(session.clone())?.create(
            &json!(1),
            DownloadDestination::Path {
                path: target.to_owned(),
            },
            DownloadOptions::default(),
        )?;
        std::fs::copy(sample, target)?;
        let song = song(1, "合成最新播放", 0);
        let resource = json!({"id":1,"level":"standard"});
        let integrity = inspect_file(target)?;
        let record = OfflineLibrary::new(session.clone())?.register(
            &token,
            &PublishedDownload {
                task_id: task.id.clone(),
                song_id: task.song_id.clone(),
                requested_quality: task.options.quality,
                target: target.to_owned(),
                song: song.clone(),
                resource: resource.clone(),
                encrypted: false,
                integrity: integrity.clone(),
            },
        )?;
        task.status = DownloadStatus::Complete;
        task.phase = DownloadPhase::Complete;
        task.target = Some(record.path);
        task.output_integrity = Some(integrity);
        task.song = Some(song);
        task.resource = Some(resource);
        let owner = token.uid.ok_or(Error::State("missing fixture account"))?;
        store.put(
            &owner,
            "download_tasks_v1",
            &task.id,
            &serde_json::to_value(&task)?,
        )?;
        Ok(())
    }

    pub fn report(&self) -> Value {
        let calls = self.calls.lock().unwrap();
        let first: Vec<_> = calls
            .iter()
            .filter(|(path, value)| path == LIST && value["firstRequest"] == true)
            .collect();
        json!({
            "firstReads":first.len(),
            "nextCursors":calls.iter().filter(|(path, value)| path == LIST && value["firstRequest"] == false)
                .map(|(_, value)| &value["startTimestamp"]).collect::<Vec<_>>(),
            "playAllReads":calls.iter().filter(|(path, _)| path == LATEST).count(),
            "songDetailReads":calls.iter().filter(|(path, _)| path == "/api/v3/song/detail").count(),
            "streamReads":calls.iter().filter(|(path, _)| path == "/api/song/enhance/player/url/v1")
                .map(|(_, value)| &value["ids"]).collect::<Vec<_>>(),
            "expectedNextCursor": NEXT_TIME,
            "notificationRequests": calls.iter().filter(|(path, _)| path == SWITCH)
                .map(|(_, value)| value).collect::<Vec<_>>(),
        })
    }
}

fn song(id: u64, name: &str, mv: u64) -> Value {
    json!({"id":id,"name":name,"ar":[{"id":12,"name":"合成新歌歌手"}],
        "al":{"id":7,"name":"合成新专辑"},"dt":25000,"mv":mv})
}
fn release(index: u64, time: u64) -> Value {
    let album = index == 1;
    json!({
        "sourceType":1,"info":{
            "blockType":if album {"album"} else {"song"},
            "publishTime":time,
            "albumId":if album {Some(7)} else {None},
            "albumSongCount":if album {Some(2)} else {None},
            "blockTitle":{"artistId":if album {13} else {12},"artistName":format!("合成新歌歌手 {}", index + 1),
                "resourceId":index+1,"resourceName":if album {"合成新专辑"} else {"合成新单曲"}},
            "songLists":if album {vec![song(2,"合成专辑曲目",29),song(3,"合成专辑第二首",0)]}
                else if index==0 {vec![song(1,"合成新单曲",0),song(4,"合成伴奏",0)]}
                else {vec![song(index+10,if index==10 {"合成较早新歌"} else {"合成后续新歌"},0)]}
        }
    })
}

impl Api for Library {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if ![LIST, LATEST, SWITCH].contains(&path) {
                if ["/api/v3/song/detail", "/api/song/enhance/player/url/v1"].contains(&path) {
                    self.calls
                        .lock()
                        .unwrap()
                        .push((path.into(), payload.clone()));
                }
                return self.base.request(path, payload, cache).await;
            }
            let first = payload["firstRequest"].as_bool();
            let attempt = {
                let mut calls = self.calls.lock().unwrap();
                calls.push((path.into(), payload.clone()));
                calls
                    .iter()
                    .filter(|(p, v)| p == path && v["firstRequest"].as_bool() == first)
                    .count()
            };
            if path == SWITCH {
                assert!(!cache && std::env::var_os("CUMULUS_FOLLOW_NOTICE_PROOF").is_some());
                return if payload == json!({"artistId":"13","switchStatus":0}) {
                    Err(Error::Transport("synthetic switch reply lost".into()))
                } else {
                    assert_eq!(payload, json!({"artistId":"12","switchStatus":0}));
                    Ok(json!({"code":200}))
                };
            }
            if path == LATEST {
                assert!(!cache && payload == json!({}));
                return Ok(if attempt == 1 {
                    json!({"code":503})
                } else {
                    let mut denied = song(5, "无权歌曲", 0);
                    denied["privilege"] = json!({"pl":0,"flag":0});
                    let mut head = song(1, "合成最新播放", 0);
                    if self.offline_head {
                        head["privilege"] = json!({"pl":0,"flag":0});
                    }
                    json!({"code":200,"data":{"songList":[head,denied,
                        song(2,"合成下一首",0),if self.offline_head {
                            json!({"id":1,"name":"重复歌曲","privilege":{"pl":0,"flag":0}})
                        } else {song(1,"重复歌曲",0)}]}})
                });
            }
            let start = payload["startTimestamp"]
                .as_u64()
                .ok_or(Error::Response("missing fixture timestamp"))?;
            assert!(
                !cache
                    && payload
                        == json!({"startTimestamp":start,"firstRequest":first,"sourceType":1,"limit":10})
            );
            let data = match first {
                Some(true) => {
                    assert!(start > FIRST_TIME);
                    if attempt == 1 {
                        self.first_release.notified().await;
                    }
                    let mut rows: Vec<_> = (0..9)
                        .map(|index| release(index, FIRST_TIME - index * 86_400_000))
                        .collect();
                    rows.push(
                        json!({"sourceType":1,"info":{"blockType":"mv","publishTime":NEXT_TIME}}),
                    );
                    json!({"code":200,"data":{"newWorks":rows,"hasMore":true,
                        "latestVisitTime":FIRST_TIME - 129_600_000}})
                }
                Some(false) => {
                    assert_eq!(start, NEXT_TIME);
                    if attempt == 1 {
                        json!({"code":503})
                    } else {
                        json!({"code":200,"data":{"newWorks":[release(10, NEXT_TIME-86_400_000)],"hasMore":false}})
                    }
                }
                None => {
                    return Err(Error::Response(
                        "missing followed release fixture request type",
                    ));
                }
            };
            Ok(data)
        })
    }
}
