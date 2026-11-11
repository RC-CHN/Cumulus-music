//! Clicked Song and Lyrics comment counts through the native PC request gate.
use super::{fixture_registry, gate, reply, vinyl_http_with_credentials};
use cumulus_core::{
    Error,
    account::AccountSession,
    client::{
        MusicClient,
        dto::{SearchCategory, SearchResultQuery},
    },
    protocol::{
        api::{Api, SearchSongCommentSource},
        http::{ApiOrigin, WireRoute},
    },
};
use serde_json::json;
use std::{future::poll_fn, sync::Arc, task::Poll};

#[tokio::test]
async fn exact_count_wire_rechecks_the_account_after_the_gate_and_rejects_other_cookie_scopes() {
    const SEARCH: &str = "/api/search/song/list/page";
    const COUNT: &str = "/api/resource/commentInfo/list";
    const HOLD: &str = "/api/w/nuser/account/get";
    const ID: &str = "9007199254740993";
    let root = tempfile::tempdir().unwrap();
    let (held, release) = gate();
    let search = json!({"code":200,"data":{"totalCount":1,"hasMore":false,"resources":[
        {"baseInfo":{"simpleSongData":{"id":ID,"name":"合成歌曲","privilege":{"st":-100}}}}
    ]}});
    let (api, http, credentials) = vinyl_http_with_credentials(
        root.path(),
        &["MUSIC_U=synthetic-count-wire; Path=/; Secure"],
        [
            search.into(),
            json!({"code":200,"data":[{"threadId":format!("R_SO_4_{ID}"),"commentCount":36}]})
                .into(),
            held,
        ],
    );
    let registry = fixture_registry(&root.path().join("host"));
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":71}), &session.token().unwrap())
        .unwrap();
    let epoch = session.token().unwrap().generation.to_string();
    let client = MusicClient::with_session(session.clone(), registry);
    let view = client
        .search_results(
            &epoch,
            SearchResultQuery {
                keyword: "合成".into(),
                category: SearchCategory::Songs,
                offset: 0,
                limit: 6,
                correct: true,
                channel: None,
            },
        )
        .await
        .unwrap();
    let view = serde_json::to_value(view.value).unwrap();
    let row = &view["sections"][0]["items"][0];
    assert_eq!(row["songCommentMenu"]["kind"], "eligible");
    assert!(row.get("songShareReceipt").is_none());
    let receipt = row["songCommentMenu"]["receipt"].as_str().unwrap();
    assert_eq!(
        client
            .search_song_row_comment_count(&epoch, receipt)
            .await
            .unwrap()
            .value,
        Some(36)
    );
    let hold = {
        let api = api.clone();
        tokio::spawn(async move { api.request(HOLD, json!({}), false).await })
    };
    http.wait(3).await;
    let mut queued = Box::pin(client.search_song_row_comment_count(&epoch, receipt));
    assert!(
        poll_fn(|cx| Poll::Ready(queued.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    session.detach().unwrap();
    assert!(release.send(Ok(reply(json!({"code":200})))).is_ok());
    hold.await.unwrap().unwrap();
    assert!(matches!(queued.await, Err(Error::StaleResponse)));

    let calls = http.calls();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        [SEARCH, COUNT, HOLD]
    );
    assert!(
        calls[1]
            .cookie
            .as_deref()
            .unwrap()
            .contains("MUSIC_U=synthetic-count-wire")
    );
    assert!(calls[1].login_chain.is_none() && calls[1].music_tu.is_none());
    let mut payload = calls[1].payload.clone();
    assert!(
        payload
            .as_object_mut()
            .unwrap()
            .remove("header")
            .unwrap()
            .is_string()
    );
    let typed = json!({"resourceType":4,"resourceIds":format!("[\"{ID}\"]")});
    assert_eq!(
        payload,
        json!({"resourceType":"4","resourceIds":typed["resourceIds"]})
    );

    let count_url = ApiOrigin::Pc.request_url(WireRoute::Eapi, COUNT).unwrap();
    let old = credentials.request_credentials(&count_url).unwrap();
    credentials
        .record_response(
            &count_url,
            &["MUSIC_U=synthetic-other; Path=/eapi/resource/commentInfo/list; Secure".into()],
            None,
            &old.revision,
        )
        .unwrap();
    let revision = api.search_privilege_credential_revision().unwrap();
    assert!(matches!(
        api.request_search_song_comment_information(
            SearchSongCommentSource::Songs,
            typed,
            revision.as_deref(),
            &|| Ok(()),
        )
        .await,
        Err(Error::CredentialChanged)
    ));
    assert_eq!(http.calls().len(), 3);
}

#[tokio::test]
async fn lyric_row_count_uses_only_the_observed_lyric_cookie_scope_and_exact_song() {
    const SEARCH: &str = "/api/search/resource/lyric";
    const COUNT: &str = "/api/resource/commentInfo/list";
    const ID: &str = "9007199254740993";
    let root = tempfile::tempdir().unwrap();
    let lyrics = json!({"code":200,"data":{"totalCount":1,"resources":[
        {"song":{"id":ID,"name":"合成歌词命中","privilege":{"st":-100}},"lyrics":"合成行"}
    ]}});
    let (api, http, credentials) = vinyl_http_with_credentials(
        root.path(),
        &[
            "MUSIC_U=synthetic-lyric-owner; Path=/; Secure",
            "MUSIC_U=synthetic-other; Path=/eapi/search/song/list/page; Secure",
        ],
        [
            lyrics.into(),
            json!({"code":200,"data":[{"threadId":format!("R_SO_4_{ID}"),"commentCount":23}]})
                .into(),
        ],
    );
    let registry = fixture_registry(&root.path().join("host"));
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":71}), &session.token().unwrap())
        .unwrap();
    let epoch = session.token().unwrap().generation.to_string();
    let client = MusicClient::with_session(session, registry);
    let view = client
        .search_results(
            &epoch,
            SearchResultQuery {
                keyword: "合成".into(),
                category: SearchCategory::Lyrics,
                offset: 0,
                limit: 10,
                correct: true,
                channel: None,
            },
        )
        .await
        .unwrap();
    let view = serde_json::to_value(view.value).unwrap();
    let row = &view["sections"][0]["items"][0];
    assert_eq!(row["track"]["id"], ID);
    assert_eq!(row["songCommentMenu"]["kind"], "eligible");
    assert_eq!(row["excerpt"], json!(["合成行"]));
    assert_eq!(http.calls().len(), 1);
    let receipt = row["songCommentMenu"]["receipt"].as_str().unwrap();
    assert_eq!(
        client
            .search_song_row_comment_count(&epoch, receipt)
            .await
            .unwrap()
            .value,
        Some(23)
    );

    // The unrelated Songs scope cannot replace the one attached to this
    // receipt, even though both requests would otherwise hit COUNT.
    let typed = json!({"resourceType":4,"resourceIds":format!("[\"{ID}\"]")});
    let revision = api.search_privilege_credential_revision().unwrap();
    assert!(matches!(
        api.request_search_song_comment_information(
            SearchSongCommentSource::Songs,
            typed.clone(),
            revision.as_deref(),
            &|| Ok(())
        )
        .await,
        Err(Error::CredentialChanged)
    ));
    let lyric_url = ApiOrigin::Pc.request_url(WireRoute::Eapi, SEARCH).unwrap();
    let old = credentials.request_credentials(&lyric_url).unwrap();
    credentials
        .record_response(
            &lyric_url,
            &["MUSIC_U=synthetic-lyric-other; Path=/eapi/search/resource/lyric; Secure".into()],
            None,
            &old.revision,
        )
        .unwrap();
    assert!(matches!(
        client.search_song_row_comment_count(&epoch, receipt).await,
        Err(Error::QueueChanged)
    ));
    // Even with the new revision, divergent Lyrics and COUNT jars are refused.
    let revision = api.search_privilege_credential_revision().unwrap();
    assert!(matches!(
        api.request_search_song_comment_information(
            SearchSongCommentSource::Lyrics,
            typed.clone(),
            revision.as_deref(),
            &|| Ok(())
        )
        .await,
        Err(Error::CredentialChanged)
    ));

    let calls = http.calls();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        [SEARCH, COUNT]
    );
    for call in &calls {
        let cookie = call.cookie.as_deref().unwrap();
        assert!(cookie.contains("MUSIC_U=synthetic-lyric-owner"));
        assert!(!cookie.contains("synthetic-other"));
        assert!(call.login_chain.is_none() && call.music_tu.is_none());
    }
    let mut payload = calls[1].payload.clone();
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
        json!({"resourceType":"4","resourceIds":typed["resourceIds"]})
    );
}
