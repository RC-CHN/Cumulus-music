use super::*;
use crate::{
    identity::client_sign,
    protocol::wire::eapi,
    song_recognition::{
        request::{Event, RequestContext, Scheduler, SessionUuid},
        seal_compressed_packet,
    },
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeFixture {
    signs: Vec<SignCase>,
    device_ids: Vec<DeviceCase>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignCase {
    parts: [String; 4],
    result: String,
    utf8_hex: String,
    fast_writer_quoted: String,
}

#[derive(Deserialize)]
struct DeviceCase {
    sid: Option<String>,
    adapters: Vec<Vec<u8>>,
    result: String,
}

fn fixture() -> NativeFixture {
    serde_json::from_str(include_str!(
        "../../../../tests/fixtures/song-recognition-native-identity.json"
    ))
    .unwrap()
}

fn sign(parts: &[String; 4]) -> NativeClientSign {
    let wide = parts
        .each_ref()
        .map(|s| s.encode_utf16().collect::<Vec<_>>());
    NativeClientSign::from_wide(wide.each_ref().map(Vec::as_slice)).unwrap()
}

#[test]
fn recognition_sign_and_fastwriter_quote_match_native_vectors() {
    for case in fixture().signs {
        let native = sign(&case.parts);
        assert_eq!(&*native.0, case.result);
        assert_eq!(hex::encode(native.0.as_bytes()), case.utf8_hex);
        assert_eq!(
            serde_json::to_string(&native.0).unwrap(),
            case.fast_writer_quoted
        );
    }

    let case = fixture().signs.remove(0);
    let normal = client_sign(case.parts.each_ref().map(String::as_str), 15).unwrap();
    assert_ne!(normal, case.result);
    assert!(normal.contains("@DISK-A@BOARD-B@"));
}

#[test]
fn system_helper_device_id_matches_native_digest_and_first_six_byte_adapter() {
    for case in fixture().device_ids {
        let adapters = case.adapters.iter().map(Vec::as_slice).collect::<Vec<_>>();
        let value = NativeDeviceId::from_cache_miss(case.sid.as_deref(), &adapters).unwrap();
        assert_eq!(&*value.0, case.result);
    }

    let existing = NativeDeviceId::from_observed_cache("0012aBcdef").unwrap();
    assert_eq!(&*existing.0, "0012aBcdef");
    for rejected in ["", "HTTP://not-an-id", "00\n12", "测试"] {
        assert!(NativeDeviceId::from_observed_cache(rejected).is_err());
    }
    assert!(NativeDeviceId::from_cache_miss(Some("synthetic-device"), &[]).is_err());
    assert!(NativeDeviceId::from_cache_miss(None, &[&[0; 9]]).is_err());
}

#[test]
fn native_values_enter_only_the_explicit_version_pinned_fixture_header() {
    let mut vector = fixture();
    let sign_case = vector.signs.pop().unwrap();
    let device_case = vector.device_ids.pop().unwrap();
    let context = RequestContext::from_native_identity(
        sign(&sign_case.parts),
        NativeDeviceId::from_observed_cache(&device_case.result).unwrap(),
    );
    let mut scheduler = Scheduler::default();
    let generation = scheduler
        .start(
            0,
            SessionUuid::from_host("A6F10324-8B20-46D4-B182-36A22E73211C").unwrap(),
        )
        .unwrap();
    assert!(matches!(
        scheduler.push_pcm(generation, 3000, &[0; 24_000]).unwrap(),
        Event::Ready(_)
    ));
    let Event::Ready(prefix) = scheduler.next_prefix(generation, 3000).unwrap() else {
        panic!("fixture did not prepare prefix");
    };
    let sealed = seal_compressed_packet(&[1]).unwrap().unwrap();
    let Event::Ready(prepared) = scheduler
        .prepare(prefix.receipt(), 3000, &sealed, &context, 7)
        .unwrap()
    else {
        panic!("fixture did not prepare request");
    };
    let encrypted = prepared.encrypted_form().strip_prefix("params=").unwrap();
    let (path, body) = eapi::unwrap(encrypted).unwrap();
    assert_eq!(path, "/api/music/audio/match");
    let cipher = hex::decode(encrypted).unwrap();
    let plain = crate::protocol::wire::crypto::decrypt_ecb(&cipher, eapi::EAPI_KEY).unwrap();
    let signed = std::str::from_utf8(&plain).unwrap();
    assert!(signed.contains(&format!("\"clientSign\":{}", sign_case.fast_writer_quoted)));
    assert_eq!(
        body["header"],
        serde_json::json!({
            "appver": "3.1.42.205594",
            "os": "pc",
            "deviceId": device_case.result,
            "clientSign": sign_case.result,
            "requestId": "7"
        })
    );

    let persistent = sign_case.parts[3].encode_utf16().collect::<Vec<_>>();
    assert!(NativeClientSign::from_wide([&[0xd800], &[], &[], &persistent]).is_err());
    assert!(NativeClientSign::from_wide([&[0], &[], &[], &persistent]).is_err());
    assert!(NativeClientSign::from_wide([&[], &[], &[], &[]]).is_err());
}
