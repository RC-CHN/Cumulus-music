#[allow(dead_code)]
#[path = "support/intelligence_player.rs"]
mod support;
use cumulus_core::playback::{
    controller::PlaybackCommand as Cmd, intelligence::IntelligenceAction,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use support::*;

#[tokio::test]
async fn native_order_matches_executed_official_filter_insert_merge_vectors_including_capacity() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/official-intelligence-queue.json")).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 7);
    for case in cases {
        let f = Fixture::new();
        let old: Vec<u64> = serde_json::from_value(case["old"].clone()).unwrap();
        let index = case["current"].as_u64().unwrap() as usize;
        let seeded = act(&f, begin(old[0], false)).await.unwrap();
        let id = seeded.intelligence.unwrap().id;
        if old.len() > 1 {
            f.command(Cmd::Append {
                resources: old[1..].iter().map(|id| json!({"id":id})).collect(),
                source: json!({"kind":"test"}),
            })
            .await
            .unwrap();
        }
        let original = f.player.queue(&f.token()).await.unwrap();
        if index > 0 {
            f.command(Cmd::Select(original.items[index].entry_id.clone()))
                .await
                .unwrap();
        }
        f.command(Cmd::Pause(true)).await.unwrap();
        let before = f.command(Cmd::Seek(12.0)).await.unwrap();
        let resources = if case["range"].is_object() {
            let start = case["range"]["start"].as_u64().unwrap();
            let count = case["range"]["count"].as_u64().unwrap();
            (start..start + count).map(|id| json!({"id":id})).collect()
        } else {
            case["recommendations"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| {
                    if v.is_number() {
                        json!({"id":v})
                    } else {
                        v.clone()
                    }
                })
                .collect()
        };
        let applied = act(
            &f,
            IntelligenceAction::Complete {
                session_id: id,
                resources,
                recommendations: BTreeMap::new(),
            },
        )
        .await
        .unwrap();
        assert_eq!(applied.play_id, before.play_id, "{}", case["name"]);
        assert_eq!(applied.position, Some(12.0));
        let queue = f.player.queue(&f.token()).await.unwrap();
        let visible: Vec<_> = queue
            .items
            .iter()
            .filter(|e| e.source["intelligence_transient"] != true)
            .collect();
        let ids: Vec<_> = visible.iter().map(|e| e.resource["id"].clone()).collect();
        if case["range"].is_object() {
            assert_eq!(
                ids.len() as u64,
                case["expected"]["count"].as_u64().unwrap()
            );
            assert_eq!(ids.first().unwrap(), &case["expected"]["first"]);
            assert_eq!(ids.last().unwrap(), &case["expected"]["last"]);
            assert_eq!(
                hex::encode(Sha256::digest(serde_json::to_vec(&ids).unwrap())),
                case["expected"]["sha256"].as_str().unwrap()
            );
            assert_eq!(
                queue.items.len(),
                10_001,
                "detached audible current is private, not discarded by the cap"
            );
        } else {
            assert_eq!(json!(ids), case["expected"]["ids"], "{}", case["name"]);
            let markers: Vec<_> = visible
                .iter()
                .filter(|e| e.resource["marker"].is_string())
                .map(|e| json!({"id":e.resource["id"],"marker":e.resource["marker"]}))
                .collect();
            assert_eq!(json!(markers), case["expected"]["markers"]);
        }
    }
}
