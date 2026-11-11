use super::*;
use serde_json::json;

#[test]
fn metadata_uses_only_the_matched_public_detail_and_album_privilege() {
    let public = json!({"id":"17","name":"合成","fee":4});
    for (fee, expected) in [(0, false), (4, true), (5, true), (8, false)] {
        let mut projected = json!({"id":"17"});
        mark_metadata(
            &mut projected,
            &public,
            Some(&json!({"id":17,"st":0,"fee":fee,"sp":0})),
        );
        assert_eq!(projected, json!({"id":"17",METADATA:expected}));
    }

    let known = json!({"id":17,"st":0});
    let candidate = |song: &Value, rights: Option<&Value>| {
        let mut projected = json!({"id":"17"});
        mark_metadata(&mut projected, song, rights);
        projected.get(METADATA).is_some()
    };
    assert!(candidate(&public, Some(&known)));
    for rights in [
        None,
        Some(json!({"id":18,"st":0})),
        Some(json!({"id":17,"st":-100,"fee":4})),
        Some(json!({"id":17,"st":"invalid"})),
        Some(json!({"id":17,"st":0,"cs":1})),
    ] {
        assert!(!candidate(&public, rights.as_ref()));
    }
    for raw in [
        json!({"id":"017"}),
        json!({"id":17,"onlineTrackId":18}),
        json!({"id":17,"t":1}),
        json!({"id":17,"songType":"1"}),
        json!({"id":17,"cloudSongId":"17"}),
        json!({"id":17,"programId":"17"}),
        json!({"id":17,"privilege":{"status":-200}}),
        json!({"id":17,"privilege":{"cs":1}}),
        json!({"id":17,"noCopyrightRcmd":{"type":2,"mvInfo":{"id":81}}}),
    ] {
        assert!(!candidate(&raw, Some(&known)), "{raw}");
    }
}

#[test]
fn target_rejects_renderer_supplied_song_and_source_fields() {
    for extra in [
        json!({"trackId":"17"}),
        json!({"url":"https://music.163.com/song?id=17"}),
        json!({"receipt":"other-source"}),
    ] {
        let mut target = json!({"playId":"play","entryId":"entry"});
        target
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        assert!(serde_json::from_value::<PlayerSongShareTarget>(target).is_err());
    }
}
