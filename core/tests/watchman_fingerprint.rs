use cumulus_core::watchman::{descriptors, encode_field, fingerprint_hash};
use serde_json::Value;

#[test]
fn official_fingerprint_uses_utf16_blocks_and_unmasked_tail_units() {
    let fixtures: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/watchman-fingerprint-official.json")).unwrap();
    for fixture in fixtures {
        for case in fixture["fingerprints"].as_array().unwrap() {
            assert_eq!(
                fingerprint_hash(
                    case["input"].as_str().unwrap(),
                    case["seed"].as_u64().unwrap() as u32
                ),
                case["hash"],
                "{case}"
            );
        }
    }
}

#[test]
fn official_device_fields_preserve_array_truncation_and_numeric_coercion() {
    let fixtures: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/watchman-fingerprint-official.json")).unwrap();
    let fields = descriptors().unwrap();
    for fixture in fixtures {
        for case in fixture["fields"].as_array().unwrap() {
            let field = fields
                .iter()
                .find(|f| f.key == case["key"].as_str().unwrap())
                .unwrap();
            let result = encode_field(field, &case["value"], false);
            if case.get("error").is_some() {
                assert!(result.is_err(), "{case}");
            } else {
                assert_eq!(hex::encode(result.unwrap()), case["hex"], "{case}");
            }
        }
    }
}
