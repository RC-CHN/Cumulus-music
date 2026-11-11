use super::*;
use cumulus_core::{
    client::dto::{DownloadCollectCreationCommand, HistoryCollectTarget},
    history::{HistoryKind, HistoryRecord, HistoryService, HistorySnapshot},
};
use std::time::Duration;

fn record(f: &Fixture, id: u64, played_at: u64, privilege: Option<Value>) {
    let mut song = json!({"id":id,"name":"合成历史歌曲"});
    if let Some(privilege) = privilege {
        song["privilege"] = privilege;
    }
    let kind = HistoryKind::Song;
    HistoryService::new(f.session.clone())
        .unwrap()
        .record_for(
            &f.session.token().unwrap(),
            kind,
            HistoryRecord::local(kind, song, played_at).unwrap(),
        )
        .unwrap();
}

fn snapshot(f: &Fixture) -> HistorySnapshot {
    HistoryService::new(f.session.clone())
        .unwrap()
        .snapshot_for(&f.session.token().unwrap(), HistoryKind::Song)
        .unwrap()
}

fn all(f: &Fixture) -> HistoryCollectTarget {
    HistoryCollectTarget::All {
        revision: snapshot(f).revision,
    }
}

async fn destinations(f: &Fixture, epoch: &str, receipt: &str) {
    let page = f
        .client
        .download_collect_destinations(epoch, receipt, PlaylistCollectSort::Default, 0)
        .await
        .unwrap();
    assert!(page.value.playlists.iter().any(|row| row.id == "81"));
    assert!(!page.value.playlists.iter().any(|row| row.id == "82"));
}

#[tokio::test]
async fn history_collect_accepts_only_current_owned_catalogue_download_provenance() {
    let f = Fixture::new();
    let (song_task, _) = f.completed().await;
    let cloud = cloud_task(&f, &song_task, 2, json!({"subp":1}));
    let (api, epoch) = connect(&f);
    let history = HistoryService::new(f.session.clone()).unwrap();
    let token = f.session.token().unwrap();
    for (id, task, played_at) in [(123, &song_task, 1), (900, &cloud, 2)] {
        history.record_for(&token, HistoryKind::Song, HistoryRecord::local(
            HistoryKind::Song, json!({"id":id,"name":"合成下载记录","download_task_id":task,"privilege":{"subp":1}}), played_at,
        ).unwrap()).unwrap();
    }
    let target = all(&f);
    let plan = f
        .client
        .history_collect_prepare(&epoch, target.clone())
        .await
        .unwrap()
        .value;
    assert_eq!(plan.selected_count, 2);
    assert_eq!(plan.song_ids, ["123"]);
    let receipt = plan.receipt.unwrap();
    destinations(&f, &epoch, &receipt).await;
    f.store
        .delete("1", "download_tasks_v1", &song_task)
        .unwrap();
    assert!(matches!(
        f.client
            .download_collect_existing(&epoch, &receipt, "81")
            .await,
        Err(Error::QueueChanged)
    ));
    let revised = f
        .client
        .history_collect_prepare(&epoch, target)
        .await
        .unwrap()
        .value;
    assert!(revised.song_ids.is_empty() && revised.receipt.is_none());
    assert!(api.writes.lock().unwrap().is_empty());
}

fn add_body(ids: &[String]) -> Value {
    json!({"pid":"81","trackIds":serde_json::to_string(ids).unwrap(),"op":"add"})
}

#[tokio::test]
async fn history_collect_header_dispatches_more_than_100_in_one_reversed_payload() {
    let f = Fixture::new();
    let (api, epoch) = connect(&f);
    for id in 1..=125 {
        record(&f, id, id, Some(json!({"subp":2})));
    }
    let expected: Vec<_> = (1..=125).map(|id| id.to_string()).collect();
    let history = snapshot(&f);
    assert!(
        history
            .entries
            .iter()
            .map(|row| &row.id)
            .eq(expected.iter().rev())
    );
    let plan = f
        .client
        .history_collect_prepare(
            &epoch,
            HistoryCollectTarget::All {
                revision: history.revision,
            },
        )
        .await
        .unwrap()
        .value;
    assert_eq!(plan.selected_count, 125);
    assert_eq!(plan.song_ids, expected);
    assert!(api.reads.lock().unwrap().is_empty());
    let receipt = plan.receipt.unwrap();
    destinations(&f, &epoch, &receipt).await;
    let result = f
        .client
        .download_collect_existing(&epoch, &receipt, "81")
        .await
        .unwrap()
        .value;
    assert_eq!(result.phase, PlaylistPhase::Confirmed);
    assert_eq!(result.observed.unwrap().track_ids, expected);
    assert_eq!(*api.writes.lock().unwrap(), [add_body(&expected)]);
}

#[tokio::test]
async fn history_collect_selected_preserves_order_and_checks_the_advertised_destination() {
    let f = Fixture::new();
    let (api, epoch) = connect(&f);
    record(&f, 30, 3, Some(json!({"subp":0})));
    record(&f, 20, 2, Some(json!({})));
    record(&f, 10, 1, Some(json!({"subPriv":2})));
    let revision = snapshot(&f).revision;
    let selected = |ids: [&str; 3]| HistoryCollectTarget::Selected {
        revision: revision.clone(),
        ids: ids.map(str::to_owned).to_vec(),
    };
    assert!(matches!(
        f.client
            .history_collect_prepare(&epoch, selected(["20", "30", "10"]))
            .await,
        Err(Error::QueueChanged)
    ));
    let plan = f
        .client
        .history_collect_prepare(&epoch, selected(["30", "20", "10"]))
        .await
        .unwrap()
        .value;
    assert_eq!(plan.selected_count, 3);
    assert_eq!(plan.song_ids, ["20", "10"]);
    assert!(api.reads.lock().unwrap().is_empty());
    let receipt = plan.receipt.unwrap();
    destinations(&f, &epoch, &receipt).await;
    let reads = api.reads.lock().unwrap().len();
    assert!(matches!(
        f.client
            .download_collect_existing(&epoch, &receipt, "82")
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(api.reads.lock().unwrap().len(), reads);
    assert!(api.writes.lock().unwrap().is_empty());
    let result = f
        .client
        .download_collect_existing(&epoch, &receipt, "81")
        .await
        .unwrap()
        .value;
    assert_eq!(result.phase, PlaylistPhase::Confirmed);
    assert_eq!(*api.writes.lock().unwrap(), [add_body(&plan.song_ids)]);
}

#[tokio::test]
async fn history_collect_cancellation_and_snapshot_changes_stop_submission() {
    let f = Fixture::new();
    let (api, epoch) = connect(&f);
    record(&f, 10, 1, Some(json!({})));
    let target = all(&f);
    let prepare = || f.client.history_collect_prepare(&epoch, target.clone());
    let receipt = prepare().await.unwrap().value.receipt.unwrap();
    destinations(&f, &epoch, &receipt).await;
    f.client.download_collect_cancel(&epoch, &receipt).unwrap();
    let reads = api.reads.lock().unwrap().len();
    assert!(matches!(
        f.client
            .download_collect_existing(&epoch, &receipt, "81")
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        f.client
            .download_collect_creation(
                &epoch,
                &receipt,
                DownloadCollectCreationCommand::Create {
                    name: "合成历史歌单".into(),
                    private_playlist: true,
                },
            )
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(api.reads.lock().unwrap().len(), reads);
    let receipt = prepare().await.unwrap().value.receipt.unwrap();
    destinations(&f, &epoch, &receipt).await;
    api.hold.store(true, Ordering::Release);
    let (result, ()) = tokio::join!(
        f.client.download_collect_existing(&epoch, &receipt, "81"),
        async {
            tokio::time::timeout(Duration::from_secs(2), api.entered.notified())
                .await
                .unwrap();
            // A newly played, ineligible song still changes the full snapshot.
            record(&f, 20, 2, Some(json!({"subp":-1})));
            api.release.notify_one();
        }
    );
    assert!(matches!(result, Err(Error::QueueChanged)));
    assert!(api.writes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn history_collect_treats_an_existing_song_502_as_definitive_without_replay() {
    let f = Fixture::new();
    let (api, epoch) = connect(&f);
    record(&f, 10, 1, Some(json!({})));
    *api.saved.lock().unwrap() = vec!["10".into()];
    api.duplicate.store(true, Ordering::Release);
    let plan = f
        .client
        .history_collect_prepare(&epoch, all(&f))
        .await
        .unwrap()
        .value;
    let receipt = plan.receipt.unwrap();
    destinations(&f, &epoch, &receipt).await;
    for _ in 0..2 {
        let result = f
            .client
            .download_collect_existing(&epoch, &receipt, "81")
            .await
            .unwrap()
            .value;
        assert_eq!(result.phase, PlaylistPhase::Rejected);
        assert_eq!(result.service_codes.get("main"), Some(&502));
    }
    assert_eq!(*api.writes.lock().unwrap(), [add_body(&plan.song_ids)]);
}

#[tokio::test]
async fn history_collect_reads_sparse_rights_and_rejects_a_late_reply_to_stale_history() {
    let f = Fixture::new();
    let (api, epoch) = connect(&f);
    record(&f, 10, 1, None);
    record(&f, 20, 2, Some(json!({"subp":0})));
    *api.privileges.lock().unwrap() = vec![json!({"id":"10","subp":2})];
    let target = all(&f);
    let plan = f
        .client
        .history_collect_prepare(&epoch, target.clone())
        .await
        .unwrap()
        .value;
    assert_eq!(plan.song_ids, ["10", "20"]);
    let receipt = plan.receipt.unwrap();
    api.hold_privileges.store(true, Ordering::Release);
    let (late, ()) = tokio::join!(f.client.history_collect_prepare(&epoch, target), async {
        tokio::time::timeout(Duration::from_secs(2), api.entered.notified())
            .await
            .unwrap();
        record(&f, 30, 3, Some(json!({})));
        api.release.notify_one();
    });
    assert!(matches!(late, Err(Error::QueueChanged)));
    assert!(matches!(
        f.client
            .download_collect_destinations(&epoch, &receipt, PlaylistCollectSort::Default, 0)
            .await,
        Err(Error::StaleResponse | Error::QueueChanged)
    ));
    let request = json!({"ids":"[\"10\"]"});
    assert_eq!(
        *api.privilege_requests.lock().unwrap(),
        [request.clone(), request]
    );
    assert!(api.writes.lock().unwrap().is_empty());
}
