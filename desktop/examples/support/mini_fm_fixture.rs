//! Synthetic FM service for the two-window probe. No network fallback.
#[allow(dead_code)]
#[path = "../../../core/tests/support/fm.rs"]
mod domain;
use cumulus_core::{Result, protocol::api::Api};
use serde_json::{Value, json};

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_MINI_FM_PROOF").is_some()
}
pub struct Fixture {
    api: domain::Backend,
}
impl Default for Fixture {
    fn default() -> Self {
        let api = domain::Backend::default();
        if enabled() {
            api.batch(&(101..=140).collect::<Vec<_>>());
        }
        Self { api }
    }
}
impl Fixture {
    pub async fn response(&self, path: &str, args: &Value) -> Option<Result<Value>> {
        if !enabled() {
            return None;
        }
        if path == "/api/music-vip-membership/client/vip/info" {
            return Some(Ok(
                json!({"code":200,"data":{"userId":2,"now":1793410000000u64}}),
            ));
        }
        if matches!(
            path,
            "/api/v1/radio/get"
                | "/api/tag/tab/fm/recommends"
                | "/api/v1/radio/skip"
                | "/api/v3/radio/trash/get"
                | "/api/music/dislike/get"
        ) {
            return Some(self.api.request(path, args.clone(), false).await);
        }
        None
    }
    pub fn report(&self) -> Value {
        json!({"personalReads":self.api.count("/api/v1/radio/get"),"zoneReads":self.api.count("/api/tag/tab/fm/recommends"),"skipWrites":self.api.count("/api/v1/radio/skip"),"trashWrites":self.api.count("/api/radio/trash/add")})
    }
}
