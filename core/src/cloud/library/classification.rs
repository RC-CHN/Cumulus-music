//! Deletion warnings from the recovered removeFromCloudDisk action and M/21.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudDeletionKind {
    Ordinary,
    Private,
    OffStore,
    Unknown,
}

impl CloudDeletionKind {
    pub(super) fn from_api(simple: &Value) -> Self {
        // M/21 normalizes privilege.cs to cloudSong=1/0; missing is private.
        if !crate::protocol::wire::serialization::js_truthy(&simple["privilege"]["cs"])
            || simple["t"].as_f64() == Some(1.0)
        {
            Self::Private
        } else if simple["t"].as_f64() == Some(3.0)
            || (simple["t"].as_f64() == Some(2.0) && negative(&simple["pst"]))
        {
            Self::OffStore
        } else {
            Self::Ordinary
        }
    }

    /// Old snapshots did not retain type/pstatus. Do not infer ordinary from
    /// their playable/shareable status or their distinct cloud/catalog IDs.
    pub fn from_cached(song: &Value) -> Self {
        serde_json::from_value(song["_cloud_delete_kind"].clone()).unwrap_or(Self::Unknown)
    }
}

fn negative(value: &Value) -> bool {
    match value {
        Value::Number(number) => number.as_f64().is_some_and(|n| n < 0.0),
        Value::String(value) => {
            let text = crate::protocol::wire::serialization::trim_js(value);
            // pstatus is numeric on the recovered API. Preserve negative decimal
            // string replies too, without treating arbitrary text as a number.
            (text == "-Infinity")
                || (text
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b"+-eE.".contains(&b))
                    && text.parse::<f64>().is_ok_and(|n| n < 0.0))
        }
        _ => false,
    }
}
