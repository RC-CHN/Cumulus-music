use cumulus_core::media::tags::{
    build_metadata, decode_comment, encode_comment, official_requires_encryption, track_plan,
};
use serde_json::{Value, json};

#[test]
fn metadata_matches_official_track_mv_voice_and_special_branches() {
    let fixtures: Value = serde_json::from_str(include_str!("fixtures/media_tags.json")).unwrap();
    for case in fixtures["cases"].as_array().unwrap() {
        let actual = build_metadata(
            &case["input"],
            serde_json::from_value(case["kind"].clone()).unwrap(),
            case["special"].as_bool().unwrap(),
        );
        if case.get("error").is_some() {
            assert!(actual.is_err(), "{}", case["name"]);
        } else {
            assert_eq!(
                actual.unwrap().unwrap_or(Value::Null),
                case["expected"],
                "{}",
                case["name"]
            );
        }
    }
}

#[test]
fn native_comment_cipher_matches_independent_node_and_rejects_corruption() {
    let fixtures: Value = serde_json::from_str(include_str!("fixtures/media_tags.json")).unwrap();
    for case in fixtures["ciphers"].as_array().unwrap() {
        let encoded = encode_comment(case["plain"].as_str().unwrap()).unwrap();
        assert_eq!(encoded, case["encoded"].as_str().unwrap());
        assert_eq!(
            decode_comment(&encoded).unwrap(),
            case["plain"].as_str().unwrap()
        );
        assert!(decode_comment(&encoded[..encoded.len() - 1]).is_err());
    }
    assert!(decode_comment("plain").is_err());
}

#[test]
fn raw_song_adapter_preserves_nulls_quality_and_actual_container_choice() {
    let song = json!({"id":123,"name":" 测试 ","al":{"id":null,"name":" 专辑 "},"ar":[{"name":" 歌手 ","id":2}],"dt":null,"mv":0,"no":3,"privilege":{"flag":260}});
    let resource = json!({"type":"flac","level":"lossless","br":1234,"md5":"123456789012345678901234567890","gain":-2,"flag":4});
    let plain = track_plan(&song, &resource, false).unwrap();
    assert_eq!(plain.tags.tit2, "测试");
    assert_eq!(plain.tags.talb, " 专辑 ");
    assert_eq!(plain.tags.trck, "3");
    assert!(plain.comment_plain.contains("\"albumId\":null"));
    assert!(plain.comment_plain.contains("\"duration\":null"));
    assert!(plain.comment_plain.contains("\"bitrate\":999000"));
    assert!(
        plain
            .comment_plain
            .contains("\"mp3DocId\":123456789012345678901234567890")
    );
    assert!(!plain.comment_plain.contains("privilege"));
    let encrypted = track_plan(&song, &resource, true).unwrap();
    assert!(
        encrypted
            .comment_plain
            .contains("\"privilege\":{\"flag\":260}")
    );
    for (flag, payed, wanted) in [
        (0, 0, false),
        (4, 0, true),
        (4, 2, false),
        (4, 4, false),
        (32, 6, true),
        (36, 2, true),
    ] {
        assert_eq!(official_requires_encryption(flag, payed), wanted);
    }
}
