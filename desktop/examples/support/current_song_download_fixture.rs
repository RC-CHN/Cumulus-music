//! Sealed PlayerBar purchase evidence, guarded playlist readback and FLAC transfer.
use cumulus_core::{
    Error, Result,
    download::transfer::{MediaBackend, MediaBody, MediaReply, MediaRequest},
    protocol::api::ApiFuture,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

pub const REVISION: &str = "synthetic-current-player-download";
const URL: &str = "https://m1.music.126.net/cumulus-current-purchased-2.flac";
const AUDIO: &[u8] = include_bytes!("../../../core/tests/fixtures/stereo-pcm-source.flac");
const MD5: &str = "4e25192f88135eb81a995c6aa5fa7848";
const ADD: &str = "/api/v1/playlist/manipulate/tracks";
const AUTH: &str = "/api/song/enhance/download/url/v1";
static TRACE: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static READBACK: AtomicBool = AtomicBool::new(false);

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_CURRENT_SONG_DOWNLOAD_PROOF").is_some()
}

pub fn playlist_mode() -> bool {
    std::env::var("CUMULUS_CURRENT_SONG_DOWNLOAD_PROOF").as_deref() == Ok("playlist")
}

pub fn valid_mode() -> bool {
    matches!(
        std::env::var("CUMULUS_CURRENT_SONG_DOWNLOAD_PROOF").as_deref(),
        Ok("liked" | "playlist" | "directory")
    )
}

pub fn witnessed() -> bool {
    enabled() || super::download_page_fixture::menu_comments_enabled()
}

pub fn annotate(ids: &[String], response: &mut Value) {
    let right = |id: &str| match (enabled(), id) {
        (true, "2") => (4, 2),
        (true, "3") => (1, 1),
        _ => (0, 0),
    };
    response["privileges"] = json!(
        ids.iter()
            .map(|id| {
                let (fee, paid) = right(id);
                json!({"id":id,"st":0,"fee":fee,"payed":paid,"dl":128000,
                    "maxbr":999000,"flag":1 << 12,"cs":0,"subp":0})
            })
            .collect::<Vec<_>>()
    );
    if enabled() {
        let mut trace = TRACE.lock().unwrap();
        trace.push(json!({"kind":"detail","ids":ids}));
        assert!(ids.iter().all(|id| matches!(id.as_str(), "2" | "3")));
    }
}

fn destination(id: u32, owner: u32, present: bool) -> Value {
    json!({"id":id,"name":if id == 81 {"合成当前歌曲下载歌单"} else {"合成外部歌单"},
        "userId":owner,"creator":{"userId":owner,"nickname":"合成账号"},
        "specialType":0,"privacy":10,"tags":[],
        "trackCount":u32::from(present),"trackIds":if present {json!([{"id":2}])} else {json!([])}})
}

pub fn read(path: &str, args: &Value, cached: bool) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    let mut trace = TRACE.lock().unwrap();
    let invalid = || Error::InvalidInput("unexpected current-song download proof request");
    Some(match path {
        "/api/song/like/get" => {
            if *args != json!({"uid":"2"}) || cached {
                return Some(Err(invalid()));
            }
            Ok(json!({"code":200,"ids":if playlist_mode() { vec![] } else { vec![2] }}))
        }
        "/api/user/playlist/v2" => {
            if !playlist_mode()
                || cached
                || *args != json!({"uid":"2","offset":0,"limit":10,"sortType":"default"})
            {
                return Some(Err(invalid()));
            }
            trace.push(json!({"kind":"destinations","args":args}));
            Ok(
                json!({"code":200,"more":false,"playlist":[destination(81,2,false),destination(83,3,false)]}),
            )
        }
        "/api/v6/playlist/detail" if args["id"] == "81" => {
            if !playlist_mode() || cached || *args != json!({"id":"81","n":100000,"s":0}) {
                return Some(Err(invalid()));
            }
            let wrote = trace.iter().any(|row| row["kind"] == "playlist_add");
            // The service's immediate readback still cannot see the lost add.
            // Only explicit status reconciliation sees the same Song in that playlist.
            let present = wrote && READBACK.load(Ordering::Acquire);
            trace.push(
                json!({"kind":"playlist_detail","args":args,"afterAdd":wrote,"present":present}),
            );
            Ok(json!({"code":200,"playlist":destination(81,2,present)}))
        }
        AUTH => {
            if cached || *args != json!({"id":"2","level":"lossless","immerseType":"c51"}) {
                trace.push(json!({"kind":"unexpected_auth","args":args}));
                return Some(Err(invalid()));
            }
            trace.push(json!({"kind":"authorize","args":args,"cached":cached}));
            Ok(
                json!({"code":200,"data":{"code":200,"id":"2","url":URL,"size":AUDIO.len(),
                "type":"flac","level":"lossless","md5":MD5}}),
            )
        }
        ADD | "/api/playlist/create" => {
            trace.push(json!({"kind":"unguarded_write","path":path}));
            Err(invalid())
        }
        _ => return None,
    })
}

pub fn write(path: &str, args: &Value) -> Result<Value> {
    let mut trace = TRACE.lock().unwrap();
    if !playlist_mode()
        || path != ADD
        || *args != json!({"op":"add","pid":"81","trackIds":r#"["2"]"#})
        || trace.iter().any(|row| row["kind"] == "playlist_add")
    {
        trace.push(json!({"kind":"unexpected_write","path":path,"args":args}));
        return Err(Error::InvalidInput(
            "unexpected current-song playlist write",
        ));
    }
    trace.push(json!({"kind":"playlist_add","args":args}));
    Err(Error::Transport("synthetic lost playlist response".into()))
}

pub fn expose_readback() {
    assert!(playlist_mode());
    assert!(
        TRACE
            .lock()
            .unwrap()
            .iter()
            .any(|row| row["kind"] == "playlist_add")
    );
    READBACK.store(true, Ordering::Release);
}

pub fn report() -> Value {
    json!(*TRACE.lock().unwrap())
}

struct Body(&'static [u8]);
impl MediaBody for Body {
    fn read<'a>(&'a mut self, output: &'a mut [u8]) -> ApiFuture<'a, usize> {
        Box::pin(async move {
            let count = self.0.len().min(output.len());
            output[..count].copy_from_slice(&self.0[..count]);
            self.0 = &self.0[count..];
            Ok(count)
        })
    }
}

pub struct Media;
impl MediaBackend for Media {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            if request.url.as_str() != URL || request.offset != 0 || request.if_range.is_some() {
                return Err(Error::InvalidInput(
                    "unexpected current-song download media",
                ));
            }
            TRACE
                .lock()
                .unwrap()
                .push(json!({"kind":"media","bytes":AUDIO.len(),"offset":0}));
            Ok(MediaReply {
                status: 200,
                headers: BTreeMap::from([("content-length".into(), AUDIO.len().to_string())]),
                body: Box::new(Body(AUDIO)),
            })
        })
    }
}
