use cumulus_core::protocol::pc_experiments::{
    ASSIGNMENT_PATH, PcExperimentGroups, assignment_arguments,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn oracle() -> Value {
    serde_json::from_str(include_str!("fixtures/official_pc_experiments_3_1_42.json")).unwrap()
}

#[test]
fn request_matches_all_open_js_keys_then_the_exact_native_order() {
    let request = assignment_arguments();
    let args = request["expNames"].as_str().unwrap();
    assert_eq!(ASSIGNMENT_PATH, "/api/rtrs/abt/front/expinfo/list");
    assert_eq!(
        args.split(',').count(),
        oracle()["requestCount"].as_u64().unwrap() as usize
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(args)),
        oracle()["requestSha256"].as_str().unwrap()
    );
    assert!(args.contains(",PH-PC-skipSongs2,"));
    assert!(args.ends_with(",PH-PC-networkComponentV3,PH-PC-aegisEncryptV2"));
}

#[test]
fn server_rows_match_frozen_original_reducer_cases() {
    for case in oracle()["cases"].as_array().unwrap() {
        let groups = PcExperimentGroups::refreshed(&json!({ "data": case["rows"] })).unwrap();
        let label = case["name"].as_str().unwrap();
        assert_eq!(
            groups.skip_songs_assigned(),
            case["skipEnabled"].as_bool().unwrap(),
            "{label}"
        );
        assert_eq!(
            groups.group("PH-PC-VIP-songPlay"),
            case["vipGroup"].as_str(),
            "{label}"
        );
    }
}

#[test]
fn restore_and_refresh_failure_follow_original_separate_emotion_reset() {
    let fixture = oracle();
    let restored = PcExperimentGroups::from_startup_cache(&json!({
        "PH-PC-skipSongs2": { "expGroupName": "t1" },
        "PH-PC-qingxv": { "expGroupName": "t1" },
        "not-an-official-web-key": { "expGroupName": "t1" }
    }));
    assert_eq!(
        restored.skip_songs_assigned(),
        fixture["startup"]["skipEnabled"].as_bool().unwrap()
    );
    assert_eq!(
        restored.group("PH-PC-qingxv"),
        fixture["startup"]["emotionGroup"].as_str()
    );
    assert_eq!(restored.group("not-an-official-web-key"), None);
    assert_eq!(fixture["failureKeys"], json!(["PH-PC-qingxv"]));
    let previous = PcExperimentGroups::refreshed(&json!({"data":[
        {"expName":"PH-PC-skipSongs2","expGroupName":"t1"},
        {"expName":"PH-PC-qingxv","expGroupName":"t1"}
    ]}))
    .unwrap();
    let failed = previous.after_refresh_failure();
    assert!(failed.skip_songs_assigned());
    assert_eq!(failed.group("PH-PC-qingxv"), Some(""));
    assert!(PcExperimentGroups::refreshed(&json!({"data":[null]})).is_err());
    assert!(PcExperimentGroups::refreshed(&json!({"data":"untrusted"})).is_err());
}
