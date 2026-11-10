//! Closed media and rights responses for the current-episode desktop proof.
use cumulus_core::{
    Error, Result,
    download::transfer::{MediaBackend, MediaBody, MediaReply, MediaRequest},
    protocol::api::ApiFuture,
};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const AUDIO: &[u8] = include_bytes!("../../../core/tests/fixtures/tag_audio/seed.mp3");
const MEDIA: &str = "https://m1.music.126.net/cumulus-voice-download-fixture.mp3";
static RIGHTS: AtomicUsize = AtomicUsize::new(0);
static OPTIONS: AtomicUsize = AtomicUsize::new(0);
static URLS: AtomicUsize = AtomicUsize::new(0);
static GETS: AtomicUsize = AtomicUsize::new(0);
static RELEASE: AtomicBool = AtomicBool::new(false);

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_VOICE_DOWNLOAD_UI_PROOF").is_some()
}

pub fn release() {
    RELEASE.store(true, Ordering::Release);
}

pub fn report() -> Value {
    json!({"rights": RIGHTS.load(Ordering::Acquire), "urls": URLS.load(Ordering::Acquire),
        "gets": GETS.load(Ordering::Acquire), "options": OPTIONS.load(Ordering::Acquire)})
}

pub fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    match path {
        "/api/dj/voices/download/check" => {
            if args != &json!({"voiceIdsStr":["401"]}) {
                return Some(Err(Error::State(
                    "wrong synthetic voice authorization identity",
                )));
            }
            RIGHTS.fetch_add(1, Ordering::AcqRel);
            Some(Ok(json!({"code":200,"data":{"allowDownloadVoiceIds":[401],
                "forbidDownloadVoiceIds":[],"downloadCheckResultList":[{"voiceId":401,"canDownload":true}]}})))
        }
        "/api/dj/program/song/musics" => {
            if args != &json!({"ids":["401"]}) {
                return Some(Err(Error::State(
                    "wrong synthetic episode quality identity",
                )));
            }
            let number = OPTIONS.fetch_add(1, Ordering::AcqRel);
            let musics = if number == 0 {
                json!([])
            } else {
                json!([{"bitrate":192000,"size":2097152},{"bitrate":128000,"size":1048576}])
            };
            Some(Ok(json!({"code":200,"programs":[{"musics":musics}]})))
        }
        "/api/song/enhance/download/url/v1" => {
            if args != &json!({"id":"1","level":"higher","immerseType":"c51"}) {
                return Some(Err(Error::State("wrong synthetic voice media identity")));
            }
            URLS.fetch_add(1, Ordering::AcqRel);
            Some(Ok(
                json!({"code":200,"data":{"id":1,"code":200,"url":MEDIA,"type":"mp3",
                "md5":"485aa2633a203a9ea13fd7416caeb9f9","size":AUDIO.len(),"level":"higher"}}),
            ))
        }
        _ => None,
    }
}

pub struct Media;
impl MediaBackend for Media {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            if request.url.as_str() != MEDIA || request.offset != 0 {
                return Err(Error::State("voice download proof forbids other media"));
            }
            GETS.fetch_add(1, Ordering::AcqRel);
            Ok(MediaReply {
                status: 200,
                headers: [("content-length".into(), AUDIO.len().to_string())].into(),
                body: Box::new(Body(AUDIO)),
            })
        })
    }
}

struct Body(&'static [u8]);
impl MediaBody for Body {
    fn read<'a>(&'a mut self, buffer: &'a mut [u8]) -> ApiFuture<'a, usize> {
        Box::pin(async move {
            tokio::time::timeout(std::time::Duration::from_secs(12), async {
                while !RELEASE.load(Ordering::Acquire) {
                    tokio::time::sleep(std::time::Duration::from_millis(15)).await;
                }
            })
            .await
            .map_err(|_| Error::State("synthetic voice download release timed out"))?;
            let len = self.0.len().min(buffer.len());
            buffer[..len].copy_from_slice(&self.0[..len]);
            self.0 = &self.0[len..];
            Ok(len)
        })
    }
}
