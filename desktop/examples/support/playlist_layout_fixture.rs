//! Display-only playlist variations; the production client never loads this module.
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};

static RELEASE_FAILURE: AtomicBool = AtomicBool::new(false);

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_PLAYLIST_LAYOUT_PROOF").is_some()
}

pub fn decorate(mut playlist: Value, id: u64) -> Value {
    if !enabled() {
        return playlist;
    }
    match id {
        2 => {
            playlist["description"] = json!(format!(
                "{}\n{}\n简介结束标记",
                "沿着熟悉的小路，把今天的音乐与故事留在这里。\n".repeat(16),
                "SyntheticDescriptionWithoutWordBreaks".repeat(8)
            ));
            playlist["creator"]["nickname"] =
                json!("合成用户的很长很长的名字，用于检查作者信息边界");
        }
        3 => playlist["description"] = json!("一段简短的介绍。"),
        4 => {
            playlist["description"] = json!("");
            playlist["trackCount"] = json!(0);
            playlist["trackIds"] = json!([]);
        }
        5 => playlist["trackIds"][0]["v"] = json!(-1),
        _ => {}
    }
    if id >= 20 {
        playlist["privacy"] = json!(0);
    }
    playlist
}

pub fn first_read_failure(id: u64) -> bool {
    static FAILED: AtomicBool = AtomicBool::new(false);
    enabled() && id == 26 && !FAILED.swap(true, Ordering::AcqRel)
}

pub fn release_failure() {
    RELEASE_FAILURE.store(true, Ordering::Release);
}

pub async fn wait_for_release() {
    // Hold the synthetic read until the loading frame has been observed, rather
    // than assuming that a slow Windows runner can capture within a fixed delay.
    for _ in 0..800 {
        if RELEASE_FAILURE.load(Ordering::Acquire) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}
