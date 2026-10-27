use cumulus_core::identity::WindowsBrowserIdentity;
use serde_json::{Value, json};

#[test]
fn declared_identity_matches_recovered_native_ua_without_inventing_hardware() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/windows-compatibility-native.json")).unwrap();
    let declaration =
        WindowsBrowserIdentity::from_locales(&["C.UTF-8".into()], "zh_CN.UTF-8").unwrap();
    let value = serde_json::to_value(&declaration).unwrap();
    assert_eq!(value["userAgent"], fixture["officialUserAgent"]);
    assert_eq!(
        format!("Mozilla/{}", value["appVersion"].as_str().unwrap()),
        fixture["officialUserAgent"]
    );
    assert_eq!(value["platform"], "Win32");
    assert_eq!(value["languages"], json!(["zh-CN"]));
    assert_eq!(value["language"], "zh-CN");
    assert_eq!(value.as_object().unwrap().len(), 5);
    let native = declaration.native_profile().unwrap();
    assert!(native.client_sign.is_none());
    assert_eq!(native.os_version, fixture["descriptionCases"][0]["output"]);
}

#[test]
fn host_locale_order_is_stable_and_unsupported_values_use_explicit_application_locale() {
    let input = [
        "en_US.UTF-8",
        "zh_hANT_tw",
        "en-US",
        "C",
        "en\r\nInjected",
        "de_DE@euro",
        "",
        "<script>",
    ];
    let identity = WindowsBrowserIdentity::from_locales(&input.map(String::from), "zh-CN").unwrap();
    let value = serde_json::to_value(identity).unwrap();
    assert_eq!(value["languages"], json!(["en-US", "zh-Hant-TW"]));
    assert_eq!(value["language"], "en-US");
    assert!(WindowsBrowserIdentity::from_locales(&[], "C").is_err());
    assert!(WindowsBrowserIdentity::from_locales(&vec!["en-US".into(); 33], "en-US").is_err());
}
