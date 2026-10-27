use cumulus_core::{
    Error,
    account::{AccountSession, resources::MutationPhase},
    comments::*,
    protocol::api::{Api, ApiFuture},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
#[derive(Clone, Copy)]
enum Write {
    Ok,
    LostApplied,
    LostBefore,
    Reject,
    Hold,
}
struct Server {
    calls: Mutex<Vec<(String, Value)>>,
    liked: AtomicBool,
    missing: AtomicBool,
    hold_read: AtomicBool,
    mode: Mutex<Write>,
    started: tokio::sync::Notify,
    release: tokio::sync::Notify,
}
fn item(id: &str, liked: bool) -> Value {
    json!({"commentId":id,"content":"plain <b>text</b>","time":1700000000000u64,"timeStr":"昨天","liked":liked,"likedCount":77,"user":{"userId":9,"nickname":"Reader","avatarUrl":"https://p1.music.126.net/a.jpg?secret=fixture"},"beReplied":[{"beRepliedCommentId":100,"content":"quoted","user":{"userId":8,"nickname":"Other"}}],"richContent":{"untrusted":"raw"}})
}
impl Api for Server {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert!(!cache);
            self.calls
                .lock()
                .unwrap()
                .push((path.into(), payload.clone()));
            if path == "/api/v1/comment/like" || path == "/api/v1/comment/unlike" {
                let mode = *self.mode.lock().unwrap();
                if matches!(mode, Write::Hold) {
                    self.started.notify_one();
                    self.release.notified().await;
                }
                if matches!(mode, Write::Reject) {
                    return Ok(json!({"code":403}));
                }
                if !matches!(mode, Write::LostBefore) {
                    self.liked.store(path.ends_with("/like"), Ordering::Release);
                }
                return if matches!(mode, Write::LostApplied | Write::LostBefore) {
                    Err(Error::Transport("secret fixture".into()))
                } else {
                    Ok(json!({"code":200}))
                };
            }
            if path == "/api/resource/commentInfo/list" {
                let ids: Vec<String> =
                    serde_json::from_str(payload["resourceIds"].as_str().unwrap()).unwrap();
                let prefix = if payload["resourceType"] == 1 {
                    "A_DJ_1_"
                } else {
                    "R_SO_4_"
                };
                return Ok(
                    json!({"code":200,"data":ids.into_iter().map(|id|json!({"threadId":format!("{prefix}{id}"),"commentCount":5})).collect::<Vec<_>>()}),
                );
            }
            let offset = payload["offset"].as_u64().unwrap_or(0);
            let ids = if offset == 0 {
                vec!["101", "102"]
            } else {
                vec!["103", "104"]
            };
            let rows = ids
                .into_iter()
                .take(payload["limit"].as_u64().unwrap() as usize)
                .map(|id| item(id, self.liked.load(Ordering::Acquire)))
                .collect::<Vec<_>>();
            let target = payload["commentId"].as_str().filter(|id| *id != "0");
            let current = if self.missing.load(Ordering::Acquire) {
                None
            } else {
                target.map(|id| item(id, self.liked.load(Ordering::Acquire)))
            };
            if self.missing.load(Ordering::Acquire) {
                return Ok(json!({"code":200,"comments":[],"total":0,"more":false}));
            }
            let response = if path.contains("/hotcomments/") {
                json!({"code":200,"hotComments":rows,"topComments":[],"hasMore":offset==0,"total":4})
            } else {
                json!({"code":200,"comments":rows,"hotComments":[item("105",false)],"topComments":[],"more":offset==0,"moreHot":false,"total":4,"currentComment":current})
            };
            if self.hold_read.swap(false, Ordering::AcqRel) {
                self.started.notify_one();
                self.release.notified().await;
            }
            Ok(response)
        })
    }
}
struct Fixture {
    _root: tempfile::TempDir,
    store: Arc<StateStore>,
    api: Arc<Server>,
    session: Arc<AccountSession>,
    service: Comments,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(root.path()).unwrap());
        let api = Arc::new(Server {
            calls: Mutex::new(vec![]),
            liked: AtomicBool::new(false),
            missing: AtomicBool::new(false),
            hold_read: AtomicBool::new(false),
            mode: Mutex::new(Write::Ok),
            started: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
        });
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        session
            .adopt(api.clone(), json!({"userId":1}), &session.token().unwrap())
            .unwrap();
        let service = Comments::new(session.clone())
            .unwrap()
            .with_settle_delay(Duration::ZERO);
        Self {
            _root: root,
            store,
            api,
            session,
            service,
        }
    }
    fn writes(&self) -> usize {
        self.api
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p.starts_with("/api/v1/comment/"))
            .count()
    }
}
fn resource() -> CommentResource {
    CommentResource {
        kind: ResourceKind::Track,
        id: "123".into(),
        owner: None,
    }
}

#[tokio::test]
async fn recovered_javascript_oracle_matches_all_request_defaults_and_resource_threads() {
    let oracle: Value =
        serde_json::from_str(include_str!("fixtures/comments-official.json")).unwrap();
    for row in oracle["threads"].as_array().unwrap() {
        let r = CommentResource {
            kind: serde_json::from_value(row["kind"].clone()).unwrap(),
            id: row["id"].as_str().unwrap().into(),
            owner: row["owner"].as_str().map(str::to_owned),
        };
        assert_eq!(r.thread().unwrap(), row["thread"].as_str().unwrap());
    }
    let f = Fixture::new();
    let api = CommentApi::new(f.api.clone());
    for (r, q) in [
        (resource(), CommentQuery::default()),
        (
            CommentResource {
                kind: ResourceKind::Mv,
                id: "456".into(),
                owner: None,
            },
            CommentQuery {
                comment_id: Some("900719925474099312345".into()),
                limit: 20,
                offset: 40,
                before_time: 1700000000000,
                show_inner: true,
                ..Default::default()
            },
        ),
        (
            CommentResource {
                kind: ResourceKind::Playlist,
                id: "789".into(),
                owner: None,
            },
            CommentQuery {
                sort: CommentSort::Hot,
                limit: 20,
                ..Default::default()
            },
        ),
        (
            CommentResource {
                kind: ResourceKind::Voice,
                id: "987".into(),
                owner: None,
            },
            CommentQuery {
                sort: CommentSort::Hot,
                limit: 20,
                offset: 20,
                before_time: 1700000000000,
                compare_user_location: true,
                ..Default::default()
            },
        ),
    ] {
        api.page(&r, &q).await.unwrap();
    }
    let actual = f
        .api
        .calls
        .lock()
        .unwrap()
        .iter()
        .map(|(endpoint, payload)| json!({"endpoint":endpoint,"payload":payload}))
        .collect::<Vec<_>>();
    assert_eq!(json!(actual), oracle["calls"]);
}
#[test]
fn official_thread_mapping_preserves_decimal_precision_and_refuses_path_injection() {
    for (kind, prefix) in [
        (ResourceKind::Playlist, "A_PL_0"),
        (ResourceKind::Voice, "A_DJ_1"),
        (ResourceKind::Album, "R_AL_3"),
        (ResourceKind::Track, "R_SO_4"),
        (ResourceKind::Mv, "R_MV_5"),
        (ResourceKind::Topic, "A_TO_6"),
    ] {
        let r = CommentResource {
            kind,
            id: "900719925474099312345".into(),
            owner: None,
        };
        assert_eq!(
            r.thread().unwrap(),
            format!("{prefix}_900719925474099312345")
        );
    }
    assert_eq!(
        CommentResource {
            kind: ResourceKind::Event,
            id: "123".into(),
            owner: Some("456".into())
        }
        .thread()
        .unwrap(),
        "A_EV_2_123_456"
    );
    assert_eq!(
        CommentResource {
            kind: ResourceKind::Video,
            id: "A9F0".into(),
            owner: None
        }
        .thread()
        .unwrap(),
        "R_VI_62_A9F0"
    );
    for id in ["0", "../track", "1?secret=x", "-1"] {
        assert!(
            CommentResource {
                id: id.into(),
                ..resource()
            }
            .thread()
            .is_err()
        );
    }
    assert!(
        CommentResource {
            owner: Some("1".into()),
            ..resource()
        }
        .thread()
        .is_err()
    );
}
#[tokio::test]
async fn official_default_payload_pagination_target_and_batch_information() {
    let f = Fixture::new();
    let api = CommentApi::new(f.api.clone());
    let r = resource();
    let page = api.page(&r, &CommentQuery::default()).await.unwrap();
    assert_eq!(page.comments.len(), 2);
    assert_eq!(page.hot_comments.len(), 1);
    assert_eq!(
        f.api.calls.lock().unwrap()[0].1,
        json!({"limit":60,"offset":0,"beforeTime":0,"compareUserLocation":false,"commentId":"0","composeConcert":false,"markReplied":false,"forceFlatComment":false,"showInner":false})
    );
    let next = page.next.unwrap();
    assert_eq!(next.offset, 2);
    assert_eq!(next.before_time, 1700000000000);
    assert_eq!(api.page(&r, &next).await.unwrap().comments[0].id, "103");
    let hot = api
        .page(
            &r,
            &CommentQuery {
                sort: CommentSort::Hot,
                limit: 20,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(hot.more);
    assert_eq!(hot.hot_comments.len(), 0);
    assert_eq!(hot.comments.len(), 2);
    assert_eq!(
        f.api.calls.lock().unwrap()[2].1,
        json!({"limit":20,"offset":0,"beforeTime":0,"compareUserLocation":false})
    );
    assert_eq!(api.target(&r, "101").await.unwrap().id, "101");
    assert_eq!(
        api.information(ResourceKind::Track, &["123".into(), "124".into()])
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(
        api.information(ResourceKind::Mv, &["123".into()])
            .await
            .is_err()
    );
    assert!(
        api.page(
            &r,
            &CommentQuery {
                limit: 0,
                ..Default::default()
            }
        )
        .await
        .is_err()
    );
}
#[tokio::test]
async fn likes_verify_authoritative_flag_twice_preserve_existing_state_and_ignore_global_count() {
    let f = Fixture::new();
    let t = f.session.token().unwrap();
    let r = resource();
    let liked = f.service.like_for(&t, &r, "101", true).await.unwrap();
    assert_eq!(liked.phase, MutationPhase::Confirmed);
    assert_eq!(liked.observed.unwrap().liked_count, 77);
    assert_eq!(f.writes(), 1);
    let same = f.service.like_for(&t, &r, "101", true).await.unwrap();
    assert_eq!(same.phase, MutationPhase::Unchanged);
    assert_eq!(f.writes(), 1);
    let restored = f.service.like_for(&t, &r, "101", false).await.unwrap();
    assert_eq!(restored.observed.unwrap().liked, Some(false));
    assert_eq!(f.writes(), 2);
    assert!(f.service.operations_for(&t, &r).unwrap().is_empty());
}
#[tokio::test]
async fn unknown_write_is_persisted_across_reopen_and_only_readback_can_resolve_it() {
    let f = Fixture::new();
    let t = f.session.token().unwrap();
    let r = resource();
    *f.api.mode.lock().unwrap() = Write::LostApplied;
    assert!(f.service.like_for(&t, &r, "101", true).await.is_err());
    assert_eq!(f.writes(), 1);
    let session = Arc::new(AccountSession::new(
        f.api.clone(),
        Some(Arc::new(StateStore::open(f.store.directory()).unwrap())),
    ));
    session
        .adopt(
            f.api.clone(),
            json!({"userId":1}),
            &session.token().unwrap(),
        )
        .unwrap();
    let service = Comments::new(session.clone())
        .unwrap()
        .with_settle_delay(Duration::ZERO);
    let t = session.token().unwrap();
    assert!(matches!(
        service.like_for(&t, &r, "101", false).await,
        Err(Error::MutationUncertain)
    ));
    assert_eq!(
        service.reconcile_for(&t, &r, "101").await.unwrap().phase,
        MutationPhase::Confirmed
    );
    assert_eq!(f.writes(), 1);
}
#[tokio::test]
async fn unchanged_observation_and_deleted_comment_do_not_clear_uncertain_write() {
    let f = Fixture::new();
    let t = f.session.token().unwrap();
    let r = resource();
    *f.api.mode.lock().unwrap() = Write::LostBefore;
    assert!(f.service.like_for(&t, &r, "101", true).await.is_err());
    assert!(matches!(
        f.service.reconcile_for(&t, &r, "101").await,
        Err(Error::MutationUncertain)
    ));
    f.api.missing.store(true, Ordering::Release);
    assert!(f.service.reconcile_for(&t, &r, "101").await.is_err());
    assert_eq!(f.service.operations_for(&t, &r).unwrap().len(), 1);
    assert_eq!(f.writes(), 1);
}
#[tokio::test]
async fn definitive_rejection_does_not_become_success_or_retry_automatically() {
    let f = Fixture::new();
    let t = f.session.token().unwrap();
    *f.api.mode.lock().unwrap() = Write::Reject;
    assert!(
        f.service
            .like_for(&t, &resource(), "101", true)
            .await
            .err()
            .expect("definite service rejection")
            .is_service_code(403)
    );
    assert_eq!(f.writes(), 1);
    assert!(
        f.service
            .operations_for(&t, &resource())
            .unwrap()
            .is_empty()
    );
}
#[tokio::test]
async fn cancelled_and_old_account_writes_keep_responsibility_and_revoke_new_account_reads() {
    let f = Fixture::new();
    let t = f.session.token().unwrap();
    *f.api.mode.lock().unwrap() = Write::Hold;
    let session = f.session.clone();
    let old = t.clone();
    let task = tokio::spawn(async move {
        Comments::new(session)
            .unwrap()
            .like_for(&old, &resource(), "101", true)
            .await
    });
    f.api.started.notified().await;
    assert!(matches!(
        f.service.like_for(&t, &resource(), "101", true).await,
        Err(Error::MutationBusy)
    ));
    task.abort();
    let _ = task.await;
    assert_eq!(
        f.service.operations_for(&t, &resource()).unwrap()[0].phase,
        MutationPhase::Sending
    );
    f.session
        .adopt(f.api.clone(), json!({"userId":2}), &t)
        .unwrap();
    assert!(
        f.service
            .cached_for(&t, &resource(), &CommentQuery::default())
            .is_err()
    );
    assert!(
        f.service
            .operations_for(&f.session.token().unwrap(), &resource())
            .unwrap()
            .is_empty()
    );
    assert_eq!(f.writes(), 1);
}
#[tokio::test]
async fn durable_intent_precedes_write_and_cache_is_account_and_query_scoped() {
    let f = Fixture::new();
    let t = f.session.token().unwrap();
    let r = resource();
    let q = CommentQuery::default();
    f.service.page_for(&t, &r, &q).await.unwrap();
    assert!(f.service.cached_for(&t, &r, &q).unwrap().is_some());
    let owner = t.uid.as_deref().unwrap();
    let (cache_key, cache) = f
        .store
        .values(owner, "comment-pages-v1")
        .unwrap()
        .pop_first()
        .unwrap();
    for field in ["resource", "query"] {
        let mut broken = cache.clone();
        if field == "resource" {
            broken["resource"]["id"] = json!("999");
        } else {
            broken["query"]["offset"] = json!(20);
        }
        f.store
            .put(owner, "comment-pages-v1", &cache_key, &broken)
            .unwrap();
        assert!(f.service.cached_for(&t, &r, &q).is_err());
    }
    f.store
        .put(owner, "comment-pages-v1", &cache_key, &cache)
        .unwrap();
    assert!(
        f.service
            .cached_for(
                &t,
                &r,
                &CommentQuery {
                    offset: 20,
                    ..q.clone()
                }
            )
            .unwrap()
            .is_none()
    );
    let db = rusqlite::Connection::open(f.store.directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_comment_intent BEFORE INSERT ON state WHEN NEW.kind='comment-likes-v1' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(f.service.like_for(&t, &r, "101", true).await.is_err());
    assert_eq!(f.writes(), 0);
    f.session
        .adopt(f.api.clone(), json!({"userId":2}), &t)
        .unwrap();
    assert!(
        f.service
            .cached_for(&f.session.token().unwrap(), &r, &q)
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn desktop_use_case_whitelists_comment_fields_and_reports_cached_pending_writes() {
    let f = Fixture::new();
    let registry = cumulus_core::account::profiles::ProfileRegistry::open(
        &f._root.path().join("profiles"),
        cumulus_core::protocol::http::ApiOrigin::Pc,
    )
    .unwrap();
    let client = cumulus_core::client::MusicClient::with_session(f.session.clone(), registry);
    let epoch = f.session.token().unwrap().generation.to_string();
    let result = client
        .comments_page(&epoch, resource(), CommentQuery::default(), true)
        .await
        .unwrap();
    let value = serde_json::to_value(result).unwrap();
    let text = value.to_string();
    assert!(!text.contains("secret") && !text.contains("untrusted") && !text.contains("userId"));
    assert_eq!(
        value["value"]["comments"][0]["author"]["avatarUrl"],
        "https://p1.music.126.net/a.jpg?param=320y320"
    );
    *f.api.mode.lock().unwrap() = Write::LostApplied;
    assert!(
        client
            .comments_like(&epoch, resource(), "101", true)
            .await
            .is_err()
    );
    let page = client
        .comments_page(&epoch, resource(), CommentQuery::default(), false)
        .await
        .unwrap()
        .value;
    assert!(page.cached);
    assert_eq!(page.operations.len(), 1);
    assert_eq!(
        client
            .comments_reconcile(&epoch, resource(), "101")
            .await
            .unwrap()
            .value
            .phase,
        MutationPhase::Confirmed
    );
    let before = f.api.calls.lock().unwrap().len();
    for result in [
        client.comments_like("0", resource(), "101", false).await,
        client.comments_reconcile("0", resource(), "101").await,
    ] {
        assert!(matches!(result, Err(Error::StaleResponse)));
    }
    assert_eq!(f.api.calls.lock().unwrap().len(), before);
}

#[tokio::test]
async fn late_write_response_is_recorded_under_original_account_without_reaching_new_one() {
    let f = Fixture::new();
    let old = f.session.token().unwrap();
    *f.api.mode.lock().unwrap() = Write::Hold;
    let session = f.session.clone();
    let token = old.clone();
    let pending = tokio::spawn(async move {
        Comments::new(session)
            .unwrap()
            .like_for(&token, &resource(), "101", true)
            .await
    });
    f.api.started.notified().await;
    f.session
        .adopt(f.api.clone(), json!({"userId":2}), &old)
        .unwrap();
    f.api.release.notify_one();
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    assert!(
        f.service
            .operations_for(&f.session.token().unwrap(), &resource())
            .unwrap()
            .is_empty()
    );
    let persisted = f
        .store
        .get("1", "comment-likes-v1", "R_SO_4_123:101")
        .unwrap()
        .unwrap();
    assert_eq!(persisted["phase"], "uncertain");
    assert_eq!(f.writes(), 1);
}

#[tokio::test]
async fn confirmed_like_updates_cached_pages_and_rejects_older_inflight_cache_writes() {
    let f = Fixture::new();
    let t = f.session.token().unwrap();
    let r = resource();
    let q = CommentQuery::default();
    f.service.page_for(&t, &r, &q).await.unwrap();
    f.api.hold_read.store(true, Ordering::Release);
    let session = f.session.clone();
    let old = t.clone();
    let page = tokio::spawn(async move {
        Comments::new(session)
            .unwrap()
            .page_for(&old, &resource(), &CommentQuery::default())
            .await
    });
    f.api.started.notified().await;
    f.service.like_for(&t, &r, "101", true).await.unwrap();
    f.api.release.notify_one();
    assert!(matches!(page.await.unwrap(), Err(Error::QueueChanged)));
    assert_eq!(
        f.service.cached_for(&t, &r, &q).unwrap().unwrap().comments[0].liked,
        Some(true)
    );
    // A genuinely newer server read (e.g. another device unliked) is still authoritative.
    f.api.liked.store(false, Ordering::Release);
    f.service.page_for(&t, &r, &q).await.unwrap();
    assert_eq!(
        f.service.cached_for(&t, &r, &q).unwrap().unwrap().comments[0].liked,
        Some(false)
    );
}

#[tokio::test]
async fn failed_cache_confirmation_keeps_write_journal_pending_until_read_only_recovery() {
    let f = Fixture::new();
    let t = f.session.token().unwrap();
    let r = resource();
    let q = CommentQuery::default();
    f.service.page_for(&t, &r, &q).await.unwrap();
    let db = rusqlite::Connection::open(f.store.directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_comment_cache BEFORE INSERT ON state WHEN NEW.kind='comment-pages-v1' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(f.service.like_for(&t, &r, "101", true).await.is_err());
    assert_eq!(f.writes(), 1);
    assert_eq!(f.service.operations_for(&t, &r).unwrap().len(), 1);
    db.execute_batch("DROP TRIGGER reject_comment_cache")
        .unwrap();
    f.service.reconcile_for(&t, &r, "101").await.unwrap();
    assert_eq!(f.writes(), 1);
    assert_eq!(
        f.service.cached_for(&t, &r, &q).unwrap().unwrap().comments[0].liked,
        Some(true)
    );
}
