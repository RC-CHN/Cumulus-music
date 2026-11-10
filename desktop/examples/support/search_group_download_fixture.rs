//! Sealed native search group download: actual tasks and FLAC publication.
use cumulus_core::{
    Error, Result,
    client::{MusicClient, dto::DownloadCommand},
    download::transfer::{MediaBackend, MediaBody, MediaReply, MediaRequest},
    protocol::api::ApiFuture,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, sync::Mutex};

const AUDIO: &[u8] = include_bytes!("../../../core/tests/fixtures/stereo-pcm-source.flac");
const ROOT: &str = "https://m1.music.126.net/cumulus-group-download-";
const LIST: &str = "/api/user/playlist/v2";
const DETAIL: &str = "/api/v6/playlist/detail";
const WRITE: &str = "/api/v1/playlist/manipulate/tracks";
const MD5: &str = "4e25192f88135eb81a995c6aa5fa7848";
static TRACE: Mutex<Vec<Value>> = Mutex::new(Vec::new());

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_SEARCH_GROUP_DOWNLOAD_PROOF").is_some()
}

pub fn destination() -> bool {
    enabled() && std::env::var_os("CUMULUS_SEARCH_GROUP_DOWNLOAD_DESTINATION_PROOF").is_some()
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
    if !destination()
        || path != WRITE
        || *args != json!({"op":"add","pid":"81","trackIds":r#"["103","101"]"#})
        || trace.iter().any(|row| row["kind"] == "playlist_add")
    {
        trace.push(json!({"kind":"unexpected_write","path":path}));
        return Err(Error::InvalidInput("unexpected group proof playlist add"));
    }
    trace.push(json!({"kind":"playlist_add","args":args}));
    Err(Error::Transport("sealed playlist response lost".into()))
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
            if cached
                || !song_allowed(id)
                || *args != json!({"id":id,"level":"exhigh","immerseType":"c51"})
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
            if request.offset != 0 || request.if_range.is_some() {
                return Err(Error::InvalidInput("unexpected group proof offset"));
            }
            TRACE
                .lock()
                .unwrap()
                .push(json!({"kind":"media","id":id,"offset":request.offset}));
            Ok(MediaReply {
                status: 200,
                headers: BTreeMap::from([("content-length".into(), AUDIO.len().to_string())]),
                body: Box::new(Body(AUDIO)),
            })
        })
    }
}

pub async fn configure(client: &MusicClient, root: &Path) -> Result<()> {
    if !enabled() {
        return Err(Error::InvalidInput("no group proof"));
    }
    let epoch = client.account()?.epoch;
    let output = root.join("group-output");
    std::fs::create_dir(&output)?;
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
    Ok(())
}
