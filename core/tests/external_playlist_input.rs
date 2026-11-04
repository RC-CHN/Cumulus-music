use cumulus_core::playlist::preview_external_playlist_inputs;
use serde_json::Value;

#[test]
fn recovered_external_playlist_examples_are_recognized_without_exposing_payloads() {
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/external_playlist_input.json")).unwrap();
    for row in reference["vectors"].as_array().unwrap() {
        let input = row["input"].as_str().unwrap().to_owned();
        let preview = preview_external_playlist_inputs(std::slice::from_ref(&input)).unwrap();
        let actual = serde_json::to_value(&preview[0]).unwrap();
        assert_eq!(actual, row["preview"], "synthetic input {input}");
        assert!(!actual.to_string().contains("PRIVATE"));
    }
}

#[test]
fn multi_link_preview_preserves_positions_and_refuses_oversized_work() {
    let link = "https://y.qq.com/playlist/synthetic".to_owned();
    let items = preview_external_playlist_inputs(&[link.clone(), String::new(), link]).unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0], items[2]);
    assert_ne!(items[0], items[1]);
    assert!(preview_external_playlist_inputs(&vec![String::new(); 11]).is_err());
    assert!(preview_external_playlist_inputs(&["a".repeat(16 * 1024 + 1)]).is_err());
}
