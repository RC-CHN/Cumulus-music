//! Read-only recommendation input for the signed, network-sealed GTK probe.
use cumulus_core::{Result, storage::StateStore};
use serde_json::{Value, json};
use std::sync::Mutex;

pub const CONFIG_URL: &str = "https://d5.music.126.net/obj/fixture/daily-notification.data";
const DAILY_V3: &str = "/api/v3/discovery/recommend/songs";
const MEDIA: &str = "/api/song/enhance/player/url/v1";
static CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());

pub fn mode() -> Option<String> {
    std::env::var("CUMULUS_DAILY_NOTIFICATION_PROOF").ok()
}

pub fn enabled() -> bool {
    mode().is_some()
}

pub fn config() -> Value {
    json!({"preload#push":{"day":3,"count":1,"t1":0.025,"desc":["合成的每日推荐"]}})
}

pub fn groups() -> [Value; 4] {
    let startup = if mode().as_deref() == Some("control") {
        "c"
    } else {
        "t1"
    };
    [
        json!({"expName":"PH-PC-push","expGroupName":startup}),
        json!({"expName":"PH-PC-push2","expGroupName":"c"}),
        json!({"expName":"PH-PC-Pop-Visual","expGroupName":"c"}),
        json!({"expName":"PH-PC-Blacklist","expGroupName":"t1"}),
    ]
}

fn song(id: u64) -> Value {
    json!({"id":id,"name":format!("合成歌曲 {id}"),"ar":[{"id":12,"name":"合成歌手"}],
           "al":{"id":7,"name":"合成专辑"},"dt":235000,"fee":0})
}

pub fn response(path: &str, args: &Value, cached: bool) -> Option<Value> {
    let mode = mode()?;
    let result = match path {
        "/api/user/setting" => {
            assert_eq!(args, &json!({}));
            assert!(!cached);
            let privacy = if mode == "privacy" { 2 } else { 0 };
            Some(json!({"code":200,"setting":{"personalServiceSetting":privacy}}))
        }
        DAILY_V3 => {
            assert_eq!(args, &json!({"limit":30}));
            assert!(!cached);
            Some(json!({"code":200,"data":{"dailySongs":(1..=4).map(song).collect::<Vec<_>>()}}))
        }
        MEDIA => None, // Standard sealed null-audio source.
        _ => return None,
    };
    CALLS.lock().unwrap().push(json!({"path":path,"args":args}));
    result
}

pub fn report(store: &StateStore) -> Result<Value> {
    let history = store.get("@device", "daily-notification-v1", "history")?;
    let queue = store.get("2", "playback", "queue")?;
    let sources = queue.and_then(|queue| {
        queue["items"].as_array().map(|rows| {
            rows.iter()
                .map(|row| row["source"].clone())
                .collect::<Vec<_>>()
        })
    });
    Ok(json!({"history":history,"calls":*CALLS.lock().unwrap(),"sources":sources}))
}
