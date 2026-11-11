//! Dedicated Lyrics visit receipts must not borrow the Single Song authority.
use super::*;
use cumulus_core::protocol::api::Api;

const LYRICS: &str = "/api/search/resource/lyric";
const SONGS: &str = "/api/search/song/list/page";

fn lyric_page(ids: &[u64], total: usize) -> HttpResponse {
    let resources = ids
        .iter()
        .map(|&id| {
            let mut row = json!({
                "song":{
                    "id":id,"name":format!("合成歌词下载歌曲 {id}"),"dt":120000,
                    "ar":[{"id":101,"name":"合成歌手"}],"al":{"id":123,"name":"合成专辑"},
                    "privilege":{
                        "id":if id==45 {145}else{id},"fee":if id==49 {1}else{0},"st":if id==42 {-100}else{0},
                        "pl":if id==41 || id==42 {0}else{320000},
                        "dl":if id==42 {0}else{320000},"cs":id==46,
                    },
                },
                "lyrics":format!("命中的合成歌词 {id}"),
            });
            if id == 43 {
                row["privilege"] = row["song"]["privilege"].clone();
            } else if id == 44 {
                row["privilege"] = row["song"]
                    .as_object_mut()
                    .unwrap()
                    .remove("privilege")
                    .unwrap();
            } else if id == 47 {
                row["radio"] = json!({"id": 901, "name": "合成电台"});
            } else if id == 48 {
                row["isOfflineDownOnline"] = json!(true);
            }
            row
        })
        .collect::<Vec<_>>();
    json!({"code":200,"data":{"resources":resources,"totalCount":total}}).into()
}

async fn lyric_rows(client: &MusicClient, epoch: &str, keyword: &str, offset: usize) -> Value {
    let data = client
        .search_results(
            epoch,
            SearchResultQuery {
                keyword: keyword.into(),
                category: SearchCategory::Lyrics,
                offset,
                limit: 3,
                correct: true,
                channel: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(data.epoch, epoch);
    serde_json::to_value(data.value).unwrap()["sections"][0]["items"].clone()
}

fn download_target(rows: &Value, index: usize) -> Value {
    let row = &rows[index];
    let receipt = row["groupDownload"]["receipt"].as_str().unwrap();
    json!({"trackId":row["track"]["id"],"receipt":receipt})
}

fn preview(anchor: &Value) -> C {
    serde_json::from_value(json!({"kind":"preview_lyrics_anchor","anchor":anchor})).unwrap()
}

fn prepare_kind(kind: &str, anchor: &Value, rows: &[Value]) -> C {
    serde_json::from_value(json!({"kind":kind,"anchor":anchor,"rows":rows})).unwrap()
}

async fn rejected(client: &MusicClient, epoch: &str, command: C) {
    assert!(matches!(
        client.search_group_download(epoch, command).await,
        Err(Error::StaleResponse | Error::QueueChanged)
    ));
}

#[tokio::test]
async fn lyric_download_uses_the_separate_clicked_row_and_exact_ordered_survivors() {
    let root = tempfile::tempdir().unwrap();
    let no_media = json!({"code":200,"data":{"code":403}});
    let (api, http) = vinyl_http(
        root.path(),
        &[COOKIE],
        [
            search_page(),
            lyric_page(&[41, 42, 43], 6),
            lyric_page(&[44, 45, 46], 6),
            json!({"code":200,"ids":[44]}).into(),
            no_media.clone().into(),
            no_media.into(),
        ],
    );
    assert!(
        api.lyric_group_download_credential_revision()
            .unwrap()
            .is_some()
    );
    let (client, session, epoch) = client(root.path(), api);
    let songs = search(&client, &epoch).await;
    let first = lyric_rows(&client, &epoch, "合成", 0).await;
    let later = lyric_rows(&client, &epoch, "合成", 3).await;
    assert_eq!(first[0]["groupDownload"]["right"], "ordinary");
    assert_eq!(first[1]["groupDownload"]["right"], "copyright_blocked");
    assert_eq!(later[0]["groupDownload"]["right"], "ordinary");
    assert!(
        later[1].get("groupDownload").is_none(),
        "mismatched privilege ID"
    );
    assert!(later[2].get("groupDownload").is_none(), "cloud row");
    assert!(first[0].get("queueDropReceipt").is_none());
    let playback_receipt = first[2]["lyricGroupPlayback"]["receipt"].as_str().unwrap();
    assert_ne!(first[2]["groupDownload"]["receipt"], playback_receipt);
    let playback_target = json!({"trackId":first[2]["track"]["id"],"receipt":playback_receipt});
    let anchor = download_target(&later, 0);
    let rows = [
        download_target(&first, 2),
        download_target(&first, 1),
        download_target(&first, 0),
    ];
    assert!(rows.iter().all(|row| row["trackId"] != anchor["trackId"]));
    let song_target = download_target(&songs, 0);
    assert_eq!(song_target["trackId"], rows[2]["trackId"]);
    assert_ne!(song_target["receipt"], rows[2]["receipt"]);
    let mut substituted = anchor.clone();
    substituted["trackId"] = rows[2]["trackId"].clone();
    rejected(&client, &epoch, preview(&song_target)).await;
    rejected(&client, &epoch, preview(&playback_target)).await;
    rejected(&client, &epoch, preview(&substituted)).await;
    rejected(
        &client,
        &epoch,
        prepare_kind("prepare_lyrics_group", &substituted, &rows),
    )
    .await;
    rejected(&client, &epoch, prepare_kind("prepare", &rows[2], &rows)).await;
    rejected(
        &client,
        &epoch,
        prepare_kind(
            "prepare_lyrics_group",
            &anchor,
            &[rows[0].clone(), song_target],
        ),
    )
    .await;
    let preview = client
        .search_group_download(&epoch, preview(&anchor))
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(preview).unwrap(),
        json!({"kind":"lyric_menu","epoch":epoch,"show":true})
    );
    assert_eq!(
        http.calls()
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        [SONGS, LYRICS, LYRICS]
    );

    let initial_calls = http.calls();
    for (call, offset) in initial_calls[1..].iter().zip([0, 3]) {
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
            json!({"keyword":"合成","scene":"normal","limit":"3","offset":offset.to_string()})
        );
    }

    // Only the clicked (outside) Song is liked. Falling back to the first
    // selected Song here would wrongly open a playlist destination chooser.
    client.liked_ids().await.unwrap();
    directory(&client, &epoch, root.path()).await;
    let R::Prepared {
        decision: D::Ready,
        receipt: Some(receipt),
        eligible_count: 2,
        omitted_count: 1,
        ..
    } = client
        .search_group_download(&epoch, prepare_kind("prepare_lyrics_group", &anchor, &rows))
        .await
        .unwrap()
    else {
        panic!("the liked outside anchor must start its two ordered survivors directly")
    };
    assert_eq!(
        http.calls().len(),
        4,
        "prepare is local after verified likes"
    );
    assert!(
        DownloadService::new(session.clone())
            .unwrap()
            .list()
            .unwrap()
            .is_empty()
    );
    let R::Accepted { result } = client
        .search_group_download(
            &epoch,
            C::Start {
                receipt: receipt.clone(),
            },
        )
        .await
        .unwrap()
    else {
        panic!("verified lyric group was not admitted")
    };
    let tasks = DownloadService::new(session).unwrap().list().unwrap();
    assert_eq!(
        tasks
            .iter()
            .map(|task| task.song_id.as_str())
            .collect::<Vec<_>>(),
        ["43", "41"]
    );
    assert_eq!(
        tasks.iter().map(|task| task.id.clone()).collect::<Vec<_>>(),
        result.accepted
    );
    assert_eq!(result.operation_id.as_deref(), Some(receipt.as_str()));
    let R::Accepted { result: readback } = client
        .search_group_download(&epoch, C::Status { receipt })
        .await
        .unwrap()
    else {
        panic!("missing durable operation readback")
    };
    assert_eq!(readback.accepted, result.accepted);
    tokio::time::timeout(Duration::from_secs(5), http.wait(6))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if client.song_download_state(&epoch, "43").await.unwrap() == SongDownloadState::Failed
                && client.song_download_state(&epoch, "41").await.unwrap()
                    == SongDownloadState::Failed
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let calls = http.calls();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        [SONGS, LYRICS, LYRICS, LIKED, AUTH, AUTH]
    );
    let mut exact = calls[4..]
        .iter()
        .map(|call| {
            assert!(
                call.cookie
                    .as_deref()
                    .unwrap()
                    .contains("MUSIC_U=synthetic-search-group-download")
            );
            assert!(call.payload.get("trialMode").is_none());
            call.payload["id"].as_str().unwrap()
        })
        .collect::<Vec<_>>();
    exact.sort_unstable();
    assert_eq!(
        exact,
        ["41", "43"],
        "never authorize blocked or clicked-only Songs"
    );
}

#[tokio::test]
async fn a_fresh_lyrics_visit_and_conflicting_pc_cookie_cannot_lend_download_authority() {
    let root = tempfile::tempdir().unwrap();
    let (api, http, credentials) = vinyl_http_with_credentials(
        root.path(),
        &[COOKIE],
        [
            lyric_page(&[41, 43, 44], 3),
            lyric_page(&[41, 43, 44], 3),
            lyric_page(&[41, 43, 44], 3),
        ],
    );
    let (client, session, epoch) = client(root.path(), api.clone());
    let first = lyric_rows(&client, &epoch, "合成", 0).await;
    let anchor = download_target(&first, 2);
    let rows = [download_target(&first, 1), download_target(&first, 0)];
    let second = lyric_rows(&client, &epoch, "另一组词", 0).await;
    let next_anchor = download_target(&second, 2);
    assert_ne!(anchor["receipt"], next_anchor["receipt"]);
    rejected(&client, &epoch, preview(&anchor)).await;
    rejected(
        &client,
        &epoch,
        prepare_kind("prepare_lyrics_group", &next_anchor, &rows),
    )
    .await;
    let current = client
        .search_group_download(&epoch, preview(&next_anchor))
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(current).unwrap()["show"], true);

    let url = ApiOrigin::Pc
        .request_url(cumulus_core::protocol::http::WireRoute::Eapi, LYRICS)
        .unwrap();
    let old = credentials.request_credentials(&url).unwrap();
    credentials
        .record_response(
            &url,
            &[
                "MUSIC_U=synthetic-other-lyric-user; Path=/eapi/search/resource/lyric; Secure"
                    .into(),
            ],
            None,
            &old.revision,
        )
        .unwrap();
    let third = lyric_rows(&client, &epoch, "权限隔离", 0).await;
    assert_eq!(third.as_array().unwrap().len(), 3);
    assert!(
        third
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row.get("groupDownload").is_none())
    );
    assert!(matches!(
        client
            .search_group_download(&epoch, preview(&next_anchor))
            .await,
        Err(Error::CredentialChanged | Error::StaleResponse | Error::QueueChanged)
    ));
    assert!(
        DownloadService::new(session)
            .unwrap()
            .list()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        http.calls()
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        [LYRICS, LYRICS, LYRICS]
    );
}

#[tokio::test]
async fn lyric_download_never_projects_outer_voice_or_offline_rows_as_songs() {
    let root = tempfile::tempdir().unwrap();
    let (api, http) = vinyl_http(root.path(), &[COOKIE], [lyric_page(&[41, 47, 48], 3)]);
    let (client, _, epoch) = client(root.path(), api);
    let rows = lyric_rows(&client, &epoch, "合成", 0).await;
    assert_eq!(rows[0]["groupDownload"]["right"], "ordinary");
    for (index, id) in [(1, "47"), (2, "48")] {
        assert_eq!(rows[index]["track"]["id"], id);
        assert!(rows[index].get("groupDownload").is_none());
        assert!(rows[index].get("lyricGroupCollectionReceipt").is_none());
    }
    assert_eq!(http.calls().len(), 1);
}

#[tokio::test]
async fn lyric_download_single_survivor_still_requires_the_clicked_songs_fee_gate() {
    let root = tempfile::tempdir().unwrap();
    let (api, http) = vinyl_http(root.path(), &[COOKIE], [lyric_page(&[41, 42, 49], 3)]);
    let (client, session, epoch) = client(root.path(), api);
    let items = lyric_rows(&client, &epoch, "合成", 0).await;
    let rows = [download_target(&items, 0), download_target(&items, 1)];
    let anchor = download_target(&items, 2);
    assert_eq!(anchor["trackId"], "49");
    let result = client
        .search_group_download(&epoch, prepare_kind("prepare_lyrics_group", &anchor, &rows))
        .await
        .unwrap();
    assert!(matches!(
        result,
        R::Prepared {
            decision: D::Unavailable,
            receipt: None,
            eligible_count: 0,
            omitted_count: 2,
            ..
        }
    ));
    assert_eq!(http.calls().len(), 1, "no payment or media request");
    assert!(
        DownloadService::new(session)
            .unwrap()
            .list()
            .unwrap()
            .is_empty()
    );
}
