use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::OnceLock;

pub const AB_PATH: &str = "/api/rtrs/abt/front/expinfo/list";
pub const LOGIN_EXPERIMENTS: [&str; 2] = ["PH-PC-loginJsk", "PH-PC-newLoginBenefit"];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LoginExperiments {
    pub jsk: bool,
    pub benefit: bool,
}
impl LoginExperiments {
    pub fn enabled(&self) -> bool {
        self.jsk && self.benefit
    }
    pub fn payload() -> Value {
        json!({"expNames":LOGIN_EXPERIMENTS.join(",")})
    }
    /// M/147 takes the first result per experiment and falls back to an empty
    /// group. M/34 requires both groups to be exactly "t1".
    pub fn from_response(value: &Value) -> Result<Self> {
        if value["code"] != json!(200) {
            return Err(Error::Response("experiment response rejected"));
        }
        let rows = value["data"]
            .as_array()
            .ok_or(Error::Response("experiment list missing"))?;
        if rows.len() > 4096 {
            return Err(Error::Response("experiment list too large"));
        }
        let hit = |name: &str| {
            rows.iter()
                .find(|r| r["expName"] == name)
                .is_some_and(|r| r["expGroupName"] == "t1")
        };
        Ok(Self {
            jsk: hit(LOGIN_EXPERIMENTS[0]),
            benefit: hit(LOGIN_EXPERIMENTS[1]),
        })
    }
}

/// Exact prefix normalization used by M/1368, including its lack of a slash
/// boundary after api/eapi. Callers still validate actual network endpoints.
pub fn normalized_path(value: &str) -> String {
    static PREFIX: OnceLock<regex::Regex> = OnceLock::new();
    PREFIX
        .get_or_init(|| {
            regex::Regex::new(r"^[^\n\r\u{2028}\u{2029}]*?/(e?api)").expect("literal regexp")
        })
        .replace(value, "")
        .into_owned()
}
