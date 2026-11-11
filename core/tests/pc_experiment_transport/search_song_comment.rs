//! Clicked Song comment count through the real native PC request gate.
use super::{fixture_registry, gate, reply, vinyl_http_with_credentials};
use cumulus_core::{
    Error,
    account::AccountSession,
    client::{
        MusicClient,
        dto::{SearchCategory, SearchResultQuery},
    },
    protocol::{
        api::Api,
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
        api.request_search_song_comment_information(typed, revision.as_deref(), &|| Ok(()))
            .await,
        Err(Error::CredentialChanged)
    ));
    assert_eq!(http.calls().len(), 3);
}
