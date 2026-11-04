use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{CommentReportCommand as Command, CommentReportReason as Reason},
    },
    comments::{CommentResource, ResourceKind},
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

const TRACK: &str = "R_SO_4_123";
const ALBUM: &str = "R_AL_3_123";
const REPORT: &str = "/api/report/reportcomment";
const RAW: &str = " \u{3000}合成评论举报 / 100%\n末行 \t";

#[derive(Default)]
struct Fixture {
    writes: Mutex<Vec<Value>>,
    lookups: Mutex<Vec<(String, String)>>,
}

impl Api for Fixture {
    fn request<'a>(&'a self, path: &'a str, body: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert!(!cache);
            if let Some(thread) = path.strip_prefix("/api/v1/resource/comments/") {
                assert!(matches!(thread, TRACK | ALBUM));
                let id = body["commentId"].as_str().unwrap();
                assert_eq!(body["limit"], 1);
                self.lookups
                    .lock()
                    .unwrap()
                    .push((thread.into(), id.into()));
                let current = if id == "108" {
                    Value::Null
                } else {
                    json!({"commentId":id, "content":"合成目标", "time":7, "likedCount":0,
                        "status":if id=="107" {-10} else {0},
                        "user":if id=="109" {Value::Null} else {json!({"userId":if id=="106" {"2"} else {"7"}})}})
                };
                return Ok(
                    json!({"code":200,"comments":[],"currentComment":current,"total":1,"more":false}),
                );
            }
            assert_eq!(path, REPORT);
            assert_eq!(body.as_object().unwrap().len(), 3);
            self.writes.lock().unwrap().push(body.clone());
            match body["commentId"].as_str().unwrap() {
                "102" => Err(Error::Transport("synthetic report response lost".into())),
                "103" => Err(Error::Service {
                    endpoint: REPORT.into(),
                    code: json!(-1),
                    body: Box::new(json!({"code":-1})),
                }),
                "104" => Ok(json!({"code":-2})), // This scope has no -2 meaning.
                _ => Ok(json!({"code":200})),
            }
        })
    }
}

fn client(root: &Path, api: &Arc<Fixture>, owner: &str) -> (MusicClient, String) {
    let registry = ProfileRegistry::open(root, ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(
            api.clone(),
            json!({"userId":owner}),
            &session.token().unwrap(),
        )
        .unwrap();
    let epoch = session.token().unwrap().generation.to_string();
    (MusicClient::with_session(session, registry), epoch)
}
fn thread(kind: ResourceKind) -> CommentResource {
    CommentResource {
        kind,
        id: "123".into(),
        owner: None,
    }
}
async fn act(
    music: &MusicClient,
    epoch: &str,
    resource: &CommentResource,
    id: &str,
    command: Command,
) -> Value {
    let value = music
        .comment_report(epoch, resource.clone(), id, command)
        .await
        .unwrap();
    assert_eq!(value.epoch, epoch);
    serde_json::to_value(value.value).unwrap()
}
async fn prepare(
    music: &MusicClient,
    epoch: &str,
    resource: &CommentResource,
    id: &str,
    reason: Reason,
) -> String {
    let value = act(music, epoch, resource, id, Command::Prepare { reason }).await;
    assert_eq!(value["status"], "ready");
    value["receipt"].as_str().unwrap().into()
}

#[tokio::test]
async fn report_uses_canonical_thread_opaque_raw_reason_and_does_not_replay() {
    let tmp = tempfile::tempdir().unwrap();
    let api = Arc::new(Fixture::default());
    let (music, epoch) = client(tmp.path(), &api, "2");
    let track = thread(ResourceKind::Track);
    let album = thread(ResourceKind::Album);
    let cancelled = prepare(&music, &epoch, &track, "105", Reason::Political).await;
    act(
        &music,
        &epoch,
        &track,
        "105",
        Command::Cancel {
            receipt: cancelled.clone(),
        },
    )
    .await;
    assert!(
        music
            .comment_report(
                &epoch,
                track.clone(),
                "105",
                Command::Submit { receipt: cancelled }
            )
            .await
            .is_err()
    );
    assert!(api.lookups.lock().unwrap().is_empty());
    for (id, reason, wire, status) in [
        (
            "101",
            Reason::Meaningless,
            "段子或者无意义的评论",
            "accepted",
        ),
        (
            "102",
            Reason::Other {
                description: RAW.into(),
            },
            RAW,
            "uncertain",
        ),
        ("103", Reason::Advertising, "广告或营销信息", "duplicate"),
        (
            "104",
            Reason::HarmfulContent,
            "色情、低俗或有害内容",
            "uncertain",
        ),
    ] {
        let receipt = prepare(&music, &epoch, &track, id, reason).await;
        assert!(
            music
                .comment_report(
                    &epoch,
                    album.clone(),
                    id,
                    Command::Submit {
                        receipt: receipt.clone()
                    }
                )
                .await
                .is_err()
        );
        let reply = act(
            &music,
            &epoch,
            &track,
            id,
            Command::Submit {
                receipt: receipt.clone(),
            },
        )
        .await;
        assert_eq!(reply, json!({"status":status,"receipt":null}));
        assert_eq!(
            act(&music, &epoch, &track, id, Command::Submit { receipt }).await["status"],
            status
        );
        assert_eq!(
            api.writes.lock().unwrap().last(),
            Some(&json!({"threadId":TRACK,"commentId":id,"reason":wire}))
        );
    }
    assert_eq!(
        act(&music, &epoch, &album, "102", Command::Inspect).await["status"],
        "ready"
    );
    drop(music);
    let (music, epoch) = client(tmp.path(), &api, "2");
    let stored = act(
        &music,
        &epoch,
        &track,
        "102",
        Command::Prepare {
            reason: Reason::Political,
        },
    )
    .await;
    assert_eq!(stored, json!({"status":"uncertain","receipt":null}));
    let registry = ProfileRegistry::open(tmp.path(), ApiOrigin::Pc).unwrap();
    assert_eq!(
        registry
            .store()
            .get("2", "comment-report-receipt-v1", r#"["R_SO_4_123","102"]"#)
            .unwrap(),
        Some(json!("uncertain"))
    );
    let (music, epoch) = client(tmp.path(), &api, "3");
    assert_eq!(
        act(&music, &epoch, &track, "102", Command::Inspect).await["status"],
        "ready"
    );
    assert_eq!(api.writes.lock().unwrap().len(), 4);
    assert_eq!(api.lookups.lock().unwrap().len(), 4);
}

#[tokio::test]
async fn own_removed_missing_and_noncanonical_targets_are_never_sent() {
    let tmp = tempfile::tempdir().unwrap();
    let api = Arc::new(Fixture::default());
    let (music, epoch) = client(tmp.path(), &api, "2");
    let track = thread(ResourceKind::Track);
    for id in ["106", "107", "108", "109"] {
        let receipt = prepare(&music, &epoch, &track, id, Reason::PersonalAttack).await;
        assert_eq!(
            act(&music, &epoch, &track, id, Command::Submit { receipt }).await["status"],
            "target_unavailable"
        );
    }
    assert!(
        music
            .comment_report(&epoch, track.clone(), "0101", Command::Inspect)
            .await
            .is_err()
    );
    let alternative = CommentResource {
        id: "0123".into(),
        ..track
    };
    assert!(
        music
            .comment_report(&epoch, alternative, "101", Command::Inspect)
            .await
            .is_err()
    );
    assert!(api.writes.lock().unwrap().is_empty());
    assert_eq!(api.lookups.lock().unwrap().len(), 4);
}
