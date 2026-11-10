use cumulus_core::{
    Error,
    protocol::api::{Api, ApiFuture},
};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
#[derive(Default)]
pub struct Library {
    base: super::mini_fixture::Library,
    pub lyrics: AtomicUsize,
    pub pending: AtomicUsize,
    pub held: AtomicBool,
    pub offline: AtomicBool,
    pub resume: tokio::sync::Notify,
    credits: bool,
    matched: bool,
    pub matched_calls: Mutex<Vec<Value>>,
    pub matched_audio_requests: AtomicUsize,
    pub omit_credits: AtomicBool,
    pub credit_calls: Mutex<Vec<Value>>,
}
impl Library {
    pub fn new(credits: bool, matched: bool) -> Self {
        Self {
            credits,
            matched,
            ..Self::default()
        }
    }
}
struct Loading<'a>(&'a AtomicUsize);
impl Drop for Loading<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
impl Api for Library {
    fn request<'a>(&'a self, path: &'a str, args: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if self.matched && path.starts_with("/api/song/enhance/player/") {
                self.matched_audio_requests.fetch_add(1, Ordering::AcqRel);
                return Err(Error::Response("synthetic CUE proof refused online audio"));
            }
            if self.matched && path == "/api/song/lyric/v1" {
                self.matched_calls.lock().unwrap().push(args.clone());
                self.lyrics.fetch_add(1, Ordering::AcqRel);
                if cache
                    || args
                        != json!({"id":super::matched_local_lyric_fixture::SONG_ID,
                    "lv":-1,"tv":-1,"rv":-1,"yv":-1})
                {
                    return Err(Error::Response(
                        "unexpected synthetic matched lyric request",
                    ));
                }
                return Ok(json!({"code":200,"qfy":true,"sfy":true,"sgc":true,
                    "lrc":{"version":17,"lyric":"[00:00]线上匹配歌词第一句\n[00:02]线上匹配歌词第二句"}}));
            }
            if path != "/api/song/lyric/v1" {
                if self.credits
                    && let Some(id) = path.strip_prefix("/api/w/v1/user/detail/")
                    && matches!(id, "12" | "13" | "72" | "73")
                {
                    self.credit_calls
                        .lock()
                        .unwrap()
                        .push(json!({"profile":id}));
                    if matches!(id, "72" | "73") && args == json!({"all":true,"userId":id}) {
                        return Ok(json!({"code":200,"profile":{
                            "userId":id,"nickname":format!("合成署名主页 {id}")
                        }}));
                    }
                    return Err(Error::Response("unexpected synthetic lyric credit profile"));
                }
                return self.base.request(path, args, cache).await;
            }
            if self.credits {
                self.credit_calls
                    .lock()
                    .unwrap()
                    .push(json!({"lyric":args}));
            }
            self.lyrics.fetch_add(1, Ordering::AcqRel);
            self.pending.fetch_add(1, Ordering::AcqRel);
            let _loading = Loading(&self.pending);
            while self.held.load(Ordering::Acquire) {
                self.resume.notified().await;
            }
            if self.offline.load(Ordering::Acquire) {
                return Err(Error::Transport("synthetic lyric offline".into()));
            }
            if self.credits && self.omit_credits.load(Ordering::Acquire) && args["id"] == "1" {
                return Ok(json!({"code":200,"transUser":null}));
            }
            let text = [
                "云间音乐陪你走过每一个安静的夜晚",
                "让旋律穿过城市的灯火",
                "把今天的故事轻轻唱给你听",
                "清晨我们沿着熟悉的小路向前",
                "音乐一直在身边",
                "Cumulus community music",
            ];
            let yrc = text
                .iter()
                .enumerate()
                .map(|(line, text)| {
                    let chars: Vec<_> = text.chars().collect();
                    let duration = 4000 / chars.len();
                    let start = line * 4000;
                    format!(
                        "[{start},4000]{}\n",
                        chars
                            .iter()
                            .enumerate()
                            .map(|(i, c)| format!("({},{duration},0){c}", start + i * duration))
                            .collect::<String>()
                    )
                })
                .collect::<String>();
            let lrc = text
                .iter()
                .enumerate()
                .map(|(i, text)| format!("[00:{:02}.00]{text}\n", i * 4))
                .collect::<String>();
            let translated = (0..6)
                .map(|i| format!("[00:{:02}.00]Music keeps you company · {}\n", i * 4, i + 1))
                .collect::<String>();
            let roman = (0..6)
                .map(|i| format!("[00:{:02}.00]yun jian yin yue · {}\n", i * 4, i + 1))
                .collect::<String>();
            let mut value = json!({"code":200,"lrc":{"lyric":lrc},"tlyric":{"lyric":translated},"romalrc":{"lyric":roman},"yrc":{"lyric":yrc},"ytlrc":{"lyric":translated},"yromalrc":{"lyric":roman}});
            if self.credits {
                value["lrc"]["version"] = json!(7);
                if args["id"] == "1" {
                    value["lyricUser"] = json!({"id":12,"userid":72,"nickname":"合成 <歌词作者>","url":"https://invalid.example/never-project"});
                    value["transUser"] = json!({"id":13,"userid":"73","nickname":"合成翻译作者","private":"never-project"});
                } else if args["id"] == "2" {
                    value["lyricUser"] =
                        json!({"id":72,"userid":"../73","nickname":"合成无链接作者"});
                }
            }
            Ok(value)
        })
    }
}
