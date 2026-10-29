use cumulus_core::{
    Error,
    protocol::api::{Api, ApiFuture},
};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
#[derive(Default)]
pub struct Library {
    base: super::mini_fixture::Library,
    pub lyrics: AtomicUsize,
    pub pending: AtomicUsize,
    pub held: AtomicBool,
    pub offline: AtomicBool,
    pub resume: tokio::sync::Notify,
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
            if path != "/api/song/lyric/v1" {
                return self.base.request(path, args, cache).await;
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
            Ok(
                json!({"code":200,"lrc":{"lyric":lrc},"tlyric":{"lyric":translated},"romalrc":{"lyric":roman},"yrc":{"lyric":yrc},"ytlrc":{"lyric":translated},"yromalrc":{"lyric":roman}}),
            )
        })
    }
}
