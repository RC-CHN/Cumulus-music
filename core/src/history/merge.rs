use super::HistoryRecord;
use std::collections::HashMap;
fn truthy(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::Number(n) => n.as_f64() != Some(0.0),
        serde_json::Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

/// M/542: local wins equal time, missing creator is backfilled, and a newer
/// remote voice retains the local resume progress. Stable ties follow insertion.
pub fn merge_records(local: &[HistoryRecord], remote: &[HistoryRecord]) -> Vec<HistoryRecord> {
    let mut result: Vec<HistoryRecord> = Vec::new();
    let mut positions = HashMap::new();
    for item in local {
        let mut item = item.clone();
        // M/542 labels the non-macOS desktop category as pc. This is not
        // a claim about the host OS or a device-fingerprint measurement.
        item.device = Some("pc".into());
        if let Some(&index) = positions.get(&item.id) {
            result[index] = item;
        } else {
            positions.insert(item.id.clone(), result.len());
            result.push(item);
        }
    }
    for item in remote {
        if let Some(&index) = positions.get(&item.id) {
            let previous = &mut result[index];
            if !truthy(&previous.resource["creator"]) && item.resource.get("creator").is_some() {
                previous.resource["creator"] = item.resource["creator"].clone();
            }
            if previous.played_at < item.played_at {
                let mut incoming = item.clone();
                incoming.play_progress = previous.play_progress;
                *previous = incoming;
            }
        } else {
            positions.insert(item.id.clone(), result.len());
            let mut incoming = item.clone();
            // The original assigns previous?.playProgress, even without a local row.
            incoming.play_progress = None;
            result.push(incoming);
        }
    }
    result.sort_by_key(|r| std::cmp::Reverse(r.played_at));
    result
}
