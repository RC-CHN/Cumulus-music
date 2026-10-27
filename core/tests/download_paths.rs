use cumulus_core::download::naming::{PathRequest, plan_path};
use serde_json::{Value, json};

#[test]
fn all_nine_layouts_track_voice_mv_cloud_match_official_builder() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/download_paths.json")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let request: PathRequest = serde_json::from_value(case["input"].clone()).unwrap();
        let plan = plan_path(&request).unwrap();
        assert_eq!(
            plan.official_windows_path,
            case["expected"].as_str().unwrap(),
            "{}",
            case["name"]
        );
    }
}

fn request(name: &str) -> PathRequest {
    serde_json::from_value(json!({"pathType":0,"fileType":1,"track":{"name":name},"offlinePathLength":20,"suffix":"flac"})).unwrap()
}
#[test]
fn portable_components_cover_reserved_names_utf8_and_utf16_boundaries() {
    for (name, wanted) in [
        ("con", "_con.flac"),
        ("LPT¹.foo", "_LPT¹.foo.flac"),
        ("aux ", "_aux.flac"),
        ("COM10", "COM10.flac"),
        ("..\\../../evil:foo|\0", "＼....evilfoo.flac"),
    ] {
        let plan = plan_path(&request(name)).unwrap();
        assert_eq!(plan.relative.to_str().unwrap(), wanted);
    }
    let name = "😀".repeat(300);
    let plan = plan_path(&request(&name)).unwrap();
    assert!(plan.relative.file_name().unwrap().as_encoded_bytes().len() <= 255);
    assert!(plan.portable_adjustment);
    // M/27's 200-unit budget does not subtract the final extension dot.
    assert!(plan.official_windows_path.encode_utf16().count() + 20 <= 201);
    let mut r = request("a");
    r.offline_path_length = 200;
    assert!(plan_path(&r).is_err());
    r = request("x");
    r.suffix = "../x".into();
    assert!(plan_path(&r).is_err());
    assert!(plan_path(&request("../")).is_err());
}
