use super::*;
use cumulus_core::{
    client::dto::HistorySelectionScope,
    download::service::DownloadTask,
    history::{HistoryKind, HistoryRecord, HistoryService},
};

const ANCHOR: u64 = 52;
const SELECTION: [u64; 3] = [ANCHOR, 61, 41];
const TASKS: &str = "download_tasks_v1";
const LIST: &str = "/api/user/playlist/v2";
const STAR_LIST: &str = "/api/user/playlist";
const DETAIL: &str = "/api/v6/playlist/detail";
const ADD: &str = "/api/v1/playlist/manipulate/tracks";

fn catalog_song(id: u64, subp: i64) -> Value {
    json!({
        "id": id,
        "name": "合成历史下载歌曲",
        "privilege": {"id":id,"fee":0,"st":0,"dl":320000,"subp":subp},
    })
}

fn record(session: &AccountSession, history: &HistoryService, song: Value, at: u64) {
    history
        .record_for(
            &session.token().unwrap(),
            HistoryKind::Song,
            HistoryRecord::local(HistoryKind::Song, song, at).unwrap(),
        )
        .unwrap();
}

fn save_task(root: &Path, task: &DownloadTask) {
    StateStore::open(root.join("host"))
        .unwrap()
        .put("71", TASKS, &task.id, &serde_json::to_value(task).unwrap())
        .unwrap();
}

fn seed_history(root: &Path, session: &Arc<AccountSession>) -> (HistoryService, DownloadTask) {
    let history = HistoryService::new(session.clone()).unwrap();
    let mut task = DownloadService::new(session.clone())
        .unwrap()
        .create_default(&json!(ANCHOR), DownloadOptions::default())
        .unwrap();
    // Seed the published ledger state at the boundary under test. No
    // physical media playback or file-integrity claim follows from this row.
    task.status = DownloadStatus::Complete;
    task.phase = DownloadPhase::Complete;
    task.completed_ms = Some(task.created_ms);
    let mut anchor = catalog_song(ANCHOR, 2);
    task.song = Some(anchor.clone());
    save_task(root, &task);
    anchor["download_task_id"] = json!(&task.id);
    record(session, &history, anchor, 3);
    record(session, &history, catalog_song(61, 0), 2);
    record(session, &history, catalog_song(41, 2), 1);
    (history, task)
}

fn prepare_history(session: &AccountSession, history: &HistoryService) -> C {
    let snapshot = history
        .snapshot_for(&session.token().unwrap(), HistoryKind::Song)
        .unwrap();
    C::PrepareHistory {
        target: HistorySelectionScope {
            revision: snapshot.revision,
            ids: SELECTION.map(|id| id.to_string()).to_vec(),
        },
    }
}

async fn history_receipt(
    client: &MusicClient,
    session: &AccountSession,
    epoch: &str,
    history: &HistoryService,
    expected: D,
) -> String {
    let R::Prepared {
        decision,
        receipt: Some(receipt),
        eligible_count: 2,
        omitted_count: 1,
        ..
    } = client
        .search_group_download(epoch, prepare_history(session, history))
        .await
        .unwrap()
    else {
        panic!("the completed anchor must be omitted without losing the selection");
    };
    assert!(decision == expected, "native decision differed");
    receipt
}

fn deny_media() -> HttpResponse {
    json!({"code":200,"data":{"code":403}}).into()
}

fn assert_native_order(service: &DownloadService, accepted: &[String]) {
    let ordered: Vec<_> = accepted
        .iter()
        .map(|id| service.get(id).unwrap().song_id)
        .collect();
    assert_eq!(ordered, ["61", "41"]);
    let tasks = service.list().unwrap();
    assert_eq!(tasks.len(), 3);
    let anchor = ANCHOR.to_string();
    assert_eq!(
        tasks.iter().filter(|task| task.song_id == anchor).count(),
        1
    );
}

#[tokio::test]
async fn history_download_singleton_keeps_the_original_unpurchased_fee_gate() {
    for completed_anchor in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (api, http) = vinyl_http(
            root.path(),
            &[COOKIE],
            [json!({"code":200,"ids":[ANCHOR]}).into()],
        );
        let (client, session, epoch) = client(root.path(), api);
        directory(&client, &epoch, root.path()).await;
        let mut anchor = catalog_song(ANCHOR, 2);
        // Positive dl passes the media precheck. M3085 still runs the fee
        // gate on the original resource before liking or quality selection.
        anchor["privilege"]["fee"] = json!(4);
        let (history, task, ids) = if completed_anchor {
            let (history, mut task) = seed_history(root.path(), &session);
            anchor["privilege"]["payed"] = json!(0);
            task.song = Some(anchor.clone());
            save_task(root.path(), &task);
            anchor["download_task_id"] = json!(&task.id);
            (
                history,
                Some(task.id),
                vec![ANCHOR.to_string(), "61".into()],
            )
        } else {
            (
                HistoryService::new(session.clone()).unwrap(),
                None,
                vec![ANCHOR.to_string()],
            )
        };
        record(&session, &history, anchor, 4);
        let revision = history
            .snapshot_for(&session.token().unwrap(), HistoryKind::Song)
            .unwrap()
            .revision;
        let selected_count = ids.len();
        let R::Prepared {
            decision: D::Unavailable,
            receipt: None,
            eligible_count: 0,
            omitted_count,
            ..
        } = client
            .search_group_download(
                &epoch,
                C::PrepareHistory {
                    target: HistorySelectionScope { revision, ids },
                },
            )
            .await
            .unwrap()
        else {
            panic!("unpaid fee gate must stop the singleton download");
        };
        assert_eq!(omitted_count, selected_count);
        let saved = DownloadService::new(session).unwrap().list().unwrap();
        assert!(saved.iter().map(|row| row.id.as_str()).eq(task.as_deref()));
        assert!(http.calls().is_empty());
    }
}

#[tokio::test]
async fn history_download_confirms_purchased_quality_before_reading_likes() {
    let root = tempfile::tempdir().unwrap();
    let (api, http) = vinyl_http(
        root.path(),
        &[COOKIE],
        [json!({"code":200,"ids":[ANCHOR]}).into(), deny_media()],
    );
    let (client, session, epoch) = client(root.path(), api);
    directory(&client, &epoch, root.path()).await;
    let history = HistoryService::new(session.clone()).unwrap();
    let mut song = catalog_song(ANCHOR, 2);
    song["privilege"]["fee"] = json!(4);
    song["privilege"]["payed"] = json!(2);
    song["privilege"]["maxbr"] = json!(999000);
    record(&session, &history, song, 1);
    let target = HistorySelectionScope {
        revision: history
            .snapshot_for(&session.token().unwrap(), HistoryKind::Song)
            .unwrap()
            .revision,
        ids: vec![ANCHOR.to_string()],
    };
    let prepare = || C::PrepareHistory {
        target: target.clone(),
    };
    let cancelled = decision(&client, &epoch, prepare(), D::PurchasedQualityRequired)
        .await
        .unwrap();
    assert!(matches!(
        client
            .search_group_download(&epoch, C::Cancel { receipt: cancelled })
            .await
            .unwrap(),
        R::Cancelled
    ));
    let R::Prepared {
        decision: D::PurchasedQualityRequired,
        receipt: Some(receipt),
        purchased_qualities,
        ..
    } = client
        .search_group_download(&epoch, prepare())
        .await
        .unwrap()
    else {
        panic!("purchased history must open its quality selector");
    };
    assert_eq!(
        purchased_qualities,
        [Quality::Lossless, Quality::ExHigh, Quality::Standard]
    );
    assert!(http.calls().is_empty());
    let R::Accepted { result } = client
        .search_group_download(
            &epoch,
            C::ConfirmQuality {
                receipt,
                quality: Quality::ExHigh,
            },
        )
        .await
        .unwrap()
    else {
        panic!("confirmed exact tier and liked original should admit one task");
    };
    let [task] = result.accepted.as_slice() else {
        panic!("only the selected song should be admitted");
    };
    assert_eq!(
        DownloadService::new(session)
            .unwrap()
            .get(task)
            .unwrap()
            .options
            .quality,
        Quality::ExHigh
    );
    http.wait(2).await;
    let calls = http.calls();
    assert_eq!(calls[0].path, LIKED);
    assert_eq!(calls[1].path, AUTH);
    assert_eq!(calls[1].payload["level"], "exhigh");
}

#[tokio::test]
async fn history_download_keeps_definitive_copyright_and_unavailable_purchase_decisions() {
    for expected in [D::CopyrightBlocked, D::PurchasedSelectionUnavailable] {
        let root = tempfile::tempdir().unwrap();
        let (api, http) = vinyl_http(root.path(), &[COOKIE], []);
        let (client, session, epoch) = client(root.path(), api);
        let history = HistoryService::new(session.clone()).unwrap();
        for (id, at) in SELECTION.into_iter().zip([3, 2, 1]) {
            let mut song = catalog_song(id, 2);
            if expected == D::CopyrightBlocked {
                // The original filter treats negative status as decisive,
                // even when these sparse rights lack fee and download bitrate.
                song["privilege"] = json!({"id":id,"st":-100});
            } else if id == 61 {
                // The two other rows are downloadable, but this known paid
                // survivor cannot supply a verified offer for the group.
                song["privilege"] =
                    json!({"id":id,"fee":4,"st":0,"dl":0,"maxbr":999000,"payed":2,"subp":2});
            }
            record(&session, &history, song, at);
        }
        let R::Prepared {
            decision,
            receipt: None,
            purchased_qualities,
            ..
        } = client
            .search_group_download(&epoch, prepare_history(&session, &history))
            .await
            .unwrap()
        else {
            panic!("blocked history must not issue a download receipt");
        };
        assert!(decision == expected, "native decision differed");
        assert!(purchased_qualities.is_empty());
        assert!(http.calls().is_empty());
        assert!(
            DownloadService::new(session)
                .unwrap()
                .list()
                .unwrap()
                .is_empty()
        );
    }
}

#[tokio::test]
async fn history_download_uses_completed_liked_anchor_and_fences_its_original_source() {
    let root = tempfile::tempdir().unwrap();
    let (api, http) = vinyl_http(
        root.path(),
        &[COOKIE],
        [
            json!({"code":200,"ids":[ANCHOR]}).into(),
            deny_media(),
            deny_media(),
        ],
    );
    let (client, session, epoch) = client(root.path(), api);
    directory(&client, &epoch, root.path()).await;
    let (history, complete) = seed_history(root.path(), &session);
    let service = DownloadService::new(session.clone()).unwrap();
    let first = history_receipt(&client, &session, &epoch, &history, D::Ready).await;
    // Keep the history revision and completed state, but remove the exact
    // catalogue identity backing the original offline row.
    let mut changed = complete.clone();
    changed.song = Some(catalog_song(53, 2));
    save_task(root.path(), &changed);
    assert!(matches!(
        client
            .search_group_download(&epoch, C::Start { receipt: first })
            .await,
        Err(Error::QueueChanged)
    ));
    save_task(root.path(), &complete);
    let second = history_receipt(&client, &session, &epoch, &history, D::Ready).await;
    // A newer unrelated entry changes the original snapshot, even if the
    // selected songs still appear in the same relative order.
    record(&session, &history, catalog_song(88, 2), 4);
    assert!(matches!(
        client
            .search_group_download(&epoch, C::Start { receipt: second })
            .await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(service.list().unwrap().len(), 1);
    assert_eq!(
        http.calls().len(),
        1,
        "no media or playlist write before start"
    );

    let receipt = history_receipt(&client, &session, &epoch, &history, D::Ready).await;
    let R::Accepted { result } = client
        .search_group_download(&epoch, C::Start { receipt })
        .await
        .unwrap()
    else {
        panic!("the anchor is liked; no picker is needed");
    };
    assert_native_order(&service, &result.accepted);
    http.wait(3).await;
    let calls = http.calls();
    assert_eq!(calls[0].path, LIKED);
    let mut sent: Vec<_> = calls[1..]
        .iter()
        .map(|call| {
            assert_eq!(call.path, AUTH);
            call.payload["id"].as_str().unwrap()
        })
        .collect();
    sent.sort_unstable();
    assert_eq!(sent, ["41", "61"]);
}

fn playlist() -> Value {
    json!({"id":81,"userId":71,"name":"合成历史下载目的地","specialType":0,"privacy":10,"tags":[]})
}

fn detail(tracks: &[u64]) -> HttpResponse {
    let mut row = playlist();
    row["trackIds"] = json!(tracks.iter().map(|id| json!({"id":id})).collect::<Vec<_>>());
    json!({"code":200,"playlist":row}).into()
}

fn configure_picker(root: &Path) {
    StateStore::open(root.join("config"))
        .unwrap()
        .put(
            "transport",
            "pc-mconfig-v1",
            "pc",
            &serde_json::to_value(Snapshot {
                origin: ApiOrigin::Pc,
                version: 1,
                bucket: String::new(),
                generation: 1,
                source_url: Some("https://p1.music.126.net/obj/fixture.data".into()),
                data: Some(json!({})),
            })
            .unwrap(),
        )
        .unwrap();
}

#[tokio::test]
async fn history_download_unliked_anchor_collects_original_selection_and_downloads_survivors_in_order()
 {
    let root = tempfile::tempdir().unwrap();
    let (api, http) = vinyl_http(
        root.path(),
        &[COOKIE],
        [
            json!({"code":200,"ids":[61]}).into(),
            json!({"code":200,"playlist":[playlist()],"more":false}).into(),
            json!({"code":200,"playlist":[],"more":false}).into(),
            detail(&[]),
            json!({"code":200}).into(),
            detail(&[ANCHOR, 41]),
            deny_media(),
            deny_media(),
        ],
    );
    configure_picker(root.path());
    let (client, session, epoch) = client(root.path(), api);
    directory(&client, &epoch, root.path()).await;
    let (history, _) = seed_history(root.path(), &session);
    let service = DownloadService::new(session.clone()).unwrap();
    let receipt = history_receipt(&client, &session, &epoch, &history, D::PlaylistRequired).await;
    assert!(matches!(
        client
            .search_group_download(
                &epoch,
                C::Start {
                    receipt: receipt.clone(),
                }
            )
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(
        http.calls().len(),
        1,
        "liked survivor cannot bypass the picker"
    );
    let R::Destinations { page, .. } = client
        .search_group_download(
            &epoch,
            C::Destinations {
                receipt: receipt.clone(),
                sort: PlaylistCollectSort::Default,
                page: 0,
            },
        )
        .await
        .unwrap()
    else {
        panic!("missing authorized playlist");
    };
    assert!(page.value.playlists.iter().any(|row| row.id == "81"));
    let choice = client
        .search_group_download(
            &epoch,
            C::Choose {
                receipt: receipt.clone(),
                playlist_id: "81".into(),
            },
        )
        .await
        .unwrap();
    let choice = serde_json::to_value(choice).unwrap();
    assert_eq!(choice["result"]["phase"], "confirmed");
    let continuation = choice["result"]["continuation"].as_str().unwrap().into();
    let calls = http.calls();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        [LIKED, LIST, STAR_LIST, DETAIL, ADD, DETAIL]
    );
    assert_eq!(
        service.list().unwrap().len(),
        1,
        "choice alone cannot create tasks"
    );
    let mut payload = calls[4].payload.clone();
    let header = payload.as_object_mut().unwrap().remove("header").unwrap();
    let header: Value = serde_json::from_str(header.as_str().unwrap()).unwrap();
    assert_eq!(header["appver"], "3.1.42.205594");
    assert_eq!(header["os"], "pc");
    assert_eq!(
        payload,
        json!({"pid":"81","trackIds":r#"["52","41"]"#,"op":"add"})
    );
    let R::Accepted { result } = client
        .search_group_download(
            &epoch,
            C::StartFromPlaylist {
                receipt,
                continuation,
            },
        )
        .await
        .unwrap()
    else {
        panic!("confirmed playlist must release the original download order");
    };
    assert_native_order(&service, &result.accepted);
    http.wait(8).await;
    let calls = http.calls();
    assert!(calls[6..].iter().all(|call| call.path == AUTH));
}
