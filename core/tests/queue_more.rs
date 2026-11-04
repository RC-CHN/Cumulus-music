mod support;

use cumulus_core::{
    Error,
    account::{AccountSession, SessionToken},
    catalog::queue_more::{QueueMorePrivilege, QueueMoreRead},
};
use serde_json::json;
use std::sync::Arc;
use support::{Reply, ScriptApi, gate};

fn owner(
    replies: impl IntoIterator<Item = Reply>,
) -> (Arc<ScriptApi>, Arc<AccountSession>, SessionToken) {
    let api = Arc::new(ScriptApi::new(replies));
    let session = Arc::new(AccountSession::new(api.clone(), None));
    session
        .adopt(api.clone(), json!({"userId":7}), &session.token().unwrap())
        .unwrap();
    let token = session.token().unwrap();
    (api, session, token)
}

#[tokio::test]
async fn official_queue_order_wire_types_and_privilege_positions_are_preserved() {
    let (api, session, token) = owner([
        json!({"code":200,"data":{
            "songList":[
                {"id":101,"name":"First","dt":0,"duration":1234,"fee":1,
                    "ar":[{"id":5,"name":"Artist"}],"al":{"id":6,"name":"Album","picUrl":"http://p1.music.126.net/cover?secret=hidden#secret"},
                    "privilege":{"id":101,"st":0,"pl":999999}},
                {"id":"102","name":"Second","album":{"cover":"https://external.invalid/hidden"}},
                {"id":"103","name":"Third","artists":[{"id":"0","name":"Unknown ID"}]},
                {"id":"not part of the first three","name":"No fourth row"}
            ],
            "algMap":{"101":"synthetic-alg","102":{"private":"opaque object"},"103":"third-alg"}
        }}).into(),
        json!({"code":200,"data":[
            {"id":102,"st":0,"fee":0,"pl":128000},
            {"id":102,"st":-100,"fee":1,"payed":4,"pl":0},
            {"id":"103","st":0,"fee":0,"payed":0,"pl":320000},
            {"id":101,"st":0,"fee":0,"payed":0,"pl":320000}
        ]}).into(),
    ]);
    let ids = [
        "001".into(),
        "3".into(),
        "3".into(),
        "9007199254740993".into(),
    ];
    let songs = QueueMoreRead::new(session)
        .for_queue(&token, &ids, "3")
        .await
        .unwrap();

    let calls = api.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].path, "/api/homepage/more/rcmd/song");
    assert_eq!(
        calls[0].payload,
        json!({
            "songIds":["001","3","3","9007199254740993"],"currentPlaySongId":"3","source":"curlist_more_rcmd"
        })
    );
    assert_eq!(calls[1].path, "/api/song/enhance/privilege");
    assert_eq!(calls[1].payload, json!({"ids":"[\"101\",\"102\",\"103\"]"}));
    assert!(calls.iter().all(|call| !call.cache));

    assert_eq!(
        songs
            .iter()
            .map(|song| song.id.as_str())
            .collect::<Vec<_>>(),
        ["101", "102", "103"]
    );
    assert_eq!(songs[0].duration_ms, 1234);
    assert_eq!(songs[0].algorithm.as_deref(), Some("synthetic-alg"));
    assert_eq!(songs[0].album_id.as_deref(), Some("6"));
    assert_eq!(
        songs[0].cover_url.as_deref(),
        Some("https://p1.music.126.net/cover?param=320y320")
    );
    assert_eq!(songs[0].artists[0].name, "Artist");
    assert_eq!(songs[0].privilege, None);
    assert_eq!(songs[1].cover_url, None);
    assert_eq!(songs[1].algorithm, None);
    assert_eq!(
        songs[1].privilege,
        Some(QueueMorePrivilege {
            status: Some(-100),
            fee: Some(1),
            paid_flags: Some(4),
            play_bitrate: Some(0),
        })
    );
    assert_eq!(songs[2].artists[0].id, None);
    assert_eq!(songs[2].privilege.unwrap().play_bitrate, Some(320_000));
}

#[tokio::test]
async fn absent_seeds_skip_wire_empty_reply_keeps_official_privilege_step_and_errors_do_not_retry()
{
    let (api, session, token) = owner([
        json!({"code":200}).into(),
        json!({"code":200,"data":[]}).into(),
        json!({"code":503}).into(),
        json!({"code":200,"data":{"songList":[{"id":"invalid/file.flac"}]}}).into(),
    ]);
    let reader = QueueMoreRead::new(session);
    assert!(reader.for_queue(&token, &[], "").await.unwrap().is_empty());
    for current in ["C:\\private\\song.flac", "offline-key", "0"] {
        assert!(matches!(
            reader.for_queue(&token, &["1".into()], current).await,
            Err(Error::InvalidInput(_))
        ));
    }
    assert!(matches!(
        reader.for_queue(&token, &["bad-path".into()], "").await,
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        reader
            .for_queue(&token, &vec!["1".into(); 10_001], "")
            .await,
        Err(Error::InvalidInput(_))
    ));
    assert!(api.calls().is_empty());
    assert!(
        reader
            .for_queue(&token, &["1".into()], "")
            .await
            .unwrap()
            .is_empty()
    );
    let error = reader
        .for_queue(&token, &["1".into()], "1")
        .await
        .err()
        .unwrap();
    assert!(error.is_service_code(503));
    assert!(matches!(
        reader.for_queue(&token, &["1".into()], "1").await,
        Err(Error::Response(_))
    ));
    let calls = api.calls();
    assert_eq!(calls.len(), 4);
    assert_eq!(calls[0].payload["currentPlaySongId"], "");
    assert_eq!(calls[1].path, "/api/song/enhance/privilege");
    assert_eq!(calls[1].payload, json!({"ids":"[]"}));
    assert_eq!(calls[2].path, "/api/homepage/more/rcmd/song");
    assert_eq!(calls[3].path, "/api/homepage/more/rcmd/song");
}

#[tokio::test]
async fn switching_owner_during_either_read_never_finishes_or_reissues_old_work() {
    for switch_after_songs in [false, true] {
        let (pending, release) = gate();
        let songs = json!({"code":200,"data":{"songList":[{"id":101,"name":"First"}]}});
        let mut responses = Vec::new();
        if switch_after_songs {
            responses.push(songs.clone().into());
        }
        responses.push(pending);
        let (old_api, session, token) = owner(responses);
        let reader = QueueMoreRead::new(session.clone());
        let task = tokio::spawn(async move { reader.for_queue(&token, &["1".into()], "1").await });
        let count = if switch_after_songs { 2 } else { 1 };
        old_api.wait_for_calls(count).await;
        let new_api = Arc::new(ScriptApi::new([]));
        session
            .adopt(
                new_api.clone(),
                json!({"userId":8}),
                &session.token().unwrap(),
            )
            .unwrap();
        release
            .send(Ok(if switch_after_songs {
                json!({"code":200,"data":[{"id":101,"st":0}]})
            } else {
                songs
            }))
            .unwrap();
        assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
        assert_eq!(old_api.calls().len(), count);
        assert!(new_api.calls().is_empty());
    }
}
