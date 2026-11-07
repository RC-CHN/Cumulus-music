//! Closed synthetic wire for the playback episode's included songs and heart.
use serde_json::{Value, json};
use std::sync::Mutex;

pub const REVISION: &str = "voice-songs-fixture-revision";
const SONGS: &str = "/api/v1/dj/program/tracks";
const LIKED: &str = "/api/song/like/get";
const LIKE: &str = "/api/song/like";
const MEDIA: &str = "/api/song/enhance/player/url/v1";
const FIXTURE_URL: &str = "https://m1.music.126.net/cumulus-relay-fixture.wav";

pub struct Fixture(Mutex<State>, bool, bool, bool, bool, bool);

#[derive(Default)]
struct State {
    song_reads: Vec<Value>,
    media_reads: Vec<Value>,
    like_reads: Vec<Value>,
    likes: Vec<Value>,
    liked: bool,
}

fn row(id: u64, name: &str, blocked: bool) -> Value {
    json!({"id":id,"name":name,"dt":30000,"ar":[{"id":57,"name":"合成歌曲作者"}],
        "al":{"id":55,"name":"合成包含歌曲专辑"},
        "privilege":{"id":id,"st":if blocked {-200} else {0},"pl":if blocked {0} else {128000}},
        "authorization":"SYNTHETIC_INCLUDED_PRIVATE"})
}

impl Fixture {
    pub fn new(case: &str) -> Self {
        Self(
            Mutex::default(),
            case == "dense",
            matches!(case, "download" | "purchased_download"),
            case == "collect",
            case == "purchased_download",
            case == "group_download",
        )
    }

    pub fn songs(&self, path: &str, payload: Value) -> Value {
        assert_eq!(path, SONGS);
        let rows = match payload["id"].as_str() {
            Some("601") if self.1 => (2201..=2300)
                .map(|id| row(id, &format!("合成包含歌曲 {id}"), false))
                .collect(),
            Some("601") if self.5 => {
                let mut rows = [
                    row(1710, "合成包含歌曲 1710", false),
                    row(1712, "合成包含歌曲 1712", false),
                    row(1790, "合成已喜欢下架歌曲", true),
                    row(1791, "合成已购歌曲", false),
                ];
                for song in &mut rows {
                    song["privilege"]["fee"] = json!(0);
                    song["privilege"]["dl"] = json!(320000);
                }
                rows[3]["privilege"]["fee"] = json!(4);
                rows[3]["privilege"]["payed"] = json!(2);
                rows.into()
            }
            Some("601") => vec![
                {
                    let mut song = row(1710, "合成包含歌曲 1710", false);
                    if self.2 {
                        song["privilege"]["fee"] = json!(0);
                        song["privilege"]["dl"] = json!(320000);
                    }
                    if self.4 {
                        song["privilege"]["fee"] = json!(4);
                        song["privilege"]["payed"] = json!(2);
                        song["privilege"]["maxbr"] = json!(999000);
                        song["privilege"]["flag"] = json!((1 << 12) | (1 << 16));
                    }
                    song
                },
                json!({"id":1799,"name":"不应显示的无专辑歌曲"}),
                row(1601, "与播客音轨同编号的普通歌", false),
                {
                    let mut song = row(1790, "合成版权受限歌曲", true);
                    if self.3 {
                        song["privilege"]["subp"] = json!(-1);
                    }
                    song
                },
                {
                    let mut context_only = row(1791, "合成菜单版权受限歌曲", true);
                    if !self.3 {
                        context_only["privilege"]["pl"] = json!(128000);
                    }
                    context_only
                },
            ],
            Some("602") => vec![
                row(1811, "合成新节目歌曲 1811", false),
                json!({"id":1899,"name":"新节目无专辑歌曲"}),
                row(1810, "合成新节目歌曲 1810", false),
            ],
            _ => panic!("unexpected synthetic episode songs identity"),
        };
        assert_eq!(payload, json!({"id":payload["id"]}));
        self.0
            .lock()
            .unwrap()
            .song_reads
            .push(json!([path, payload]));
        json!({"code":200,"tracks":rows})
    }

    pub fn read(&self, path: &str, body: &Value, cache: bool) -> Option<Value> {
        match path {
            LIKED => {
                assert!(!cache);
                assert_eq!(*body, json!({"uid":"2"}));
                let mut state = self.0.lock().unwrap();
                let ids = if state.liked { vec![1710] } else { vec![] };
                state.like_reads.push(json!([path, body, ids]));
                Some(json!({"code":200,"ids":ids}))
            }
            MEDIA => {
                let ids: Vec<Value> =
                    serde_json::from_str(body["ids"].as_str().expect("fixture player IDs"))
                        .unwrap();
                assert_eq!(ids.len(), 1);
                let id = ids[0]
                    .as_u64()
                    .or_else(|| ids[0].as_str()?.parse().ok())
                    .unwrap();
                assert!(
                    matches!(id, 1601 | 1602 | 1710 | 1810 | 1811),
                    "unexpected media request"
                );
                assert_eq!(body["level"], "standard");
                self.0
                    .lock()
                    .unwrap()
                    .media_reads
                    .push(json!([path, body, id.to_string()]));
                Some(json!({"code":200,"data":[{"id":ids[0],"url":FIXTURE_URL,
                    "code":200,"md5":"fixture","level":"standard","expi":120}]}))
            }
            LIKE => panic!("test forbids unguarded synthetic song writes"),
            _ => None,
        }
    }

    pub fn like(&self, path: &str, body: Value) -> Value {
        assert_eq!(path, LIKE);
        let liked = body["like"].as_bool().expect("boolean like action");
        assert_eq!(body, json!({"trackId":"1710","userid":"2","like":liked}));
        let mut state = self.0.lock().unwrap();
        assert_ne!(state.liked, liked, "no repeated mutation");
        state.liked = liked;
        state.likes.push(json!([path, body]));
        json!({"code":200})
    }

    pub fn snapshot(&self) -> Value {
        let state = self.0.lock().unwrap();
        json!({"songReads":state.song_reads,"mediaReads":state.media_reads,
            "likeReads":state.like_reads,"likes":state.likes,"liked":state.liked})
    }
}
