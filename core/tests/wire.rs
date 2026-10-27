use base64::{Engine, engine::general_purpose::STANDARD};
use cumulus_core::protocol::wire::{aegis, crypto, eapi, serialization, weapi};
use serde_json::{Value, json};

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/wire.json")).unwrap()
}
fn bytes(value: &Value) -> Vec<u8> {
    hex::decode(value.as_str().unwrap()).unwrap()
}
fn array<const N: usize>(value: &Value) -> [u8; N] {
    bytes(value).try_into().unwrap()
}

#[test]
fn frontend_form_conversion_matches_actual_official_module_48() {
    for case in fixture()["forms"].as_array().unwrap() {
        assert_eq!(
            serialization::official_form(&case["data"]).unwrap(),
            case["form"].as_str().unwrap()
        );
        assert_eq!(
            serialization::official_data(&case["data"]).unwrap(),
            case["roundtrip"]
        );
        assert_eq!(
            serialization::cache_canonical(&case["data"]).unwrap(),
            case["canonical"].as_str().unwrap()
        );
        assert_eq!(
            serialization::js_json(&case["data"]).unwrap(),
            case["stringified"].as_str().unwrap()
        );
    }
    let reversed: Value = serde_json::from_str(r#"{"10":"ten","2":"two","01":"leading"}"#).unwrap();
    assert_eq!(
        serialization::official_form(&reversed).unwrap(),
        "2=two&10=ten&01=leading"
    );
    assert_eq!(
        serialization::js_string(&json!([null, [null, 2], {}])).unwrap(),
        ",,2,[object Object]"
    );
    assert_eq!(serialization::js_string(&json!(-0.0)).unwrap(), "0");
    assert!(serialization::official_form(&json!([])).is_err());
}

#[test]
fn aes_fips_answer_and_strict_padding_checks() {
    let key: Vec<_> = (0_u8..16).collect();
    let plain = hex::decode("00112233445566778899aabbccddeeff").unwrap();
    let encrypted = crypto::encrypt_ecb(&plain, &key).unwrap();
    assert_eq!(
        hex::encode(&encrypted[..16]),
        "69c4e0d86a7b0430d8cdb78070b4c55a"
    );
    assert_eq!(encrypted.len(), 32);
    assert_eq!(crypto::decrypt_ecb(&encrypted, &key).unwrap(), plain);
    assert!(crypto::decrypt_ecb(&[], &key).is_err());
    assert!(crypto::decrypt_ecb(&encrypted[..15], &key).is_err());
    assert!(crypto::encrypt_ecb(&plain, &key[..15]).is_err());
    let mut invalid = [0_u8; 16];
    invalid[14] = 3;
    invalid[15] = 2;
    let encrypted = crypto::encrypt_ecb(&invalid, eapi::EAPI_KEY).unwrap();
    assert!(eapi::decrypt_response(&encrypted[..16]).is_err());
}

#[test]
fn eapi_matches_independent_node_encryption_and_detects_digest_tampering() {
    for case in fixture()["eapi"].as_array().unwrap() {
        let path = case["path"].as_str().unwrap();
        let encrypted = eapi::encrypt(path, &case["data"]).unwrap();
        assert_eq!(encrypted, case["eapi"].as_str().unwrap());
        assert_eq!(
            eapi::encrypt_json(path, case["body"].as_str().unwrap()).unwrap(),
            encrypted
        );
        assert_eq!(
            eapi::unwrap(&encrypted).unwrap(),
            (path.into(), case["data"].clone())
        );
    }
    let bad = format!("/api/test-36cd479b6b5-{{}}-36cd479b6b5-{}", "0".repeat(32));
    let encrypted = crypto::encrypt_ecb(bad.as_bytes(), eapi::EAPI_KEY).unwrap();
    assert!(eapi::unwrap(&hex::encode(encrypted)).is_err());
    for data in ["", "xx", "00"] {
        assert!(eapi::unwrap(data).is_err());
    }
}

#[test]
fn native_cache_serial_data2_and_response_vectors_match() {
    let fixture = fixture();
    for case in fixture["serial"].as_array().unwrap() {
        assert_eq!(
            eapi::serial_key(case["text"].as_str().unwrap()).unwrap(),
            case["key"].as_str().unwrap()
        );
    }
    for case in fixture["serial2"].as_array().unwrap() {
        assert_eq!(
            eapi::serial_data2(case["text"].as_str().unwrap().as_bytes()).unwrap(),
            case["cipher"].as_str().unwrap()
        );
    }
    assert!(eapi::serial_data2(&[]).is_err());
    assert_eq!(
        eapi::decrypt_response(&bytes(&fixture["response"]["cipher"])).unwrap(),
        fixture["response"]["plain"].as_str().unwrap().as_bytes()
    );
    let (canonical, key) = eapi::cache_envelope(&fixture["forms"][1]["data"]).unwrap();
    assert_eq!(
        canonical,
        fixture["forms"][1]["canonical"].as_str().unwrap()
    );
    assert_eq!(key, eapi::serial_key(&canonical).unwrap());
}

#[test]
fn weapi_ciphertext_and_rsa_block_match_independent_openssl_vector() {
    let fixture = fixture();
    let case = &fixture["weapi"];
    let key = case["key"].as_str().unwrap().as_bytes().try_into().unwrap();
    let envelope = weapi::encrypt_with_key(&case["data"], key).unwrap();
    assert_eq!(envelope.params, case["params"].as_str().unwrap());
    assert_eq!(envelope.encrypted_key, case["encSecKey"].as_str().unwrap());
    let fresh = weapi::encrypt(&case["data"]).unwrap();
    assert_eq!(fresh.encrypted_key.len(), 256);
    assert_ne!(fresh.encrypted_key, envelope.encrypted_key);
}

#[test]
fn aegis_mix_matches_all_frozen_lengths_and_rotation_nibbles() {
    for case in fixture()["mix"].as_array().unwrap() {
        let raw = bytes(&case["raw"]);
        let mixed = bytes(&case["mixed"]);
        assert_eq!(aegis::mix(&raw, &array(&case["mask"])), mixed);
        assert_eq!(aegis::unmix(&mixed).unwrap(), raw);
    }
    assert!(aegis::unmix(&[0; 15]).is_err());
    let mut invalid = vec![0; 16];
    invalid.extend(b"????");
    assert!(aegis::unmix(&invalid).is_err());
}

#[test]
fn complete_aegis_envelope_matches_independently_decrypted_frozen_vector() {
    let fixture = fixture();
    let case = &fixture["aegis"];
    let static_key = bytes(&case["static"]);
    let dynamic_key = bytes(&case["dynamic"]);
    let public = array(&case["public"]);
    let context = aegis::EnvelopeContext {
        static_key: &static_key,
        dynamic_key: &dynamic_key,
        public_key: &public,
        version: "v-test",
        os_name: "pc",
        sk: "synthetic-sk",
        session_id: "synthetic-session",
    };
    let randomness = aegis::Randomness {
        mask: array(&case["mask"]),
        ephemeral_private: array(&case["ephemeral"]),
        iv: array(&case["iv"]),
    };
    let body = case["body"].as_str().unwrap().as_bytes();
    assert_eq!(
        aegis::envelope_with_randomness(body, &context, &randomness).unwrap(),
        case["envelope"].as_str().unwrap()
    );
    assert_eq!(
        aegis::business_encrypt(body, &static_key, &dynamic_key, &randomness.mask).unwrap(),
        bytes(&case["business"])
    );
    assert_eq!(
        aegis::business_decrypt(&bytes(&case["business"]), &static_key, &dynamic_key).unwrap(),
        body
    );
    assert!(
        aegis::seal_dynamic(
            &dynamic_key,
            &[0; 32],
            "pc",
            "",
            &randomness.ephemeral_private,
            &randomness.iv
        )
        .is_err()
    );
}

#[test]
fn aegis_key_response_requires_the_correct_nonce_and_text_signing_key() {
    let fixture = fixture();
    let case = &fixture["signature"];
    let key = case["key"].as_str().unwrap().as_bytes();
    let nonce = case["nonce"].as_str().unwrap();
    let signature = aegis::key_signature(key, "123", nonce).unwrap();
    assert_eq!(signature, case["value"].as_str().unwrap());
    let response = json!({"timestamp":123,"signature":signature});
    assert!(aegis::verify_key_response(&response, key, nonce).unwrap());
    assert!(!aegis::verify_key_response(&response, key, "wrong").unwrap());
    assert!(
        !aegis::verify_key_response(&json!({"timestamp":123,"signature":"???"}), key, nonce)
            .unwrap()
    );
    let content = json!({"version":"synthetic","keys":[1,2]});
    let encrypted = crypto::encrypt_ecb(
        serde_json::to_string(&content).unwrap().as_bytes(),
        &[1; 32],
    )
    .unwrap();
    assert_eq!(
        aegis::decode_key_response(
            &json!({"encryptedData":STANDARD.encode(encrypted)}),
            &[1; 32]
        )
        .unwrap(),
        content
    );
}
