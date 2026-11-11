//! The read is the original song detail request. The witness is optional,
//! native-only, and never survives a cookie rotation or path mismatch.
use super::{HttpResponse, reply, vinyl_http_with_credentials};
use cumulus_core::{
    Error,
    protocol::{
        api::Api,
        http::{ApiOrigin, WireRoute},
    },
};
use serde_json::json;

#[tokio::test]
async fn player_purchase_detail_proves_exact_existing_wire_and_retires_rotating_or_split_scope() {
    const DETAIL: &str = "/api/v3/song/detail";
    const ID: &str = "9007199254740993";
    let root = tempfile::tempdir().unwrap();
    let response = json!({"code":200,"songs":[{"id":ID}],"privileges":[{"id":ID,"payed":2,"st":0,"dl":128000}]});
    let mut rotating = reply(response.clone());
    rotating.set_cookies = vec!["MUSIC_U=synthetic-purchase-rotated; Path=/; Secure".into()];
    let (api, http, credentials) = vinyl_http_with_credentials(
        root.path(),
        &["MUSIC_U=synthetic-purchase-original; Path=/; Secure"],
        [
            response.clone().into(),
            HttpResponse::Ready(Ok(rotating)),
            response.clone().into(),
        ],
    );
    let ids = [ID.to_owned()];
    let revision = api
        .player_song_purchase_credential_revision()
        .unwrap()
        .unwrap();
    assert_eq!(
        api.request_player_song_purchase_details(&ids)
            .await
            .unwrap(),
        (response.clone(), Some(revision.clone()))
    );
    assert_eq!(
        api.request_player_song_purchase_details(&ids)
            .await
            .unwrap(),
        (response.clone(), None)
    );
    let current = api
        .player_song_purchase_credential_revision()
        .unwrap()
        .unwrap();
    assert_ne!(current, revision);

    let target = ApiOrigin::Pc.request_url(WireRoute::Eapi, DETAIL).unwrap();
    credentials
        .record_response(
            &target,
            &["MUSIC_U=synthetic-purchase-split; Path=/eapi/v3/song/detail; Secure".into()],
            None,
            &current,
        )
        .unwrap();
    assert!(matches!(
        api.player_song_purchase_credential_revision(),
        Err(Error::CredentialChanged)
    ));
    assert_eq!(
        api.request_player_song_purchase_details(&ids)
            .await
            .unwrap(),
        (response, None)
    );

    let calls = http.calls();
    assert_eq!(calls.len(), 3);
    for call in &calls {
        assert_eq!(call.path, DETAIL);
        let mut payload = call.payload.clone();
        assert!(
            payload
                .as_object_mut()
                .unwrap()
                .remove("header")
                .unwrap()
                .is_string()
        );
        assert_eq!(
            payload,
            json!({"c": format!("[{{\"id\":\"{ID}\",\"v\":0}}]")})
        );
    }
    for call in &calls[..2] {
        assert!(
            call.cookie
                .as_deref()
                .unwrap()
                .contains("MUSIC_U=synthetic-purchase-original")
        );
    }
}

#[test]
fn current_song_picker_does_not_inherit_a_separate_playlist_list_cookie() {
    const COLLECT: &str = "/api/user/playlist/v2";
    let root = tempfile::tempdir().unwrap();
    let (api, http, credentials) = vinyl_http_with_credentials(
        root.path(),
        &["MUSIC_U=synthetic-player-picker; Path=/; Secure"],
        std::iter::empty::<HttpResponse>(),
    );
    let original = api
        .player_song_purchase_credential_revision()
        .unwrap()
        .unwrap();
    assert_eq!(
        api.player_song_download_picker_credential_revision()
            .unwrap(),
        Some(original.clone())
    );
    let target = ApiOrigin::Pc.request_url(WireRoute::Eapi, COLLECT).unwrap();
    credentials
        .record_response(
            &target,
            &["MUSIC_U=synthetic-list-only; Path=/eapi/user/playlist/v2; Secure".into()],
            None,
            &original,
        )
        .unwrap();
    assert!(
        api.player_song_purchase_credential_revision()
            .unwrap()
            .is_some()
    );
    assert!(matches!(
        api.player_song_download_picker_credential_revision(),
        Err(Error::CredentialChanged)
    ));
    assert!(http.calls().is_empty());
}
