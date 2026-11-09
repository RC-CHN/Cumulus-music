use super::*;
use cumulus_core::protocol::api::SystemLikedRead;
use std::sync::atomic::AtomicU8;

#[derive(Default)]
struct Liked {
    state: AtomicU8,
    changed_credentials: AtomicBool,
    reads: Mutex<Vec<(String, Value)>>,
    writes: Mutex<Vec<Value>>,
}

fn star(id: &str, tracks: Value) -> Value {
    let mut row = list(id, 1, 5);
    row["name"] = json!("原生系统歌单改了名字");
    row["trackIds"] = tracks;
    row
}

impl Api for Liked {
    fn request<'a>(&'a self, path: &'a str, args: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert!(!cache);
            if path == LIST {
                assert_eq!(
                    args,
                    json!({"uid":"1","offset":0,"limit":10,"sortType":"default"})
                );
                return Ok(json!({"code":200,"more":false,"playlist":[list("81",1,0)]}));
            }
            panic!("unbound download liked request: {path}")
        })
    }

    fn download_collection_scope_revision(&self) -> Result<Option<String>> {
        Ok(Some("download".into()))
    }

    fn download_system_liked_scope_revision(&self) -> Result<Option<String>> {
        Ok(Some(
            if self.changed_credentials.load(Ordering::Acquire) {
                "changed"
            } else {
                "download"
            }
            .into(),
        ))
    }

    fn request_download_system_liked<'a>(
        &'a self,
        read: SystemLikedRead,
        revision: &'a str,
        permission: &'a PlaylistAddPermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert_eq!(
                Some(revision),
                self.download_system_liked_scope_revision()?.as_deref()
            );
            permission()?;
            let (path, args) = read.into_request();
            self.reads.lock().unwrap().push((path.into(), args.clone()));
            let state = self.state.load(Ordering::Acquire);
            let id = if state == 3 { "86" } else { "83" };
            let tracks = if matches!(state, 1 | 2) {
                json!([{"id":123,"v":1},{"id":17,"v":1}])
            } else {
                json!([{"id":17,"v":1}])
            };
            match path {
                "/api/user/playlist" => {
                    assert_eq!(args, json!({"uid":"1","offset":0,"limit":1000}));
                    let mut decoy = list("82", 1, 0);
                    decoy["name"] = json!("我喜欢的音乐");
                    let rows = if state == 4 {
                        json!([decoy])
                    } else {
                        json!([decoy, star(id, tracks)])
                    };
                    Ok(json!({"code":200,"more":false,"playlist":rows}))
                }
                DETAIL => {
                    assert_eq!(args, json!({"id":id,"n":100000,"s":0}));
                    Ok(json!({"code":200,"playlist":star(id, tracks)}))
                }
                "/api/song/like/get" => {
                    assert_eq!(args, json!({"uid":"1"}));
                    Ok(
                        json!({"code":200,"ids":if state == 2 { json!([17,123]) } else { json!([17]) }}),
                    )
                }
                _ => panic!("unknown system liked route"),
            }
        })
    }

    fn request_guarded_playlist_add<'a>(
        &'a self,
        write: GuardedPlaylistAdd,
        permission: &'a PlaylistAddPermission<'a>,
        dispatched: &'a AtomicBool,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let (path, args) = write.into_request()?;
            assert_eq!(path, WRITE);
            permission()?;
            dispatched.store(true, Ordering::Release);
            self.writes.lock().unwrap().push(args);
            assert_eq!(self.state.swap(1, Ordering::AcqRel), 0);
            Err(Error::Transport("synthetic add response lost".into()))
        })
    }
}

async fn setup() -> (Fixture, Arc<Liked>, String, String) {
    let f = Fixture::new();
    let (task, _) = f.completed().await;
    let api = Arc::new(Liked::default());
    f.session
        .adopt(
            api.clone(),
            json!({"userId":1}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    let epoch = f.client.account().unwrap().epoch;
    (f, api, epoch, task)
}

#[tokio::test]
async fn downloaded_collection_system_liked_preserves_pending_and_recovers_both_reads_without_replay()
 {
    let (f, api, epoch, task) = setup().await;
    f.store
        .put("1", "resources", "liked", &json!(["17"]))
        .unwrap();
    let visit = f
        .client
        .download_collect_prepare(&epoch, &task)
        .unwrap()
        .value
        .receipt;
    let target = f
        .client
        .download_collect_liked_destination(&epoch, &visit)
        .await
        .unwrap()
        .value
        .unwrap();
    for raw in ["83", "82"] {
        assert!(
            f.client
                .download_collect_liked(&epoch, &visit, raw, false)
                .await
                .is_err()
        );
    }
    assert!(
        f.client
            .download_collect_liked(&epoch, &visit, &target.receipt, true)
            .await
            .is_err()
    );
    assert!(api.writes.lock().unwrap().is_empty());
    let pending = f
        .client
        .download_collect_liked(&epoch, &visit, &target.receipt, false)
        .await
        .unwrap()
        .value;
    assert_eq!(pending.phase, PlaylistPhase::Uncertain);
    assert!(pending.observed.is_none());
    assert_eq!(pending.verified_liked_track_ids, Some(vec![]));
    assert_eq!(
        f.store.get("1", "resources", "liked").unwrap().unwrap(),
        json!(["17"])
    );
    let pending_list = f.client.playlist_operations(&epoch).unwrap().value;
    assert_eq!(pending_list[0].operation_id, pending.operation_id);
    let same = f
        .client
        .download_collect_liked(&epoch, &visit, &target.receipt, false)
        .await
        .unwrap()
        .value;
    assert_eq!(same.operation_id, pending.operation_id);
    let still_pending = f
        .client
        .download_collect_liked(&epoch, &visit, &target.receipt, true)
        .await
        .unwrap()
        .value;
    assert_eq!(still_pending.phase, PlaylistPhase::Uncertain);
    f.client.download_collect_cancel(&epoch, &visit).unwrap();
    api.state.store(2, Ordering::Release);
    assert!(
        f.client
            .download_collect_liked(&epoch, &visit, &target.receipt, true)
            .await
            .is_err()
    );
    // Generic journal recovery after close retains download credentials and
    // dual observation; this transport deliberately has no VoiceSong lane.
    let done = f
        .client
        .reconcile_playlist(&epoch, &pending.operation_id)
        .await
        .unwrap()
        .value;
    assert_eq!(done.phase, PlaylistPhase::Confirmed);
    assert_eq!(done.verified_liked_track_ids, Some(vec!["123".into()]));
    assert_eq!(done.observed.unwrap().track_ids, ["123", "17"]);
    assert_eq!(
        f.store.get("1", "resources", "liked").unwrap().unwrap(),
        json!(["17", "123"])
    );
    let visit = f
        .client
        .download_collect_prepare(&epoch, &task)
        .unwrap()
        .value
        .receipt;
    let target = f
        .client
        .download_collect_liked_destination(&epoch, &visit)
        .await
        .unwrap()
        .value
        .unwrap();
    let duplicate = f
        .client
        .download_collect_liked(&epoch, &visit, &target.receipt, false)
        .await
        .unwrap()
        .value;
    assert_eq!(duplicate.phase, PlaylistPhase::Unchanged);
    assert_eq!(duplicate.verified_liked_track_ids, Some(vec!["123".into()]));
    assert_eq!(
        *api.writes.lock().unwrap(),
        [json!({"pid":"83","trackIds":"[\"123\"]","op":"add"})]
    );
    let reads = api.reads.lock().unwrap();
    assert!(
        reads
            .iter()
            .filter(|(path, _)| path == "/api/song/like/get")
            .count()
            >= 5
    );
}

#[tokio::test]
async fn downloaded_collection_system_liked_rejects_name_guess_target_replacement_and_account_change()
 {
    let (f, api, epoch, task) = setup().await;
    let visit = f
        .client
        .download_collect_prepare(&epoch, &task)
        .unwrap()
        .value
        .receipt;
    api.state.store(4, Ordering::Release);
    assert!(
        f.client
            .download_collect_liked_destination(&epoch, &visit)
            .await
            .unwrap()
            .value
            .is_none()
    );
    api.state.store(0, Ordering::Release);
    let target = f
        .client
        .download_collect_liked_destination(&epoch, &visit)
        .await
        .unwrap()
        .value
        .unwrap();
    let newer = f
        .client
        .download_collect_liked_destination(&epoch, &visit)
        .await
        .unwrap()
        .value
        .unwrap();
    assert!(
        f.client
            .download_collect_liked(&epoch, &visit, &target.receipt, false)
            .await
            .is_err()
    );
    api.state.store(3, Ordering::Release);
    assert!(
        f.client
            .download_collect_liked(&epoch, &visit, &newer.receipt, false)
            .await
            .is_err()
    );
    assert!(api.writes.lock().unwrap().is_empty());
    api.state.store(0, Ordering::Release);
    let target = f
        .client
        .download_collect_liked_destination(&epoch, &visit)
        .await
        .unwrap()
        .value
        .unwrap();
    api.changed_credentials.store(true, Ordering::Release);
    assert!(
        f.client
            .download_collect_liked(&epoch, &visit, &target.receipt, false)
            .await
            .is_err()
    );
    api.changed_credentials.store(false, Ordering::Release);
    f.session
        .adopt(
            Arc::new(Liked::default()),
            json!({"userId":2}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(
        f.client
            .download_collect_liked(&epoch, &visit, &target.receipt, false)
            .await
            .is_err()
    );
    assert!(api.writes.lock().unwrap().is_empty());
}
