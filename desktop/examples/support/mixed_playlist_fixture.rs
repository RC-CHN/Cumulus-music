//! Isolated API and local file for the mixed playlist WebView proof.
use cumulus_core::{
    Error,
    protocol::api::{Api, ApiFuture},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

const OPAQUE: &str = "a10c0000000000000000000000000001";
const MEDIA: &str = "合成混排本地曲目.wav";
const TARGET: &str = "11";

pub struct Fixture {
    audio: PathBuf,
    audio_bytes: Vec<u8>,
    catalog_reads: Mutex<Vec<Vec<String>>>,
    details: AtomicUsize,
    remote_mutations: AtomicUsize,
    online_audio: AtomicUsize,
}

impl Fixture {
    pub fn new(root: &Path) -> std::io::Result<Self> {
        let directory = root.join("media");
        fs::create_dir(&directory)?;
        let audio = directory.join(MEDIA);
        let audio_bytes = super::native_lyric_wave::native_stereo_wave(12);
        fs::write(&audio, &audio_bytes)?;
        Ok(Self {
            audio,
            audio_bytes,
            catalog_reads: Mutex::new(Vec::new()),
            details: AtomicUsize::new(0),
            remote_mutations: AtomicUsize::new(0),
            online_audio: AtomicUsize::new(0),
        })
    }

    pub fn receipt(&self, store: &StateStore) -> cumulus_core::Result<Value> {
        let record = store
            .get("2", "playlist-local-membership-v1", TARGET)?
            .unwrap_or(Value::Null);
        Ok(json!({
            "generation":record["generation"],
            "localMembers":record["local"].as_array().map_or(0, Vec::len),
            "localPosition":record["local"][0]["position"],
            "localId":record["local"][0]["reference"]["id"],
            "detailReads":self.details.load(Ordering::Acquire),
            "catalogReads":&*self.catalog_reads.lock().unwrap(),
            "remoteMutations":self.remote_mutations.load(Ordering::Acquire),
            "onlineAudio":self.online_audio.load(Ordering::Acquire),
            "originalUnchanged":fs::read(&self.audio).is_ok_and(|bytes| bytes == self.audio_bytes),
        }))
    }
}

fn playlist(id: u64, owner: u64, special: u8, name: &str) -> Value {
    json!({"id":id,"userId":owner,"creator":{"userId":owner,"nickname":"合成歌单用户"},
        "name":name,"privacy":0,"specialType":special,"trackCount":3,
        "trackIds":[{"id":701,"v":0},{"id":OPAQUE,"v":-1},{"id":702,"v":0}]})
}

fn target_playlist() -> Value {
    playlist(11, 2, 0, "合成自建混排歌单")
}

impl Api for Fixture {
    fn request<'a>(&'a self, path: &'a str, args: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            Ok(match path {
                "/api/w/nuser/account/get" => {
                    json!({"code":200,"profile":{"userId":2,"nickname":"合成混排账号"}})
                }
                "/api/user/playlist" => json!({"code":200,"more":false,"playlist":[
                    playlist(10,2,5,"我喜欢的音乐"),target_playlist(),playlist(12,3,0,"合成收藏歌单")
                ]}),
                "/api/v6/playlist/detail" if args["id"] == TARGET => {
                    self.details.fetch_add(1, Ordering::AcqRel);
                    json!({"code":200,"playlist":target_playlist()})
                }
                "/api/playlist/detail/dynamic" => json!({"code":200,"bookedCount":0}),
                "/api/v3/song/detail" => {
                    let rows: Vec<Value> =
                        serde_json::from_str(args["c"].as_str().ok_or(Error::Response(
                            "synthetic song detail needs exact c field",
                        ))?)?;
                    let ids = rows
                        .iter()
                        .map(|row| cumulus_core::protocol::api::identifier(&row["id"]))
                        .collect::<cumulus_core::Result<Vec<_>>>()?;
                    self.catalog_reads.lock().unwrap().push(ids.clone());
                    if ids.iter().any(|id| !matches!(id.as_str(), "701" | "702")) {
                        return Err(Error::Response(
                            "mixed playlist sent non-catalog ID to song detail",
                        ));
                    }
                    json!({"code":200,"songs":ids.iter().map(|id|
                        json!({"id":id,"name":format!("合成线上曲目 {id}"),"ar":[{"name":"Cumulus"}],
                            "al":{"name":"合成专辑"},"dt":12000,"fee":0})
                    ).collect::<Vec<_>>()})
                }
                "/api/song/like/get" => json!({"code":200,"ids":[]}),
                "/api/v3/discovery/recommend/songs" => json!({"code":200,"data":{"dailySongs":[]}}),
                "/api/rtrs/abt/front/expinfo/list" => json!({"code":200,"data":[]}),
                "/api/relay/setting/show" => json!({"code":200,"data":{"groups":[]}}),
                path if path.starts_with("/api/song/enhance/player/") => {
                    self.online_audio.fetch_add(1, Ordering::AcqRel);
                    return Err(Error::Response("synthetic proof refuses online audio"));
                }
                "/api/batch"
                | "/api/v1/playlist/manipulate/tracks"
                | "/api/song/like"
                | "/api/playlist/create"
                | "/api/playlist/delete"
                | "/api/playlist/track/insert"
                | "/api/playlist/update/name"
                | "/api/playlist/tags/update"
                | "/api/playlist/desc/update"
                | "/api/playlist/order/update" => {
                    self.remote_mutations.fetch_add(1, Ordering::AcqRel);
                    return Err(Error::Response("synthetic proof refuses remote mutation"));
                }
                _ => return Err(Error::Response("outside sealed mixed playlist fixture")),
            })
        })
    }
}
