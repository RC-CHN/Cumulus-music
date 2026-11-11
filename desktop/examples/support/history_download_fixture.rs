//! A closed rights service and media reader for history episode downloads.
use cumulus_core::{
    Error, Result,
    download::transfer::{MediaBackend, MediaBody, MediaReply, MediaRequest},
    protocol::api::ApiFuture,
};
use serde_json::{Value, json};
use std::sync::Mutex;

const AUDIO: &[u8] = include_bytes!("../../../core/tests/fixtures/tag_audio/seed.mp3");
const URL: &str = "https://m1.music.126.net/cumulus-history-voice-fixture.mp3";

#[derive(Default)]
pub struct Fixture(Mutex<Vec<Value>>);

impl Fixture {
    pub fn report(&self) -> Value {
        json!(*self.0.lock().unwrap())
    }

    pub fn response(&self, path: &str, args: &Value, cached: bool) -> Option<Result<Value>> {
        let value = match path {
            "/api/dj/voices/download/check" => args["voiceIdsStr"][0]
                .as_str()
                .filter(|id| !cached && matches!(*id, "1" | "2" | "3") && args == &json!({"voiceIdsStr":[id]}))
                .map(|id| {
                    let allowed = id != "3";
                    json!({"code":200,"data":{
                        "allowDownloadVoiceIds":if allowed {vec![id]} else {vec![]},
                        "forbidDownloadVoiceIds":if allowed {vec![]} else {vec![id]},
                        "downloadCheckResultList":[{"voiceId":id,"canDownload":allowed}]}})
                }),
            "/api/dj/program/song/musics" => (args == &json!({"ids":["1"]}) && !cached)
                .then(|| json!({"code":200,"programs":[{"musics":[{"bitrate":128000,"size":1048576}]}]})),
            "/api/song/enhance/download/url/v1" => args["id"]
                .as_str()
                .filter(|id| !cached && matches!(*id, "1001" | "1002") && args == &json!({"id":id,"level":"standard","immerseType":"c51"}))
                .map(|id| json!({"code":200,"data":{"id":id,"code":200,"url":URL,"type":"mp3",
                    "md5":"485aa2633a203a9ea13fd7416caeb9f9","size":AUDIO.len(),"level":"standard"}})),
            _ => return None,
        };
        self.0
            .lock()
            .unwrap()
            .push(json!({"path":path,"args":args}));
        Some(value.ok_or(Error::State("unexpected history download fixture request")))
    }
}

impl MediaBackend for Fixture {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            if request.url.as_str() != URL || request.offset != 0 {
                return Err(Error::State("history download fixture forbids other media"));
            }
            self.0.lock().unwrap().push(json!({"path":"transfer"}));
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
            let len = self.0.len().min(buffer.len());
            buffer[..len].copy_from_slice(&self.0[..len]);
            self.0 = &self.0[len..];
            Ok(len)
        })
    }
}
