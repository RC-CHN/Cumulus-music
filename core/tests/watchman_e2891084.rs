//! Byte-level conformance against the pinned 2.7.5_e2891084 vectors.
//!
//! The vectors were produced by `references/recovered-client/scripts/watchman_min_oracle.cjs`,
//! which evaluates only the official SDK's pure codec functions inside a Node vm.
//! Nothing here initializes the SDK, contacts a server or asserts server acceptance.
use cumulus_core::watchman::{
    CommonFields, MAX_BYTES, SdkBuild, decode_packet, decode_packet_for, decode_text,
    encode_packet, encode_packet_for, encode_text, public_fields_for,
};
use serde_json::Value;

const BUILD: SdkBuild = SdkBuild::E2891084;
const SOURCE_SHA256: &str = "2744116741e56fd25d543905e2dfd25cab645aa78aa2f71688ccc3d25111551a";

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/watchman-e2891084-official.json")).unwrap()
}

#[test]
fn e2891084_build_identity_matches_pinned_source() {
    let f = fixture();
    assert_eq!(f["build"], "2.7.5_e2891084");
    assert_eq!(f["source_sha256"], SOURCE_SHA256);
    assert_eq!(BUILD.version(), "2.7.5_e2891084");
    assert_eq!(BUILD.wire_version(), "e2891084");
    assert_eq!(BUILD.source_sha256(), SOURCE_SHA256);
    assert_eq!(SdkBuild::parse(BUILD.version()).unwrap(), BUILD);
    // The three builds must stay distinguishable: a version string never maps to
    // two different codec tables.
    assert_ne!(SdkBuild::parse("2.7.5_602a5ad7").unwrap(), BUILD);
    assert_ne!(SdkBuild::parse("2.7.5_af2952a4").unwrap(), BUILD);
}

#[test]
fn e2891084_text_encoding_matches_official_vectors() {
    let f = fixture();
    let vectors = f["text"].as_array().unwrap();
    assert!(!vectors.is_empty());
    for v in vectors {
        let input = v["input"].as_str().unwrap();
        assert_eq!(
            encode_text(input).unwrap(),
            v["encoded"],
            "text encode mismatch for {input:?}"
        );
        assert_eq!(decode_text(v["encoded"].as_str().unwrap()).unwrap(), input);
    }
}

#[test]
fn e2891084_packet_bytes_match_official_vectors() {
    let f = fixture();
    let vectors = f["packets"].as_array().unwrap();
    assert!(!vectors.is_empty());
    let alphabet: Vec<u8> = f["constants"]["alphabet"]
        .as_str()
        .unwrap()
        .bytes()
        .collect();
    let padding = f["constants"]["padding"].as_str().unwrap().as_bytes()[0];
    // Empty payload, single byte, sub-block, exact block, cross-block and
    // multi-block boundaries are all present in the frozen vector set.
    let mut lengths = std::collections::BTreeSet::new();
    for v in vectors {
        let payload = hex::decode(v["payload"].as_str().unwrap()).unwrap();
        lengths.insert(payload.len());
        let seed: [u8; 4] = hex::decode(v["seed"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap();
        let encoded = encode_packet_for(BUILD, &payload, seed).unwrap();
        assert_eq!(
            encoded,
            v["encoded"],
            "packet mismatch for {} bytes with seed {}",
            payload.len(),
            v["seed"]
        );
        // Every emitted character must come from this build's alphabet; only
        // padding may fall outside it.
        for b in encoded.bytes() {
            assert!(
                b == padding || alphabet.contains(&b),
                "byte {b:#04x} outside the e2891084 alphabet"
            );
        }
        assert_eq!(decode_packet_for(BUILD, &encoded).unwrap(), payload);
        // Another build's tables must not silently accept these bytes.
        assert!(decode_packet(&encoded).is_err());
        assert!(decode_packet_for(SdkBuild::B602a5ad7, &encoded).is_err());
        assert!(
            decode_packet_for(BUILD, &encode_packet(&payload, seed).unwrap()).is_err(),
            "af2952a4 output must not decode under e2891084"
        );
    }
    for boundary in [0, 1, 55, 56, 57, 63, 64, 65, 127, 128] {
        assert!(lengths.contains(&boundary), "missing boundary {boundary}");
    }
    assert!(lengths.iter().any(|n| *n > 128));
}

#[test]
fn e2891084_padding_character_is_not_a_base64_symbol() {
    let f = fixture();
    let padding = f["constants"]["padding"].as_str().unwrap();
    assert_eq!(padding, "l");
    // The padding byte is also a valid alphabet member's neighbour in ASCII, so
    // check the observable effect: an unpadded payload whose base64 ends with
    // '=' must carry this build's padding instead.
    let payload = b"a";
    let encoded = encode_packet_for(BUILD, payload, [0, 0, 0, 0]).unwrap();
    assert!(!encoded.contains('='));
}

#[test]
fn e2891084_max_payload_round_trips() {
    let max = vec![255; MAX_BYTES];
    let seed = [0, 1, 128, 255];
    let encoded = encode_packet_for(BUILD, &max, seed).unwrap();
    assert_eq!(decode_packet_for(BUILD, &encoded).unwrap(), max);
    assert!(decode_packet_for(SdkBuild::B602a5ad7, &encoded).is_err());
    assert!(
        encode_packet_for(BUILD, &vec![0; MAX_BYTES + 1], seed).is_err(),
        "oversized payload must be rejected"
    );
}

#[test]
fn e2891084_rejects_corruption() {
    let payload = vec![7_u8; 200];
    let encoded = encode_packet_for(BUILD, &payload, [1, 2, 3, 4]).unwrap();
    let mut bytes = encoded.into_bytes();
    // Flip one content character to a different alphabet member.
    let alphabet = fixture()["constants"]["alphabet"]
        .as_str()
        .unwrap()
        .to_owned();
    let position = bytes.len() / 2;
    let replacement = alphabet.bytes().find(|b| *b != bytes[position]).unwrap();
    bytes[position] = replacement;
    let corrupted = String::from_utf8(bytes).unwrap();
    assert!(
        decode_packet_for(BUILD, &corrupted).is_err(),
        "corrupted packet decoded cleanly"
    );
}

/// The official builder fills three of the five tag slots and leaves the other
/// two undefined. Which three differ per build, so this pins the selection for
/// e2891084 against the carriers the oracle recovered from the pinned bundle.
#[test]
fn e2891084_public_field_tags_match_official_carriers() {
    let f = fixture();
    let tags = &f["public_field_tags"];
    let filled = tags["filled"].as_object().unwrap();
    // Slot ids come from the shared field table: Wa 800, Xa 801, Ya 802,
    // Za 803, $a 804.
    let slot_of = |key: &str| match key {
        "Wa" => 800_u16,
        "Xa" => 801,
        "Ya" => 802,
        "Za" => 803,
        "$a" => 804,
        other => panic!("unknown tag slot {other}"),
    };
    assert_eq!(filled.len(), 3, "expected exactly three filled tag slots");

    let context = CommonFields {
        product: "YD00000558929251",
        business: None,
        nonce: None,
        tid: "",
        did: "",
        track: None,
        local_ms: 0,
        server_offset_ms: 0,
        sequence: 1,
    };
    let encoded = public_fields_for(BUILD, context, [0_u8; 31]).unwrap();
    let by_id: std::collections::BTreeMap<u16, String> = encoded
        .iter()
        .map(|field| {
            let id = u16::from_be_bytes([field[0], field[1]]);
            let length = u16::from_be_bytes([field[2], field[3]]) as usize;
            (
                id,
                String::from_utf8(field[4..4 + length].to_vec()).unwrap(),
            )
        })
        .collect();

    for (key, value) in filled {
        let id = slot_of(key);
        assert_eq!(
            by_id.get(&id),
            Some(&value.as_str().unwrap().to_owned()),
            "tag {key} (id {id}) mismatch"
        );
    }
    for key in tags["undefined"].as_array().unwrap() {
        let key = key.as_str().unwrap();
        assert!(
            !by_id.contains_key(&slot_of(key)),
            "tag {key} must stay unset for this build"
        );
    }
}
