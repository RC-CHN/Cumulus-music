//! Sealed native search group download: actual tasks and FLAC publication.
use cumulus_core::{
    Error, Result,
    account::AccountSession,
    client::{MusicClient, dto::DownloadCommand},
    download::{
        directory::DownloadDirectory,
        service::{
            DownloadDestination, DownloadOptions, DownloadRunOptions, DownloadService,
            DownloadStatus,
        },
        transfer::{MediaBackend, MediaBody, MediaReply, MediaRequest},
    },
    membership::Quality,
    protocol::api::ApiFuture,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
};

const AUDIO: &[u8] = include_bytes!("../../../core/tests/fixtures/stereo-pcm-source.flac");
const ROOT: &str = "https://m1.music.126.net/cumulus-group-download-";
const LIST: &str = "/api/user/playlist/v2";
const STAR_LIST: &str = "/api/user/playlist";
const DETAIL: &str = "/api/v6/playlist/detail";
const WRITE: &str = "/api/v1/playlist/manipulate/tracks";
const MEMBERSHIP: &str = "/api/song/like/get";
pub const REVISION: &str = "synthetic-search-group-download";
const MD5: &str = "4e25192f88135eb81a995c6aa5fa7848";
static TRACE: Mutex<Vec<Value>> = Mutex::new(Vec::new());
const PARTIAL: u64 = 2048;

pub fn mixed() -> bool {
    destination() && std::env::var_os("CUMULUS_SEARCH_GROUP_DOWNLOAD_MIXED_PROOF").is_some()
}

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_SEARCH_GROUP_DOWNLOAD_PROOF").is_some()
}

pub fn destination() -> bool {
    enabled() && std::env::var_os("CUMULUS_SEARCH_GROUP_DOWNLOAD_DESTINATION_PROOF").is_some()
}

pub fn liked() -> bool {
    destination() && std::env::var_os("CUMULUS_SEARCH_GROUP_DOWNLOAD_LIKED_PROOF").is_some()
}

fn song_allowed(id: &str) -> bool {
    matches!(id, "101" | "103") || destination() && id == "104"
}

fn playlist(id: u64, owner: u64, special: u64) -> Value {
    json!({"id":id,"userId":owner,"creator":{"userId":owner,"nickname":"合成账号"},
        "name":"合成下载目的地","specialType":special,"privacy":10,"tags":[]})
}

pub fn write(path: &str, args: &Value) -> Result<Value> {
    let mut trace = TRACE.lock().unwrap();
    let target = if liked() { "83" } else { "81" };
    if !destination()
        || path != WRITE
        || *args != json!({"op":"add","pid":target,"trackIds":r#"["103","101"]"#})
        || trace.iter().any(|row| row["kind"] == "playlist_add")
    {
        trace.push(json!({"kind":"unexpected_write","path":path}));
        return Err(Error::InvalidInput("unexpected group proof playlist add"));
    }
    trace.push(json!({"kind":"playlist_add","args":args}));
    Err(Error::Transport("sealed playlist response lost".into()))
}

pub fn liked_read(read: cumulus_core::protocol::api::SystemLikedRead) -> Result<Value> {
    let (path, args) = read.into_request();
    let mut trace = TRACE.lock().unwrap();
    let wrote = trace.iter().any(|row| row["kind"] == "playlist_add");
    Ok(match path {
        STAR_LIST if args == json!({"uid":"2","offset":0,"limit":1000}) => {
            trace.push(json!({"kind":"liked_list"}));
            json!({"code":200,"playlist":[playlist(81,2,0),playlist(83,2,5)],"more":false})
        }
        DETAIL if args == json!({"id":"83","n":100000,"s":0}) => {
            let mut row = playlist(83, 2, 5);
            row["trackIds"] = if wrote {
                json!([{"id":103},{"id":101}])
            } else {
                json!([{"id":103}])
            };
            trace.push(json!({"kind":"liked_detail","tracks":row["trackIds"]}));
            json!({"code":200,"playlist":row})
        }
        MEMBERSHIP if args == json!({"uid":"2"}) => {
            let previous_after_write = trace
                .iter()
                .any(|row| row["kind"] == "liked_membership" && row["afterWrite"] == true);
            let ids = if wrote && previous_after_write {
                json!([103, 101])
            } else {
                json!([103])
            };
            trace.push(json!({"kind":"liked_membership","ids":ids,"afterWrite":wrote}));
            json!({"code":200,"ids":ids})
        }
        _ => return Err(Error::InvalidInput("unexpected system liked proof read")),
    })
}

pub fn response(path: &str, args: &Value, cached: bool) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    Some(match path {
        "/api/song/like/get" => Ok(json!({"code":200,"ids":[if destination() {103} else {102}]})),
        LIST if destination() && args["uid"] == "2" => {
            TRACE
                .lock()
                .unwrap()
                .push(json!({"kind":"playlist_list","args":args,"cached":cached}));
            Ok(
                json!({"code":200,"playlist":[playlist(81,2,0),playlist(82,9,0),playlist(83,2,5)],"more":false}),
            )
        }
        DETAIL if destination() && (args["id"] == "81" || args["id"] == 81) => {
            let mut trace = TRACE.lock().unwrap();
            let count = trace
                .iter()
                .filter(|row| row["kind"] == "playlist_detail")
                .count();
            let mut row = playlist(81, 2, 0);
            row["trackIds"] = if count >= 2 {
                json!([{"id":103},{"id":101}])
            } else {
                json!([])
            };
            trace.push(json!({"kind":"playlist_detail","id":"81","tracks":row["trackIds"],"cached":cached}));
            Ok(json!({"code":200,"playlist":row}))
        }
        "/api/song/like" | WRITE | "/api/playlist/manipulate/tracks" | "/api/playlist/create" => {
            TRACE
                .lock()
                .unwrap()
                .push(json!({"kind":"unexpected_write","path":path}));
            Err(Error::InvalidInput(
                "search group proof cannot write account resources",
            ))
        }
        "/api/song/enhance/download/url/v1" => {
            let id = args["id"].as_str().unwrap_or_default();
            TRACE
                .lock()
                .unwrap()
                .push(json!({"kind":"authorize","args":args,"cached":cached}));
            let level = if mixed() && id == "104" {
                "lossless"
            } else {
                "exhigh"
            };
            if cached
                || !song_allowed(id)
                || *args != json!({"id":id,"level":level,"immerseType":"c51"})
            {
                Err(Error::InvalidInput("unexpected group proof authorization"))
            } else {
                Ok(
                    json!({"code":200,"data":{"code":200,"id":id,"url":format!("{ROOT}{id}.flac"),
                    "size":AUDIO.len(),"type":"flac","level":"lossless","md5":MD5}}),
                )
            }
        }
        _ => return None,
    })
}
pub fn report() -> Vec<Value> {
    TRACE.lock().unwrap().clone()
}

struct Body(&'static [u8]);
impl MediaBody for Body {
    fn read<'a>(&'a mut self, output: &'a mut [u8]) -> ApiFuture<'a, usize> {
        Box::pin(async move {
            let n = self.0.len().min(output.len());
            output[..n].copy_from_slice(&self.0[..n]);
            self.0 = &self.0[n..];
            Ok(n)
        })
    }
}
pub struct Media;
impl MediaBackend for Media {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            let id = request
                .url
                .as_str()
                .strip_prefix(ROOT)
                .and_then(|url| url.strip_suffix(".flac"))
                .filter(|id| song_allowed(id))
                .ok_or(Error::InvalidInput("unexpected group proof media"))?;
            if (request.offset != 0 && (!mixed() || id != "104" || request.offset != PARTIAL))
                || request.if_range.is_some()
            {
                return Err(Error::InvalidInput("unexpected group proof offset"));
            }
            let start = request.offset as usize;
            let body = &AUDIO[start..];
            let mut headers = BTreeMap::from([("content-length".into(), body.len().to_string())]);
            if start != 0 {
                headers.insert(
                    "content-range".into(),
                    format!("bytes {start}-{}/{}", AUDIO.len() - 1, AUDIO.len()),
                );
            }
            TRACE
                .lock()
                .unwrap()
                .push(json!({"kind":"media","id":id,"offset":request.offset}));
            Ok(MediaReply {
                status: if start == 0 { 200 } else { 206 },
                headers,
                body: Box::new(Body(body)),
            })
        })
    }
}

pub async fn configure(
    client: &MusicClient,
    root: &Path,
    session: Arc<AccountSession>,
) -> Result<Value> {
    if !enabled() {
        return Err(Error::InvalidInput("no group proof"));
    }
    let epoch = client.account()?.epoch;
    let output = root.join("group-output");
    std::fs::create_dir(&output)?;
    if mixed() {
        DownloadDirectory::new(session.clone())?.configure(&output, 0, 0)?;
    } else {
        client
            .download_command(
                &epoch,
                DownloadCommand::Configure {
                    root: output
                        .to_str()
                        .ok_or(Error::InvalidInput("invalid group proof directory"))?
                        .into(),
                    path_type: 0,
                    file_type: 0,
                },
            )
            .await?;
    }
    let prefs = client.download_quality_preferences(&epoch)?;
    let mut options = prefs.options;
    options.quality = cumulus_core::membership::Quality::ExHigh;
    options.cover = false;
    client
        .download_command(
            &epoch,
            DownloadCommand::Preferences {
                revision: prefs.revision,
                options,
            },
        )
        .await?;
    if !mixed() {
        return Ok(Value::Null);
    }
    let service = DownloadService::with_backend(session, Arc::new(Media), 512 * 1024 * 1024)?;
    let task = service.create(
        &json!("104"),
        DownloadDestination::Path {
            path: root.join("group-previous/original-104.flac"),
        },
        DownloadOptions {
            quality: Quality::Lossless,
            cover: false,
            tags: false,
            ..Default::default()
        },
    )?;
    let old = service
        .run(
            &task.id,
            DownloadRunOptions {
                byte_budget: Some(PARTIAL),
                ..Default::default()
            },
        )
        .await?;
    if old.id != task.id || old.status != DownloadStatus::Paused || old.bytes != PARTIAL {
        return Err(Error::State("fixture did not pause real partial transfer"));
    }
    Ok(json!({"id":old.id,"createdMs":old.created_ms,"bytes":old.bytes}))
}
