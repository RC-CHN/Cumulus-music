//! Only the tray GUI proof uses this mutable synthetic account backend.
use cumulus_core::{
    Error, Result,
    protocol::api::{Api, ApiFuture},
};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
pub struct Library {
    pub base: super::relay_fixture::Library,
    likes: Mutex<Vec<String>>,
    pub writes: Mutex<Vec<Value>>,
    pub voice_writes: Mutex<Vec<Value>>,
    voice_liked: AtomicBool,
    pub lose: AtomicBool,
}
impl Default for Library {
    fn default() -> Self {
        Self {
            base: Default::default(),
            likes: Mutex::new(vec!["3".into(), "7".into()]),
            writes: Mutex::new(vec![]),
            voice_writes: Mutex::new(vec![]),
            voice_liked: AtomicBool::new(false),
            lose: AtomicBool::new(false),
        }
    }
}
impl Api for Library {
    fn local_device_id(&self) -> Result<Option<String>> {
        self.base.local_device_id()
    }
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            match path {
                "/api/dj/user/setting/resumeplay/get" => {
                    Ok(json!({"code":200,"data":{"resumePlayValue":0}}))
                }
                "/api/content/my/liked/voice" => Ok(
                    json!({"code":200,"data":{"data":if self.voice_liked.load(Ordering::Acquire){vec![voice()]}else{vec![]},"hasMore":false}}),
                ),
                "/api/resource/like" | "/api/resource/unlike" => {
                    assert_eq!(payload["threadId"], "A_DJ_1_5");
                    self.voice_writes.lock().unwrap().push(payload);
                    self.voice_liked
                        .store(path.ends_with("/like"), Ordering::Release);
                    Ok(json!({"code":200}))
                }
                "/api/song/like/get" => {
                    Ok(json!({"code":200,"ids":self.likes.lock().unwrap().clone()}))
                }
                "/api/song/like" => {
                    assert_eq!(payload["userid"], "2");
                    assert_eq!(payload["trackId"], "2");
                    self.writes.lock().unwrap().push(payload.clone());
                    let id = payload["trackId"].as_str().unwrap().to_owned();
                    let mut likes = self.likes.lock().unwrap();
                    likes.retain(|value| value != &id);
                    if payload["like"] == true {
                        likes.push(id);
                    }
                    if self.lose.swap(false, Ordering::AcqRel) {
                        return Err(Error::Transport("synthetic like reply lost".into()));
                    }
                    Ok(json!({"code":200}))
                }
                _ => self.base.request(path, payload, cache).await,
            }
        })
    }
}

pub fn voice() -> Value {
    json!({"id":"5","name":"合成声音","mainSong":{"id":"1005","duration":30000},"duration":30000,"radio":{"id":8,"name":"合成播客"}})
}
pub fn voice_resource() -> Value {
    let voice = cumulus_core::podcast::Episode::from_official(&voice()).unwrap();
    let mut resource = voice.playback_resource().unwrap();
    resource["song"] = voice.download_metadata().unwrap();
    resource
}
