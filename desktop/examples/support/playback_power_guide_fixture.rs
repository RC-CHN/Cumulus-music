use cumulus_core::{
    Result,
    playback::keep_awake::{PlaybackPowerRequest, PlaybackPowerSink},
    preferences::system_media::SystemMediaPreferences,
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

pub fn power_host_available() -> bool {
    mode().as_deref() != Some("smtc_close")
}

pub fn assignment() -> Result<Option<PcExperimentGroups>> {
    let Some(mode) = mode() else { return Ok(None) };
    READS.fetch_add(1, Ordering::Relaxed);
    let (power, system_media, legacy) = match mode.as_str() {
        "offer" | "enabled" => ("t1", "", ""),
        "smtc" | "smtc_close" | "smtc_unavailable" => ("t1", "t1", ""),
        "smtc_legacy" => ("t1", "t1", "t1"),
        "control" => ("c", "", ""),
        _ => panic!("unknown playback guide fixture"),
    };
    PcExperimentGroups::refreshed(&json!({"data":[
        {"expName":"PH-PC-keepAwake","expGroupName":power},
        {"expName":"PH-PC-newSMTCoff","expGroupName":system_media},
        {"expName":"PH-PC-SMTC","expGroupName":legacy}
    ]}))
    .map(Some)
}

pub fn initialize(store: &StateStore) -> Result<()> {
    let mode = mode();
    if mode.as_deref() == Some("enabled") {
        store.put(
            "local-device",
            "playback-power-v1",
            "enabled",
            &Value::Bool(true),
        )?;
    }
    if mode.as_deref() == Some("smtc") {
        let media = SystemMediaPreferences::read(store)?;
        if media.revision == "initial" {
            SystemMediaPreferences::replace(store, &media.revision, false)?;
        }
    }
    Ok(())
}

pub fn install_media(app: &tauri::AppHandle) {
    use std::sync::Arc;
    use tauri::Manager;
    if let Some(mode) = mode() {
        if cfg!(target_os = "windows") && mode == "smtc_unavailable" {
            return;
        }
        cumulus_desktop::system_media::install(app);
        if mode != "smtc_unavailable"
            && app
                .try_state::<Arc<cumulus_desktop::system_media::GuideHost>>()
                .is_none()
        {
            let guide = Arc::new(cumulus_desktop::system_media::GuideHost::default());
            guide.mark_ready();
            app.manage(guide);
        }
    }
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
        "savedPower":store.get("local-device","playback-power-v1","enabled")?,
        "savedSystemMedia":store.get("local-device","system-media-preferences-v1","current")?
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
