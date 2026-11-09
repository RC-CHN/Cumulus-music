#[path = "download_collection/creation.rs"]
mod creation;

use super::Fixture;
use cumulus_core::{
    Error, Result,
    client::dto::{DownloadCategory, DownloadQuery, DownloadTab, PlaylistCollectSort},
    playlist::PlaylistPhase,
    protocol::api::{Api, ApiFuture, GuardedPlaylistAdd, PlaylistAddPermission},
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU8, Ordering},
};
use tokio::sync::Notify;

const LIST: &str = "/api/user/playlist/v2";
const DETAIL: &str = "/api/v6/playlist/detail";
const WRITE: &str = "/api/v1/playlist/manipulate/tracks";

#[derive(Default)]
struct Transport {
    reads: Mutex<Vec<String>>,
    writes: Mutex<Vec<Value>>,
    saved: Mutex<Vec<String>>,
    pending: Mutex<Vec<String>>,
    lost: AtomicBool,
    hold: AtomicBool,
    changed: AtomicBool,
    detail_special: AtomicU8,
    entered: Notify,
    release: Notify,
}

fn list(id: &str, owner: u8, special: u8) -> Value {
    json!({"id":id,"userId":owner,"creator":{"userId":owner,"nickname":"synthetic"},
           "name":"合成歌单","privacy":10,"specialType":special,"tags":[]})
}

impl Api for Transport {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert!(!cache);
            self.reads.lock().unwrap().push(path.into());
            match path {
                LIST => {
                    assert_eq!(
                        payload,
                        json!({"uid":"1","offset":0,"limit":10,"sortType":"default"})
                    );
                    Ok(
                        json!({"code":200,"playlist":[list("81",1,0),list("82",2,0),list("83",1,5),list("84",1,6)],
                              "more":false}),
                    )
                }
                DETAIL => {
                    assert_eq!(payload, json!({"id":"81","n":100000,"s":0}));
                    let mut row = list("81", 1, self.detail_special.load(Ordering::Acquire));
                    if self.changed.load(Ordering::Acquire) {
                        row["creator"]["userId"] = json!(2);
                    }
                    row["trackIds"] = json!(
                        self.saved
                            .lock()
                            .unwrap()
                            .iter()
                            .map(|id| json!({"id":id,"v":1}))
                            .collect::<Vec<_>>()
                    );
                    Ok(json!({"code":200,"playlist":row}))
                }
                _ => panic!("unexpected generic collection request {path}"),
            }
        })
    }

    fn download_collection_scope_revision(&self) -> Result<Option<String>> {
        Ok(Some("synthetic-download-scope".into()))
    }

    fn request_guarded_playlist_add<'a>(
        &'a self,
        write: GuardedPlaylistAdd,
        permission: &'a PlaylistAddPermission<'a>,
        dispatched: &'a AtomicBool,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let (path, payload) = write.into_request()?;
            assert_eq!(path, WRITE);
            if self.hold.swap(false, Ordering::AcqRel) {
                self.entered.notify_one();
                self.release.notified().await;
            }
            permission()?;
            dispatched.store(true, Ordering::Release);
            let ids: Vec<String> =
                serde_json::from_str(payload["trackIds"].as_str().unwrap()).unwrap();
            self.writes.lock().unwrap().push(payload);
            if self.lost.load(Ordering::Acquire) {
                self.pending.lock().unwrap().extend(ids);
                Err(Error::Transport("synthetic collection reply lost".into()))
            } else {
                self.saved.lock().unwrap().extend(ids);
                Ok(json!({"code":200}))
            }
        })
    }
}

fn connect(f: &Fixture) -> (Arc<Transport>, String) {
    let api = Arc::new(Transport::default());
    f.session
        .adopt(
            api.clone(),
            json!({"userId":1}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    (api, f.client.account().unwrap().epoch)
}

async fn choose(f: &Fixture, epoch: &str, id: &str) -> String {
    let receipt = f
        .client
        .download_collect_prepare(epoch, id)
        .unwrap()
        .value
        .receipt;
    let list = f
        .client
        .download_collect_destinations(epoch, &receipt, PlaylistCollectSort::Default, 0)
        .await
        .unwrap()
        .value;
    assert_eq!(
        list.playlists
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        ["81", "84"]
    );
    receipt
}

fn cloud_task(f: &Fixture, id: &str, variant: u8, privilege: Value) -> String {
    let mut row = f.store.get("1", "download_tasks_v1", id).unwrap().unwrap();
    let task = format!("{:032x}", 900 + u32::from(variant));
    row["id"] = json!(task);
    row["song_id"] = json!("900");
    row["origin"] = json!({"kind":"cloud","file_name":"original.flac","file_size":42});
    row["song"] = json!({"id":"900","name":"合成云盘文件","privilege":privilege});
    match variant {
        0 => {
            row["song"]["_cloud_public_id"] = json!("77");
            row["song"]["_cloud_catalog_id"] = json!("77");
        }
        1 => row["song"]["_cloud_public_id"] = json!("78"), // public marker predates catalog marker
        2 => row["song"]["_cloud_catalog_id"] = json!("79"), // private only
        3 => {}                                             // legacy with no public provenance
        4 => {
            row["song"]["_cloud_public_id"] = json!("77");
            row["song"]["_cloud_catalog_id"] = json!("79");
        }
        _ => {
            row["song"]["_cloud_public_id"] = json!("77");
            row["song"]["_cloud_catalog_id"] = json!("77");
        }
    }
    f.store.put("1", "download_tasks_v1", &task, &row).unwrap();
    task
}

#[tokio::test]
async fn downloaded_collection_uses_explicit_regular_destination_and_exact_official_add() {
    let f = Fixture::new();
    let (ordinary, _) = f.completed().await;
    let matched = cloud_task(&f, &ordinary, 0, json!({"id":77,"subp":0}));
    let legacy_public = cloud_task(&f, &ordinary, 1, json!({}));
    let private = cloud_task(&f, &ordinary, 2, json!({}));
    let legacy_private = cloud_task(&f, &ordinary, 3, json!({}));
    let inconsistent = cloud_task(&f, &ordinary, 4, json!({}));
    let denied = cloud_task(&f, &ordinary, 5, json!({"id":77,"subp":-1}));
    let truthy_zero = cloud_task(&f, &ordinary, 6, json!({"id":77,"subPriv":"0"}));
    let (api, epoch) = connect(&f);
    let view = f
        .client
        .downloads(
            &epoch,
            DownloadQuery {
                tab: DownloadTab::Completed,
                category: DownloadCategory::Song,
                ..DownloadQuery::default()
            },
        )
        .await
        .unwrap();
    for eligible in [&ordinary, &matched, &legacy_public] {
        assert!(
            view.tasks
                .iter()
                .find(|row| &row.id == eligible)
                .unwrap()
                .collection_available
        );
    }
    for blocked in [
        &private,
        &legacy_private,
        &inconsistent,
        &denied,
        &truthy_zero,
    ] {
        assert!(
            !view
                .tasks
                .iter()
                .find(|row| &row.id == blocked)
                .unwrap()
                .collection_available
        );
        assert!(f.client.download_collect_prepare(&epoch, blocked).is_err());
    }
    assert!(api.reads.lock().unwrap().is_empty());
    let receipt = f
        .client
        .download_collect_prepare(&epoch, &ordinary)
        .unwrap()
        .value
        .receipt;
    assert!(
        f.client
            .download_collect_existing(&epoch, &receipt, "81")
            .await
            .is_err()
    );
    assert!(api.reads.lock().unwrap().is_empty());
    for (task, song) in [(&ordinary, "123"), (&matched, "77"), (&legacy_public, "78")] {
        let receipt = choose(&f, &epoch, task).await;
        assert!(
            f.client
                .download_collect_existing(&epoch, &receipt, "82")
                .await
                .is_err()
        );
        assert!(
            f.client
                .download_collect_existing(&epoch, &receipt, "83")
                .await
                .is_err()
        );
        let added = f
            .client
            .download_collect_existing(&epoch, &receipt, "81")
            .await
            .unwrap()
            .value;
        assert_eq!(added.phase, PlaylistPhase::Confirmed);
        assert!(
            added
                .observed
                .unwrap()
                .track_ids
                .contains(&song.to_string())
        );
        assert_eq!(
            api.writes.lock().unwrap().last().unwrap(),
            &json!({
                "pid":"81","trackIds":serde_json::to_string(&[song]).unwrap(),"op":"add",
            })
        );
        f.client.download_collect_cancel(&epoch, &receipt).unwrap();
    }
    assert_eq!(api.writes.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn downloaded_collection_lost_reply_keeps_exact_pending_operation_and_reconciles_without_replay()
 {
    let f = Fixture::new();
    let (task, _) = f.completed().await;
    let (api, epoch) = connect(&f);
    api.lost.store(true, Ordering::Release);
    let receipt = choose(&f, &epoch, &task).await;
    let result = f
        .client
        .download_collect_existing(&epoch, &receipt, "81")
        .await
        .unwrap()
        .value;
    assert_eq!(result.phase, PlaylistPhase::Uncertain);
    let same = f
        .client
        .download_collect_existing(&epoch, &receipt, "81")
        .await
        .unwrap()
        .value;
    assert_eq!(same.operation_id, result.operation_id);
    f.client.download_collect_cancel(&epoch, &receipt).unwrap();
    assert!(
        f.client
            .download_collect_existing(&epoch, &receipt, "81")
            .await
            .is_err()
    );
    assert_eq!(
        f.client.playlist_operations(&epoch).unwrap().value[0].operation_id,
        result.operation_id
    );
    api.saved
        .lock()
        .unwrap()
        .extend(api.pending.lock().unwrap().drain(..));
    let recovered = f
        .client
        .reconcile_playlist(&epoch, &result.operation_id)
        .await
        .unwrap()
        .value;
    assert_eq!(recovered.phase, PlaylistPhase::Confirmed);
    assert_eq!(recovered.observed.unwrap().track_ids, ["123"]);
    assert_eq!(api.writes.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn downloaded_collection_rechecks_owner_and_rejects_retired_or_changed_task_at_dispatch() {
    let f = Fixture::new();
    let (task, _) = f.completed().await;
    let (api, epoch) = connect(&f);
    let receipt = choose(&f, &epoch, &task).await;
    api.changed.store(true, Ordering::Release);
    assert!(
        f.client
            .download_collect_existing(&epoch, &receipt, "81")
            .await
            .is_err()
    );
    assert!(api.writes.lock().unwrap().is_empty());
    api.changed.store(false, Ordering::Release);
    for remove_task in [false, true] {
        let receipt = choose(&f, &epoch, &task).await;
        api.hold.store(true, Ordering::Release);
        let (result, ()) = tokio::join!(
            f.client.download_collect_existing(&epoch, &receipt, "81"),
            async {
                tokio::time::timeout(std::time::Duration::from_secs(2), api.entered.notified())
                    .await
                    .unwrap();
                if remove_task {
                    f.store.delete("1", "download_tasks_v1", &task).unwrap();
                } else {
                    f.client.download_collect_cancel(&epoch, &receipt).unwrap();
                }
                api.release.notify_one();
            }
        );
        assert!(result.is_err());
        assert!(api.writes.lock().unwrap().is_empty());
    }
}

#[path = "download_collection/liked.rs"]
mod liked;

#[path = "download_collection/http.rs"]
mod http;

fn ordinary_batch_row(f: &Fixture, template: &str, n: u32, privilege: Value) -> String {
    let id = format!("{:032x}", 2000 + n);
    let mut row = f
        .store
        .get("1", "download_tasks_v1", template)
        .unwrap()
        .unwrap();
    row["id"] = json!(id);
    row["song_id"] = json!(n.to_string());
    row["song"] = json!({"id":n,"name":"合成批量下载","privilege":privilege});
    f.store.put("1", "download_tasks_v1", &id, &row).unwrap();
    id
}

async fn batch_destination(f: &Fixture, epoch: &str, receipt: &str) {
    let page = f
        .client
        .download_collect_destinations(epoch, receipt, PlaylistCollectSort::Default, 0)
        .await
        .unwrap()
        .value;
    assert_eq!(
        page.playlists.into_iter().map(|p| p.id).collect::<Vec<_>>(),
        ["81"]
    );
}

#[tokio::test]
async fn downloaded_batch_collect_filters_in_order_and_reconciles_one_lost_write() {
    let f = Fixture::new();
    let (template, _) = f.completed().await;
    let zero = ordinary_batch_row(&f, &template, 23, json!({"subPriv":0}));
    let negative = ordinary_batch_row(&f, &template, 24, json!({"subp":-1}));
    let positive = ordinary_batch_row(&f, &template, 25, json!({"subp":4}));
    let missing = ordinary_batch_row(&f, &template, 26, json!({}));
    let matched = cloud_task(&f, &template, 0, json!({"id":77,"subp":2}));
    let (api, epoch) = connect(&f);
    let plan = f
        .client
        .download_batch_collect_prepare(
            &epoch,
            vec![
                zero,
                missing.clone(),
                negative,
                matched.clone(),
                positive.clone(),
            ],
        )
        .await
        .unwrap()
        .value;
    assert_eq!(plan.accepted_task_ids, [missing, matched, positive]);
    assert!(api.reads.lock().unwrap().is_empty());
    assert!(
        f.client
            .download_collect_liked_destination(&epoch, &plan.receipt)
            .await
            .unwrap()
            .value
            .is_none()
    );
    assert!(
        f.client
            .download_collect_creation(
                &epoch,
                &plan.receipt,
                cumulus_core::client::dto::DownloadCollectCreationCommand::Create {
                    name: "合成不创建".into(),
                    private_playlist: true,
                },
            )
            .await
            .is_err()
    );
    assert!(
        f.client
            .download_collect_existing(&epoch, &plan.receipt, "81")
            .await
            .is_err()
    );
    batch_destination(&f, &epoch, &plan.receipt).await;
    for rejected in ["82", "83", "84"] {
        assert!(
            f.client
                .download_collect_existing(&epoch, &plan.receipt, rejected)
                .await
                .is_err()
        );
    }
    api.detail_special.store(6, Ordering::Release);
    assert!(
        f.client
            .download_collect_existing(&epoch, &plan.receipt, "81")
            .await
            .is_err()
    );
    assert!(api.writes.lock().unwrap().is_empty());
    api.detail_special.store(0, Ordering::Release);
    api.lost.store(true, Ordering::Release);
    let result = f
        .client
        .download_collect_existing(&epoch, &plan.receipt, "81")
        .await
        .unwrap()
        .value;
    assert_eq!(result.phase, PlaylistPhase::Uncertain);
    assert_eq!(
        f.client
            .download_collect_existing(&epoch, &plan.receipt, "81")
            .await
            .unwrap()
            .value
            .operation_id,
        result.operation_id
    );
    f.client
        .download_collect_cancel(&epoch, &plan.receipt)
        .unwrap();
    api.saved
        .lock()
        .unwrap()
        .extend(api.pending.lock().unwrap().drain(..));
    let recovered = f
        .client
        .reconcile_playlist(&epoch, &result.operation_id)
        .await
        .unwrap()
        .value;
    assert_eq!(recovered.phase, PlaylistPhase::Confirmed);
    assert_eq!(recovered.observed.unwrap().track_ids, ["26", "77", "25"]);
    assert_eq!(
        *api.writes.lock().unwrap(),
        [json!({"pid":"81","trackIds":"[\"26\",\"77\",\"25\"]","op":"add"})]
    );
}

#[tokio::test]
async fn downloaded_batch_collect_fallback_is_first_and_even_skipped_rows_fence_dispatch() {
    let f = Fixture::new();
    let (template, _) = f.completed().await;
    let first = ordinary_batch_row(&f, &template, 23, json!({"subp":null}));
    let zero = ordinary_batch_row(&f, &template, 24, json!({"subPriv":0}));
    let denied = ordinary_batch_row(&f, &template, 25, json!({"subp":-1}));
    let private = cloud_task(&f, &template, 2, json!({}));
    let (api, epoch) = connect(&f);
    for input in [
        vec![],
        vec![denied, zero.clone()],
        vec![first.clone(), first.clone()],
        vec![first.clone(), private],
        vec!["forged catalogue id".into()],
    ] {
        assert!(
            f.client
                .download_batch_collect_prepare(&epoch, input)
                .await
                .is_err()
        );
    }
    let plan = f
        .client
        .download_batch_collect_prepare(&epoch, vec![first.clone(), zero.clone()])
        .await
        .unwrap()
        .value;
    assert_eq!(plan.accepted_task_ids, [first]);
    batch_destination(&f, &epoch, &plan.receipt).await;
    api.hold.store(true, Ordering::Release);
    let (result, ()) = tokio::join!(
        f.client
            .download_collect_existing(&epoch, &plan.receipt, "81"),
        async {
            tokio::time::timeout(std::time::Duration::from_secs(2), api.entered.notified())
                .await
                .unwrap();
            assert_eq!(
                ordinary_batch_row(&f, &template, 24, json!({"subPriv":2})),
                zero
            );
            api.release.notify_one();
        }
    );
    assert!(result.is_err());
    assert!(api.writes.lock().unwrap().is_empty());
}
