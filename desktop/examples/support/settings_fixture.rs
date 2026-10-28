//! Synthetic account settings; every mutation stays in this process.
use cumulus_core::{
    Error,
    protocol::api::{Api, ApiFuture},
};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicU8, AtomicUsize, Ordering},
};
pub struct Library {
    base: super::fixture::Library,
    mode: AtomicU8,
    settings: Mutex<Value>,
    pub writes: AtomicUsize,
}
impl Default for Library {
    fn default() -> Self {
        Self {
            base: Default::default(),
            mode: AtomicU8::new(0),
            writes: AtomicUsize::new(0),
            settings: Mutex::new(
                json!({"shareSetting":0,"playRecordSetting":0,"personalServiceSetting":0,"allowSubscriptionNotify":true,"allowLikedNotify":true,"allowNewFollowerNotify":true,"allowPeopleCanSeeMyPlayRecord":true,"allowFollowedCanSeeMyPlayRecord":true,"socialPlaylistSetting":0,"nativeOnly":"SYNTHETIC_PRIVATE"}),
            ),
        }
    }
}
impl Library {
    pub fn mode(&self, value: &str) {
        self.mode.store(
            match value {
                "offline" => 1,
                "lost" => 2,
                "rejected" => 3,
                "unapplied" => 4,
                "unknown" => 5,
                "slow-read" => 6,
                _ => 0,
            },
            Ordering::SeqCst,
        );
    }
}
impl Api for Library {
    fn request<'a>(&'a self, path: &'a str, args: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let mode = self.mode.load(Ordering::SeqCst);
            if path == "/api/user/setting" {
                if mode == 6 {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
                if mode == 1 {
                    return Err(Error::Transport("synthetic offline".into()));
                }
                let mut values = self.settings.lock().unwrap().clone();
                if mode == 5 {
                    values["playRecordSetting"] = json!(99);
                    values["personalServiceSetting"] = json!("0");
                    values.as_object_mut().unwrap().remove("allowLikedNotify");
                }
                return Ok(json!({"code":200,"setting":values}));
            }
            if path == "/api/user/setting/update" {
                self.writes.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                if mode == 3 {
                    return Ok(json!({"code":403}));
                }
                if mode == 4 {
                    return Err(Error::Transport(
                        "synthetic reply lost without applying".into(),
                    ));
                }
                let patch = args
                    .as_object()
                    .ok_or(Error::Response("fixture missing patch"))?;
                if patch.len() != 1 {
                    return Err(Error::Response("fixture expects single setting"));
                }
                let mut values = self.settings.lock().unwrap();
                for (key, value) in patch {
                    values[key] = value.clone();
                    if key == "playRecordSetting" {
                        values["allowPeopleCanSeeMyPlayRecord"] = json!(value == 0);
                        values["allowFollowedCanSeeMyPlayRecord"] = json!(value != 2);
                        values["socialPlaylistSetting"] = value.clone();
                    }
                }
                if mode == 2 {
                    return Err(Error::Transport("synthetic lost acknowledgement".into()));
                }
                return Ok(json!({"code":200}));
            }
            self.base.request(path, args, cache).await
        })
    }
}
