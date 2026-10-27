use cumulus_core::{Error, storage::StateStore, watchman::*};
use serde_json::{Value, json};
use std::sync::{Arc, Barrier};
fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/watchman-official.json")).unwrap()
}
fn config(value: Value) -> Config {
    Config::from_conf(
        "YD00000558929251",
        "",
        &encode_text(&value.to_string()).unwrap(),
    )
    .unwrap()
}
fn stored(v: &Value) -> StoredValue<'_> {
    match v["kind"].as_str().unwrap() {
        "text" => StoredValue::Text(v["value"].as_str().unwrap()),
        "null" => StoredValue::Null,
        _ => StoredValue::Undefined,
    }
}

#[test]
fn official_text_nonce_and_token_bytes_include_missing_value_coercions() {
    let f = fixture();
    assert_eq!(f["source_sha256"], SOURCE_SHA256);
    for v in f["text"].as_array().unwrap() {
        let input = v["input"].as_str().unwrap();
        let encoded = encode_text(input).unwrap();
        assert_eq!(encoded, v["encoded"]);
        assert_eq!(decode_text(&encoded).unwrap(), input);
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
        assert_eq!(
            encode_token(
                &v["ret"],
                stored(&v["did"]),
                v["nonce"].as_str().unwrap(),
                v["fallback"].as_bool().unwrap()
            )
            .unwrap(),
            v["encoded"]
        );
    }
    for v in f["ndInfo"].as_array().unwrap() {
        assert_eq!(
            encode_nd_info(&json!(1), "", stored(&v["ni"])).unwrap(),
            v["encoded"]
        );
    }
    assert!(decode_text("a").is_err());
    assert!(decode_text("gg").is_err());
    assert!(encode_text(&"x".repeat(MAX_BYTES + 1)).is_err());
    assert!(encode_token(&json!([]), StoredValue::Undefined, "", false).is_err());
}

#[test]
fn official_packet_boundaries_inverse_and_corruption() {
    for v in fixture()["packets"].as_array().unwrap() {
        let bytes = hex::decode(v["payload"].as_str().unwrap()).unwrap();
        let seed = hex::decode(v["seed"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap();
        let encoded = encode_packet(&bytes, seed).unwrap();
        assert_eq!(encoded, v["encoded"]);
        assert_eq!(decode_packet(&encoded).unwrap(), bytes);
        let mut corrupt = encoded.into_bytes();
        corrupt[0] = if corrupt[0] == b'i' { b'c' } else { b'i' };
        assert!(decode_packet(std::str::from_utf8(&corrupt).unwrap()).is_err());
    }
    for v in ["", "icB3", "💥", "PPPP", "abcd=", "\n"] {
        assert!(decode_packet(v).is_err());
    }
    let max = vec![127; MAX_BYTES];
    let encoded = encode_packet(&max, [0, 1, 2, 3]).unwrap();
    assert_eq!(decode_packet(&encoded).unwrap(), max);
    assert!(encode_packet(&vec![0; MAX_BYTES + 1], [0; 4]).is_err());
}

#[test]
fn all_86_official_field_descriptors_match_utf16_and_numeric_rules() {
    let fields = descriptors().unwrap();
    assert_eq!(fields.len(), 86);
    let mut errors = 0;
    for v in fixture()["fields"].as_array().unwrap() {
        let field = fields
            .iter()
            .find(|f| f.key == v["key"].as_str().unwrap())
            .unwrap();
        let result = encode_field(
            field,
            &v["value"],
            v["collectionError"].as_bool().unwrap_or(false),
        );
        if v.get("error").is_some() {
            assert!(result.is_err(), "{}", field.key);
            errors += 1;
        } else {
            assert_eq!(hex::encode(result.unwrap()), v["hex"], "{}", field.key);
        }
    }
    assert!(errors > 10);
    let f = fields.iter().find(|f| f.key == "Ob").unwrap();
    assert_eq!(
        hex::encode(encode_field(f, &json!(4294967297_u64), false).unwrap()),
        "02c0000400000001"
    );
    assert!(encode_fields(&[("unknown", json!(1), false)]).is_err());
    let input = [
        ("bc", json!("200"), false),
        ("_move", json!([1, 2, 3, 4, 5]), false),
        ("_move", Value::Null, true),
    ];
    let packet = encode_packet(&encode_fields(&input).unwrap(), [1, 2, 3, 4]).unwrap();
    assert_eq!(
        decode_packet(&packet).unwrap(),
        encode_fields(&input).unwrap()
    );
}

#[test]
fn public_configuration_snapshot_and_explicit_input_boundaries() {
    let snapshot = fixture();
    let public = &snapshot["publicConfig"];
    let decoded: Value =
        serde_json::from_str(&decode_text(public["encoded"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(decoded, public["decoded"]);
    let original =
        Config::from_conf("YD00000558929251", "", public["encoded"].as_str().unwrap()).unwrap();
    assert_eq!(original.device_ttl_ms(), 86_400_000);
    assert_eq!(original.config_ttl_ms(), 300_000);
    assert_eq!(original.token_timeout_ms(), 2000);

    let c = config(json!({}));
    assert!(c.device_enabled());
    assert_eq!(c.device_ttl_ms(), 3_600_000);
    assert_eq!(c.config_ttl_ms(), 86_400_000);
    assert_eq!(c.token_timeout_ms(), 2000);
    assert_eq!(c.ret(), &json!(1));
    let c = config(json!({"edc":false,"dtvp":0,"ivp":0,"tto":0,"ret":0,"bl":"abc,def"}));
    assert!(!c.device_enabled());
    assert_eq!(c.device_ttl_ms(), 0);
    assert_eq!(c.token_timeout_ms(), 0);
    assert_eq!(c.ret(), &json!(0));
    assert!(c.business_suppressed("def"));
    assert!(!c.business_suppressed("de"));
    let c = config(json!({"edc":"false","dtvp":null,"ret":false}));
    assert!(c.device_enabled());
    assert_eq!(c.device_ttl_ms(), 3_600_000);
    assert_eq!(c.ret(), &json!(1));
    for value in [
        json!([]),
        json!({"pn":"another"}),
        json!({"dtvp":-1}),
        json!({"ret":{}}),
    ] {
        assert!(
            Config::from_conf(
                "YD00000558929251",
                "",
                &encode_text(&value.to_string()).unwrap()
            )
            .is_err()
        );
    }
    assert!(Config::from_conf("../bad", "", "").is_err());
}

#[test]
fn official_cache_time_and_version_predicate() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(StateStore::open(dir.path()).unwrap());
    let row = |version: &str, tid: &str, ni: &str| json!({"revision":1,"did":"synthetic","version":version,"created_ms":1000,"expires_ms":2000,"tid":tid,"ni":ni,"clock_offset_ms":0});
    for v in fixture()["caches"].as_array().unwrap() {
        db.put(
            "device",
            "watchman-device-v1",
            "YD00000558929251",
            &row(BUILD_VERSION, "tid", "ni"),
        )
        .unwrap();
        let store = DeviceStore::new(db.clone(), config(json!({})));
        assert_eq!(
            store
                .snapshot()
                .unwrap()
                .ready(store.config(), v["now"].as_u64().unwrap()),
            v["valid"].as_bool().unwrap()
        );
    }
    for v in fixture()["eligibility"].as_array().unwrap() {
        let ver = match v["version"].as_str().unwrap() {
            "current" => BUILD_VERSION,
            x => x,
        };
        let ids = v["ids"].as_bool().unwrap();
        db.put(
            "device",
            "watchman-device-v1",
            "YD00000558929251",
            &row(
                ver,
                if ids { "tid" } else { "" },
                if ids { "ni" } else { "" },
            ),
        )
        .unwrap();
        let c = Config::from_conf(
            "YD00000558929251",
            v["previous"].as_str().unwrap(),
            &encode_text(&json!({"edc":v["enabled"]}).to_string()).unwrap(),
        )
        .unwrap();
        let store = DeviceStore::new(db.clone(), c);
        let now = if v["valid"].as_bool().unwrap() {
            1500
        } else {
            2001
        };
        assert_eq!(
            store.snapshot().unwrap().ready(store.config(), now),
            v["ready"].as_bool().unwrap()
        );
    }
}

#[test]
fn device_response_slots_persist_reopen_offset_and_namespace() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(StateStore::open(dir.path()).unwrap());
    let c = config(json!({"dtvp":86400000}));
    let store = DeviceStore::new(db.clone(), c.clone());
    assert!(!store.snapshot().unwrap().ready(&c, 1000));
    assert_eq!(
        store
            .apply_response(
                0,
                ReportKind::Device,
                &json!([420, 5000, "tid-a", "did-a", null, "ni-a"]),
                1000,
                Some(5000)
            )
            .unwrap(),
        ReportUpdate::ClockCorrection
    );
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.expires_ms(), 72_001_000);
    assert_eq!(snapshot.clock_offset_ms(), 4000);
    assert!(!snapshot.ready(&c, 1000));
    assert!(snapshot.ready(&c, 1001));
    assert!(!snapshot.refresh_due(&c, 1500));
    assert!(snapshot.refresh_due(&c, 71_701_000));
    let reopened = DeviceStore::new(Arc::new(StateStore::open(dir.path()).unwrap()), c.clone());
    assert_eq!(
        reopened.snapshot().unwrap().nd_info(&c).unwrap(),
        encode_nd_info(&json!(1), "did-a", StoredValue::Text("ni-a")).unwrap()
    );
    assert_eq!(
        reopened
            .apply_response(
                1,
                ReportKind::Behavior,
                &json!([200, 999, "tid-b", "ignored", null, "ignored"]),
                2000,
                None
            )
            .unwrap(),
        ReportUpdate::Accepted
    );
    let next = reopened.snapshot().unwrap();
    assert_eq!(next.did_native(), "did-a");
    assert_eq!(next.tid_native(), "tid-b");
    assert_eq!(next.ni_native(), "ni-a");
    assert_eq!(next.clock_offset_ms(), 4000);
    assert_eq!(
        reopened
            .apply_response(2, ReportKind::Behavior, &json!([470]), 2000, None)
            .unwrap(),
        ReportUpdate::IllegalBusiness
    );
    assert_eq!(reopened.snapshot().unwrap().revision(), 2);
    assert!(
        reopened
            .apply_response(2, ReportKind::Device, &json!([470]), 2000, None)
            .is_err()
    );
    assert_eq!(
        reopened
            .apply_response(
                2,
                ReportKind::Device,
                &json!([200, null, "", "", null, ""]),
                2000,
                None
            )
            .unwrap(),
        ReportUpdate::Accepted
    );
    assert_eq!(reopened.snapshot().unwrap().did_native(), "did-a");
    let other = DeviceStore::new(
        db.clone(),
        Config::from_conf("OTHER", "", &encode_text("{}").unwrap()).unwrap(),
    );
    assert!(other.snapshot().unwrap().did_native().is_empty());
    db.clear_owner("synthetic-account").unwrap();
    assert_eq!(store.snapshot().unwrap().did_native(), "did-a");
}

#[test]
fn response_failures_are_atomic_stale_or_corrupt_state_is_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(StateStore::open(dir.path()).unwrap());
    let store = DeviceStore::new(db.clone(), config(json!({})));
    for value in [
        json!({}),
        json!(["200"]),
        json!([500]),
        json!([200, null, "valid", {}, null, "ni"]),
        json!([420, 5000, "tid", "did", null, "ni"]),
    ] {
        assert!(
            store
                .apply_response(0, ReportKind::Device, &value, 1000, None)
                .is_err()
        );
        assert_eq!(store.snapshot().unwrap().revision(), 0);
    }
    let connection = rusqlite::Connection::open(dir.path().join("core.sqlite3")).unwrap();
    connection.execute_batch("CREATE TRIGGER reject_watchman BEFORE INSERT ON state WHEN NEW.kind='watchman-device-v1' BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
    let response = json!([200, null, "tid", "did", null, "ni"]);
    assert!(
        store
            .apply_response(0, ReportKind::Device, &response, 1000, None)
            .is_err()
    );
    assert_eq!(store.snapshot().unwrap().revision(), 0);
    connection
        .execute_batch("DROP TRIGGER reject_watchman")
        .unwrap();
    store
        .apply_response(0, ReportKind::Device, &response, 1000, None)
        .unwrap();
    store.invalidate(1).unwrap();
    assert!(matches!(
        store.apply_response(1, ReportKind::Device, &response, 2000, None),
        Err(Error::StaleResponse)
    ));
    assert!(store.snapshot().unwrap().did_native().is_empty());
    db.put(
        "device",
        "watchman-device-v1",
        "YD00000558929251",
        &json!({"corrupt":true}),
    )
    .unwrap();
    assert!(store.snapshot().is_err());
    assert!(
        store
            .apply_response(0, ReportKind::Device, &response, 2000, None)
            .is_err()
    );
    assert_eq!(
        db.get("device", "watchman-device-v1", "YD00000558929251")
            .unwrap()
            .unwrap(),
        json!({"corrupt":true})
    );
}

#[test]
fn concurrent_independent_connections_cannot_replace_newer_device_response() {
    let dir = tempfile::tempdir().unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|i| {
            let db = Arc::new(StateStore::open(dir.path()).unwrap());
            let b = barrier.clone();
            std::thread::spawn(move || {
                let store = DeviceStore::new(db, config(json!({})));
                b.wait();
                store.apply_response(
                    0,
                    ReportKind::Device,
                    &json!([
                        200,
                        null,
                        format!("tid-{i}"),
                        format!("did-{i}"),
                        null,
                        "ni"
                    ]),
                    1000,
                    None,
                )
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(Error::StaleResponse)))
            .count(),
        1
    );
    let store = DeviceStore::new(
        Arc::new(StateStore::open(dir.path()).unwrap()),
        config(json!({})),
    );
    assert_eq!(store.snapshot().unwrap().revision(), 1);
}

#[test]
fn official_behavior_callbacks_normal_timeout_illegal_and_unchecked_420_retry() {
    let f = fixture();
    let nonce = f["nonce"][7]["value"].as_str().unwrap();
    for v in f["controls"].as_array().unwrap() {
        let mut attempt = BehaviorAttempt::new(
            config(json!({})),
            "synthetic-business",
            nonce.into(),
            1000,
            None,
        )
        .unwrap();
        let mut actual = Vec::new();
        for event in v["events"].as_array().unwrap() {
            let step = if event == "timeout" {
                attempt.timeout(3000, "synthetic-device").unwrap()
            } else {
                attempt
                    .response(event.as_i64(), "synthetic-device")
                    .unwrap()
            };
            match step {
                BehaviorStep::Token(t) => actual.push(json!({"token":t.encoded_native()})),
                BehaviorStep::IllegalBusiness => actual.push(json!({"error":"illegal-business"})),
                _ => {}
            }
        }
        assert_eq!(json!(actual), v["results"], "{:?}", v["events"]);
    }
    let mut a = BehaviorAttempt::new(config(json!({})), "biz", nonce.into(), 1000, None).unwrap();
    assert!(matches!(
        a.response(Some(420), "did").unwrap(),
        BehaviorStep::Resubmit
    ));
    let BehaviorStep::Token(token) = a.response(None, "did").unwrap() else {
        panic!()
    };
    assert_eq!(token.status(), TokenStatus::UncheckedRetry);
    let parsed: Value =
        serde_json::from_str(&decode_text(token.encoded_native()).unwrap()).unwrap();
    assert!(parsed.get("t").is_none());
    assert!(matches!(
        a.response(Some(200), "did").unwrap(),
        BehaviorStep::Ignored
    ));
    assert!(matches!(
        a.timeout(9000, "did").unwrap(),
        BehaviorStep::Ignored
    ));
}

#[test]
fn device_attempt_retries_once_and_native_cancellation_suppresses_late_results() {
    let mut d = DeviceAttempt::default();
    assert_eq!(
        d.response(Ok(ReportUpdate::ClockCorrection)),
        DeviceStep::Resubmit
    );
    assert_eq!(
        d.response(Err(Error::Transport("synthetic".into()))),
        DeviceStep::Failed
    );
    assert_eq!(d.response(Ok(ReportUpdate::Accepted)), DeviceStep::Ignored);
    let mut d = DeviceAttempt::default();
    d.cancel();
    assert_eq!(d.response(Ok(ReportUpdate::Accepted)), DeviceStep::Ignored);
    let nonce = request_nonce(1000, [1; 8]);
    let mut a =
        BehaviorAttempt::new(config(json!({"tto":0})), "biz", nonce.clone(), 1000, None).unwrap();
    assert!(matches!(a.timeout(999, "").unwrap(), BehaviorStep::Ignored));
    let BehaviorStep::Token(t) = a.timeout(1000, "").unwrap() else {
        panic!()
    };
    assert_eq!(t.status(), TokenStatus::TimeoutFallback);
    let mut a =
        BehaviorAttempt::new(config(json!({})), "biz", nonce.clone(), 1000, Some(1)).unwrap();
    a.cancel();
    assert!(matches!(
        a.response(Some(200), "").unwrap(),
        BehaviorStep::Ignored
    ));
    assert!(BehaviorAttempt::new(config(json!({"bl":"biz"})), "biz", nonce, 1000, None).is_err());
}
