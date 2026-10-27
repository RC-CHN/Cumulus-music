use cumulus_core::listen_together::RoomMessage;
use serde_json::{Value, json};

#[test]
fn progress_ignore_selection_matches_executed_official_dispatch() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/together-progress-official.json")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let message = RoomMessage::from_attachment(
            &json!({"content":{"type":20008,"content":case["content"]}}).to_string(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            message
                .requests_progress_for(case["owner"].as_str().unwrap())
                .unwrap(),
            case["report"].as_bool().unwrap()
        );
    }
    for value in [json!({}), json!(true), json!("1"), json!(1)] {
        let message = RoomMessage::from_attachment(
            &json!({"content":{"type":20008,"content":{"ignoreUserIds":value}}}).to_string(),
        )
        .unwrap()
        .unwrap();
        assert!(message.requests_progress_for("1").is_err());
    }
    let unrelated = RoomMessage::from_attachment(
        r#"{"content":{"type":20009,"content":{"ignoreUserIds":[]}}}"#,
    )
    .unwrap()
    .unwrap();
    assert!(!unrelated.requests_progress_for("1").unwrap());
}
