//! Exercise exact search share authority through the native PC request gate.
use super::{fixture_registry, gate, reply, vinyl_http_with_credentials};
use cumulus_core::{
    Error,
    account::AccountSession,
    client::{
        MusicClient,
        dto::{
            HistoryRowShareChoice, HistoryRowShareOutcome, HistoryRowSharePreparation,
            SearchCategory, SearchResultQuery, SearchSongShareTarget,
        },
    },
    protocol::{
        api::Api,
        http::{ApiOrigin, WireRoute},
    },
};
use serde_json::json;
use std::{
    future::poll_fn,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::Poll,
};

#[tokio::test]
async fn share_wire_rechecks_a_revoked_visit_and_sends_only_exact_fresh_native_reads() {
    const SHARE: &str = "/api/song/enhance/privilege";
    const HOLD: &str = "/api/w/nuser/account/get";
    const ID: &str = "9007199254740993";
    let root = tempfile::tempdir().unwrap();
    let (held, release) = gate();
    let rights = json!({"code":200,"data":[{"id":ID,"sp":1}]});
    let (api, http, credentials) = vinyl_http_with_credentials(
        root.path(),
        &["MUSIC_U=synthetic-share-wire; Path=/; Secure"],
        [held, rights.clone().into(), rights.clone().into()],
    );
    let revision = api
        .search_song_share_credential_revision()
        .unwrap()
        .unwrap();
    let live = AtomicBool::new(true);
    let permission = || {
        live.load(Ordering::Acquire)
            .then_some(())
            .ok_or(Error::Cancelled)
    };
    let holding = {
        let api = api.clone();
        tokio::spawn(async move { api.request(HOLD, json!({}), false).await })
    };
    http.wait(1).await;
    let mut queued = api.request_search_song_share_privilege(ID, &revision, &permission);
    assert!(
        poll_fn(|cx| Poll::Ready(queued.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    live.store(false, Ordering::Release);
    assert!(release.send(Ok(reply(json!({"code":200})))).is_ok());
    holding.await.unwrap().unwrap();
    assert!(matches!(queued.await, Err(Error::Cancelled)));
    assert_eq!(http.calls().len(), 1, "a revoked visit must not reach HTTP");

    live.store(true, Ordering::Release);
    for _ in 0..2 {
        assert_eq!(
            api.request_search_song_share_privilege(ID, &revision, &permission)
                .await
                .unwrap(),
            rights
        );
    }
    let calls = http.calls();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        [HOLD, SHARE, SHARE]
    );
    for call in &calls[1..] {
        assert!(
            call.cookie
                .as_deref()
                .unwrap()
                .contains("MUSIC_U=synthetic-share-wire")
        );
        assert!(call.login_chain.is_none() && call.music_tu.is_none());
        let mut payload = call.payload.clone();
        assert!(
            payload
                .as_object_mut()
                .unwrap()
                .remove("header")
                .unwrap()
                .is_string()
        );
        assert_eq!(payload, json!({"ids":"[\"9007199254740993\"]"}));
    }

    // A second MUSIC_U scoped only to the privilege endpoint may never be
    // combined with the account/search cookie used to present the row.
    let share_url = ApiOrigin::Pc.request_url(WireRoute::Eapi, SHARE).unwrap();
    let old = credentials.request_credentials(&share_url).unwrap();
    credentials
        .record_response(
            &share_url,
            &["MUSIC_U=synthetic-other-user; Path=/eapi/song/enhance/privilege; Secure".into()],
            None,
            &old.revision,
        )
        .unwrap();
    assert!(matches!(
        api.search_song_share_credential_revision(),
        Err(Error::CredentialChanged)
    ));
    assert!(
        api.request_search_song_share_privilege(ID, &revision, &permission)
            .await
            .is_err()
    );
    assert_eq!(http.calls().len(), 3);
}

#[tokio::test]
async fn cancelling_a_consumed_search_offer_fences_native_dispatch_and_output() {
    const SHARE: &str = "/api/song/enhance/privilege";
    const HOLD: &str = "/api/w/nuser/account/get";
    const SEARCH: &str = "/api/search/song/list/page";
    const ID: &str = "17";

    let root = tempfile::tempdir().unwrap();
    let (holding, release_holding) = gate();
    let (sent_rights, release_rights) = gate();
    let search = json!({"code":200,"data":{"totalCount":1,"hasMore":false,"resources":[
        {"baseInfo":{"simpleSongData":{"id":ID,"name":"合成歌曲","privilege":{"st":0}}}}
    ]}});
    let rights = json!({"code":200,"data":[{"id":ID,"sp":1}]});
    let (api, http, _) = vinyl_http_with_credentials(
        root.path(),
        &["MUSIC_U=synthetic-share-cancellation; Path=/; Secure"],
        [
            search.into(),
            rights.clone().into(),
            holding,
            rights.clone().into(),
            sent_rights,
            rights.clone().into(),
            rights.clone().into(),
        ],
    );
    let registry = fixture_registry(&root.path().join("host"));
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":71}), &session.token().unwrap())
        .unwrap();
    let epoch = session.token().unwrap().generation.to_string();
    let client = MusicClient::with_session(session, registry);
    let results = client
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
    let data = serde_json::to_value(results.value).unwrap();
    let target: SearchSongShareTarget = serde_json::from_value(json!({
        "receipt": data["sections"][0]["items"][0]["songShareReceipt"], "trackId": ID,
    }))
    .unwrap();

    let prepare = || async {
        let prepared = client
            .search_song_share_prepare(&epoch, target.clone())
            .await
            .unwrap();
        let HistoryRowSharePreparation::Ready { receipt } = prepared.value else {
            panic!("expected synthetic rights to authorize share");
        };
        receipt
    };
    let first = prepare().await;
    let held = tokio::spawn(async move { api.request(HOLD, json!({}), false).await });
    http.wait(3).await;
    let mut queued = Box::pin(client.search_song_share_choose(
        &epoch,
        target.clone(),
        &first,
        HistoryRowShareChoice::Copy,
    ));
    assert!(
        poll_fn(|cx| Poll::Ready(queued.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    assert!(
        matches!(
            client
                .search_song_share_choose(
                    &epoch,
                    target.clone(),
                    &first,
                    HistoryRowShareChoice::WechatQr,
                )
                .await,
            Err(Error::StaleResponse)
        ),
        "a pending choice already consumed the offer"
    );
    client
        .search_song_share_cancel(&epoch, target.clone(), &first)
        .unwrap();
    assert!(release_holding.send(Ok(reply(json!({"code":200})))).is_ok());
    held.await.unwrap().unwrap();
    assert!(matches!(queued.await, Err(Error::StaleResponse)));
    assert_eq!(
        http.calls().len(),
        3,
        "exact cancellation before dispatch blocks the request"
    );

    let second = prepare().await;
    let mut sent = Box::pin(client.search_song_share_choose(
        &epoch,
        target.clone(),
        &second,
        HistoryRowShareChoice::WechatQr,
    ));
    tokio::select! {
        _ = http.wait(5) => {},
        result = &mut sent => panic!("choice completed before its held reply: {:?}", result.map(|reply| reply.value)),
    }
    client
        .search_song_share_cancel(&epoch, target.clone(), &second)
        .unwrap();
    assert!(release_rights.send(Ok(reply(rights))).is_ok());
    assert!(
        matches!(sent.await, Err(Error::StaleResponse)),
        "late allowed reply cannot return QR"
    );

    let third = prepare().await;
    client
        .search_song_share_cancel(&epoch, target.clone(), &second)
        .unwrap();
    let copied = client
        .search_song_share_choose(&epoch, target.clone(), &third, HistoryRowShareChoice::Copy)
        .await
        .unwrap();
    assert_eq!(
        copied.value,
        HistoryRowShareOutcome::Copy {
            url: format!("https://music.163.com/song?id={ID}")
        }
    );
    assert_eq!(
        http.calls()
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        [SEARCH, SHARE, HOLD, SHARE, SHARE, SHARE, SHARE]
    );
}

#[tokio::test]
async fn player_share_binds_song_detail_scope_and_not_the_search_scope() {
    const SHARE: &str = "/api/song/enhance/privilege";
    const DETAIL: &str = "/api/v3/song/detail";
    const HOLD: &str = "/api/w/nuser/account/get";
    const ID: &str = "9007199254740995";
    let root = tempfile::tempdir().unwrap();
    let (held, release) = gate();
    let rights = json!({"code":200,"data":[{"id":ID,"sp":1}]});
    let (api, http, credentials) = vinyl_http_with_credentials(
        root.path(),
        &["MUSIC_U=synthetic-player-share; Path=/; Secure"],
        [held, rights.clone().into(), rights.clone().into()],
    );
    let revision = api
        .player_song_share_credential_revision()
        .unwrap()
        .unwrap();
    let live = AtomicBool::new(true);
    let permission = || {
        live.load(Ordering::Acquire)
            .then_some(())
            .ok_or(Error::Cancelled)
    };
    let blocking = {
        let api = api.clone();
        tokio::spawn(async move { api.request(HOLD, json!({}), false).await })
    };
    http.wait(1).await;
    let mut pending = api.request_player_song_share_privilege(ID, &revision, &permission);
    assert!(
        poll_fn(|cx| Poll::Ready(pending.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    live.store(false, Ordering::Release);
    assert!(release.send(Ok(reply(json!({"code":200})))).is_ok());
    blocking.await.unwrap().unwrap();
    assert!(matches!(pending.await, Err(Error::Cancelled)));
    assert_eq!(http.calls().len(), 1);

    live.store(true, Ordering::Release);
    for _ in 0..2 {
        assert_eq!(
            api.request_player_song_share_privilege(ID, &revision, &permission)
                .await
                .unwrap(),
            rights
        );
    }
    let calls = http.calls();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        [HOLD, SHARE, SHARE]
    );
    for call in &calls[1..] {
        let mut wire = call.payload.clone();
        assert!(
            wire.as_object_mut()
                .unwrap()
                .remove("header")
                .unwrap()
                .is_string()
        );
        assert_eq!(wire, json!({"ids":"[\"9007199254740995\"]"}));
        assert!(
            call.cookie
                .as_deref()
                .unwrap()
                .contains("MUSIC_U=synthetic-player-share")
        );
    }

    // Different MUSIC_U only on the source metadata path invalidates Player
    // even when the permission and Search endpoints still have one Cookie.
    let detail = ApiOrigin::Pc.request_url(WireRoute::Eapi, DETAIL).unwrap();
    let old = credentials.request_credentials(&detail).unwrap();
    credentials
        .record_response(
            &detail,
            &["MUSIC_U=synthetic-other-detail; Path=/eapi/v3/song/detail; Secure".into()],
            None,
            &old.revision,
        )
        .unwrap();
    assert!(matches!(
        api.player_song_share_credential_revision(),
        Err(Error::CredentialChanged)
    ));
    assert!(
        api.search_song_share_credential_revision()
            .unwrap()
            .is_some()
    );
    assert!(
        api.request_player_song_share_privilege(ID, &revision, &permission)
            .await
            .is_err()
    );
    assert_eq!(http.calls().len(), 3);
}
