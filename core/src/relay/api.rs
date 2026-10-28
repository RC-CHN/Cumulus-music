use super::{PullPlan, RelayAlert, RelayConfig, RelayUpload};
use crate::{Error, Result, account::login_policy::AB_PATH, protocol::api::Api};
use serde_json::json;
use std::sync::Arc;

pub const CONFIG_PATH: &str = "/api/relay/config/get";
pub const SETTING_PATH: &str = "/api/relay/setting/show";
pub const SETTING_UPDATE_PATH: &str = "/api/relay/setting/update";
pub const SOURCE_PATH: &str = "/api/relay/songlist/submit";
pub const STATE_PATH: &str = "/api/relay/play/state/submit";
pub const PULL_PATH: &str = "/api/relay/play/pull";
pub const ALERT_PATH: &str = "/api/link/position/show/resource";

/// Low-level native contracts. Account-aware callers use RelayService; this
/// transport adapter alone does not authorize publishing or replay a request.
pub struct RelayApi {
    api: Arc<dyn Api>,
}
impl RelayApi {
    pub fn new(api: Arc<dyn Api>) -> Self {
        Self { api }
    }
    pub async fn experiment(&self) -> Result<bool> {
        let response = self
            .api
            .call(AB_PATH, json!({"expNames":"device_connect"}), false, &[200])
            .await?;
        let rows = response["data"]
            .as_array()
            .filter(|r| r.len() <= 4096)
            .ok_or(Error::Response("relay experiment list missing"))?;
        Ok(rows
            .iter()
            .find(|r| r["expName"] == "device_connect")
            .is_some_and(|r| r["expGroupName"] == "t2"))
    }
    pub async fn config(&self) -> Result<RelayConfig> {
        let response = self.api.call(CONFIG_PATH, json!({}), false, &[200]).await?;
        RelayConfig::from_data(&response["data"])
    }
    pub async fn setting(&self) -> Result<Option<bool>> {
        let response = self
            .api
            .call(
                SETTING_PATH,
                json!({"all":false,"settingKey":"RELAY_SWITCH"}),
                false,
                &[200],
            )
            .await?;
        // Missing groups is unavailable, never a fabricated OFF switch.
        Ok(response["data"]["groups"][0]["deviceSettingList"][0]["enable"].as_bool())
    }
    pub async fn update_setting(&self, local_device_id: &str, enable: bool) -> Result<()> {
        super::types::validate_device(local_device_id)?;
        self.api.call(SETTING_UPDATE_PATH, json!({"targetDeviceId":local_device_id,"enable":enable,"settingKey":"RELAY_SWITCH"}), false, &[200]).await?;
        Ok(())
    }
    pub async fn alert(&self) -> Result<Option<RelayAlert>> {
        let response = self.api.call(ALERT_PATH, json!({"positionCode":"multi_terminal_reconnect_info","extJson":{"interactRecords":[],"preData":[],"states":{"relayInfo":{"current":"{\"needCheck\":true}","prev":""}}}}), false, &[200]).await?;
        response["data"]["commonResourceList"][0]
            .get("generalizedObject")
            .filter(|v| !v.is_null())
            .cloned()
            .map(RelayAlert::from_official)
            .transpose()
    }
    pub async fn pull(&self, alert: &RelayAlert) -> Result<PullPlan> {
        let response = self
            .api
            .call(PULL_PATH, alert.payload()?, false, &[200])
            .await?;
        PullPlan::from_data(&response["data"])
    }
    pub async fn submit(&self, upload: &RelayUpload) -> Result<()> {
        let (path, payload) = match upload {
            RelayUpload::Source { value, .. } | RelayUpload::Commands(value) => (
                SOURCE_PATH,
                json!({"songListSubmitReq":serde_json::to_string(value)?}),
            ),
            RelayUpload::State(value) => (
                STATE_PATH,
                json!({"playStateSubmitReq":serde_json::to_string(value)?}),
            ),
        };
        self.api.call(path, payload, false, &[200]).await?;
        Ok(())
    }
}
