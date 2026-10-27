use cumulus_core::{storage::StateStore, watchman::*};
use serde_json::{Value, json};
use std::sync::Arc;

const BUILD: SdkBuild = SdkBuild::B602a5ad7;
fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/watchman-current-official.json")).unwrap()
}
#[test]
fn current_official_packet_bytes_inverse_and_wrong_version_rejection() {
    let f = fixture();
    assert_eq!(f["source_sha256"], BUILD.source_sha256());
    assert_eq!(SdkBuild::parse(BUILD.version()).unwrap(), BUILD);
    assert!(SdkBuild::parse("2.7.5_future").is_err());
    for v in f["packets"].as_array().unwrap() {
        let bytes = hex::decode(v["payload"].as_str().unwrap()).unwrap();
        let seed = hex::decode(v["seed"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap();
        let encoded = encode_packet_for(BUILD, &bytes, seed).unwrap();
        assert_eq!(encoded, v["encoded"]);
        assert_eq!(decode_packet_for(BUILD, &encoded).unwrap(), bytes);
        assert!(decode_packet(&encoded).is_err());
        assert!(decode_packet_for(BUILD, &encode_packet(&bytes, seed).unwrap()).is_err());
    }
    let max = vec![255; MAX_BYTES];
    assert_eq!(
        decode_packet_for(
            BUILD,
            &encode_packet_for(BUILD, &max, [0, 1, 128, 255]).unwrap()
        )
        .unwrap(),
        max
    );
}
#[test]
fn current_official_shared_codecs_and_fields_still_match() {
    let f = fixture();
    for v in f["text"].as_array().unwrap() {
        assert_eq!(
            encode_text(v["input"].as_str().unwrap()).unwrap(),
            v["encoded"]
        );
    }
    for v in f["nonce"].as_array().unwrap() {
        assert_eq!(
            request_nonce(
                v["ms"].as_u64().unwrap(),
                hex::decode(v["seed"].as_str().unwrap())
                    .unwrap()
                    .try_into()
                    .unwrap()
            ),
            v["value"]
        );
    }
    for v in f["tokens"].as_array().unwrap() {
        let did = match v["did"]["kind"].as_str().unwrap() {
            "text" => StoredValue::Text(v["did"]["value"].as_str().unwrap()),
            "null" => StoredValue::Null,
            _ => StoredValue::Undefined,
        };
        assert_eq!(
            encode_token(
                &v["ret"],
                did,
                v["nonce"].as_str().unwrap(),
                v["fallback"].as_bool().unwrap()
            )
            .unwrap(),
            v["encoded"]
        );
    }
    let table = descriptors().unwrap();
    for v in f["fields"].as_array().unwrap() {
        let descriptor = table.iter().find(|d| d.key == v["key"]).unwrap();
        let actual = encode_field(
            descriptor,
            &v["value"],
            v["collectionError"].as_bool().unwrap_or(false),
        );
        if v.get("error").is_some() {
            assert!(actual.is_err());
        } else {
            assert_eq!(hex::encode(actual.unwrap()), v["hex"]);
        }
    }
}
#[test]
fn current_official_public_fields_use_changed_tags_and_id_802() {
    for v in fixture()["publicFields"].as_array().unwrap() {
        let all = v["all"].as_bool().unwrap();
        let context = CommonFields {
            product: "YD00000558929251",
            business: all.then_some("synthetic-business"),
            nonce: all.then_some("synthetic-nonce"),
            tid: if all { "synthetic-tid" } else { "" },
            did: if all { "synthetic-did" } else { "" },
            track: all.then_some("track-中"),
            local_ms: v["time"].as_u64().unwrap(),
            server_offset_ms: v["offset"].as_i64().unwrap(),
            sequence: (v["previousSeq"].as_u64().unwrap() as u8).saturating_add(1),
        };
        let mut actual: Vec<_> = public_fields_for(
            BUILD,
            context,
            hex::decode(v["random"].as_str().unwrap())
                .unwrap()
                .try_into()
                .unwrap(),
        )
        .unwrap()
        .iter()
        .map(hex::encode)
        .collect();
        actual.sort();
        assert_eq!(json!(actual), v["fields"]);
    }
}
#[test]
fn current_official_post_envelope_has_current_wire_version() {
    for v in fixture()["plans"].as_array().unwrap() {
        let request = Request {
            host: ApiHost::Primary,
            callback: "__wmjsonp_1122334".into(),
            kind: RequestKind::Report {
                build: BUILD,
                kind: if v["url"].as_str().unwrap().ends_with("/d") {
                    ReportKind::Device
                } else {
                    ReportKind::Behavior
                },
                packet: "synthetic+/.3".into(),
            },
        };
        assert_eq!(request.url().unwrap(), v["url"]);
        assert_eq!(request.form().unwrap(), v["body"]);
    }
}
#[test]
fn device_cache_upgrade_respects_current_build_and_server_last_used_version() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(StateStore::open(dir.path()).unwrap());
    let conf = encode_text("{}").unwrap();
    let old = Config::from_conf("testProduct", "unrelated", &conf).unwrap();
    let new = Config::from_conf_for(BUILD, "testProduct", old.build_version(), &conf).unwrap();
    let strict = Config::from_conf_for(BUILD, "testProduct", "unrelated", &conf).unwrap();
    let old_store = DeviceStore::new(db.clone(), old.clone());
    old_store
        .apply_response(
            0,
            ReportKind::Device,
            &json!([200, null, "tid-old", "did-old", null, "ni-old"]),
            1000,
            None,
        )
        .unwrap();
    let state = old_store.snapshot().unwrap();
    assert!(state.ready(&new, 1001));
    assert!(!state.ready(&strict, 1001));
    let new_store = DeviceStore::new(db.clone(), new);
    new_store
        .apply_response(
            1,
            ReportKind::Device,
            &json!([200, null, "tid-new", "did-new", null, "ni-new"]),
            2000,
            None,
        )
        .unwrap();
    assert!(new_store.snapshot().unwrap().ready(&strict, 2001));
    assert!(!new_store.snapshot().unwrap().ready(&old, 2001));
    drop(new_store);
    drop(old_store);
    drop(db);
    let reopened = DeviceStore::new(
        Arc::new(StateStore::open(dir.path()).unwrap()),
        strict.clone(),
    );
    assert!(reopened.snapshot().unwrap().ready(&strict, 2001));
    assert_eq!(reopened.snapshot().unwrap().did_native(), "did-new");
}
