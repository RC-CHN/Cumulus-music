use cumulus_core::{
    Result,
    playback::keep_awake::{PlaybackPowerRequest, PlaybackPowerSink},
    protocol::pc_experiments::PcExperimentGroups,
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};

static READS: AtomicUsize = AtomicUsize::new(0);
static KEEPS: AtomicUsize = AtomicUsize::new(0);

fn mode() -> Option<String> {
    std::env::var("CUMULUS_PLAYBACK_POWER_GUIDE_PROOF").ok()
}

pub fn enabled() -> bool {
    mode().is_some()
}

pub fn assignment() -> Result<Option<PcExperimentGroups>> {
    let Some(mode) = mode() else { return Ok(None) };
    READS.fetch_add(1, Ordering::Relaxed);
    let (power, system_media) = match mode.as_str() {
        "offer" | "enabled" => ("t1", ""),
        "smtc" => ("t1", "t1"),
        "control" => ("c", ""),
        _ => panic!("unknown playback guide fixture"),
    };
    PcExperimentGroups::refreshed(&json!({"data":[
        {"expName":"PH-PC-keepAwake","expGroupName":power},
        {"expName":"PH-PC-newSMTCoff","expGroupName":system_media}
    ]}))
    .map(Some)
}

pub fn initialize(store: &StateStore) -> Result<()> {
    if mode().as_deref() == Some("enabled") {
        store.put(
            "local-device",
            "playback-power-v1",
            "enabled",
            &Value::Bool(true),
        )?;
    }
    Ok(())
}

pub fn home_response(path: &str, args: &Value) -> Option<Value> {
    if !enabled() {
        return None;
    }
    Some(match path {
        "/api/user/setting" => json!({"code":200,"setting":{"personalServiceSetting":0}}),
        "/api/batch" if args["/api/homepage/block/page"].is_string() => json!({
            "code":200,"/api/homepage/block/page":{"code":200,"data":{"blocks":[{
                "blockCode":"HOMEPAGE_BLOCK_PLAYLIST_RCMD",
                "creatives":([731,732,733].map(|id|json!({
                    "uiElement":{"mainTitle":{"title":format!("合成推荐歌单 {id}")}},
                    "resources":[{"resourceType":"playlist","resourceId":id}]
                })))
            }]}}
        }),
        "/api/pc/recent/listen/list" => json!({"code":200,"data":{"resources":[]}}),
        _ => return None,
    })
}

pub fn report(store: &StateStore) -> Result<Value> {
    Ok(json!({
        "assignmentReads":READS.load(Ordering::Relaxed),
        "powerKeeps":KEEPS.load(Ordering::Relaxed),
        "savedPower":store.get("local-device","playback-power-v1","enabled")?
    }))
}

pub struct Sink;
impl PlaybackPowerSink for Sink {
    fn submit(&self, request: PlaybackPowerRequest) {
        if request == PlaybackPowerRequest::KeepSystemAndDisplayAwake {
            KEEPS.fetch_add(1, Ordering::Relaxed);
        }
    }
}
