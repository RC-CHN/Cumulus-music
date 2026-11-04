use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{ProfileBlacklistCommand, ProfileFollowCommand},
    },
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
};

const LIST: &str = "/api/blacklist/get";
const ADD: &str = "/api/blacklist/add";
const DELETE: &str = "/api/blacklist/delete";

#[derive(Default)]
struct ApiFixture {
    calls: Mutex<Vec<(String, Value)>>,
    blocked: Mutex<BTreeSet<String>>,
    full: AtomicI64,
    lost_delete: AtomicBool,
    next_read_offline: AtomicBool,
    partial: AtomicBool,
}
impl Api for ApiFixture {
    fn request<'a>(&'a self, path: &'a str, body: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert!(!cache);
            self.calls.lock().unwrap().push((path.into(), body.clone()));
            Ok(match path {
                LIST => {
                    assert!(body == json!({"userid":"2"}) || body == json!({"userid":"3"}));
                    if self.next_read_offline.swap(false, Ordering::SeqCst) {
                        return Err(Error::Transport("lost list reply".into()));
                    }
                    let rows = if body["userid"] == "3" {
                        Vec::new()
                    } else {
                        self.blocked.lock().unwrap().iter().map(|id| json!({
                            "userId":id,"nickName":"Fixture user","signature":"Profile bio",
                            "accountStatus": if id=="74" {40}else{0},
                            "avatarUrl":"https://p1.music.126.net/fixture.jpg?secret=PRIVATE",
                            "credential":"PRIVATE"
                        })).collect::<Vec<_>>()
                    };
                    json!({"code":200,"data":rows,"hasMore":self.partial.load(Ordering::SeqCst)})
                }
                ADD => {
                    assert_eq!(body, json!({"userid":"73"}));
                    let code = self.full.load(Ordering::SeqCst);
                    if code != -2 {
                        self.blocked.lock().unwrap().insert("73".into());
                    }
                    json!({"code":if code == 0 { 200 } else { code }})
                }
                DELETE => {
                    assert_eq!(body, json!({"userid":"73"}));
                    self.blocked.lock().unwrap().remove("73");
                    if self.lost_delete.swap(false, Ordering::SeqCst) {
                        self.next_read_offline.store(true, Ordering::SeqCst);
                        return Err(Error::Transport("lost delete reply".into()));
                    }
                    json!({"code":200})
                }
                "/api/w/v1/user/detail/73" | "/api/w/v1/user/detail/75" => {
                    let id = path.rsplit('/').next().unwrap();
                    assert_eq!(body, json!({"all":true,"userId":id}));
                    json!({"code":200,"profile":{"userId":id,"nickname":"Fixture",
                        "blacklist":self.blocked.lock().unwrap().contains(id),"inBlacklist":id=="75",
                        "followed":false}})
                }
                _ => panic!("unexpected closed blacklist request {path}"),
            })
        })
    }
}

fn client(root: &Path, api: Arc<ApiFixture>, uid: &str) -> (MusicClient, String) {
    let registry = ProfileRegistry::open(root, ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api, json!({"userId":uid}), &session.token().unwrap())
        .unwrap();
    let epoch = session.token().unwrap().generation.to_string();
    (MusicClient::with_session(session, registry), epoch)
}
fn change(expected: bool, origin: &str) -> ProfileBlacklistCommand {
    serde_json::from_value(json!({"kind":"change","expected":expected,"origin":origin})).unwrap()
}

#[tokio::test]
async fn exact_mutations_follow_complete_membership_and_do_not_replay_lost_delete() {
    let tmp = tempfile::tempdir().unwrap();
    let api = Arc::new(ApiFixture::default());
    api.blocked.lock().unwrap().insert("74".into());
    let (music, epoch) = client(tmp.path(), api.clone(), "2");
    let read =
        serde_json::to_value(music.profile_blacklist_list(&epoch).await.unwrap().value).unwrap();
    assert_eq!(read["users"][0]["closed"], true);
    assert!(!read.to_string().contains("PRIVATE"));
    let add = serde_json::to_value(
        music
            .profile_blacklist(&epoch, "73", change(false, "profile"))
            .await
            .unwrap()
            .value,
    )
    .unwrap();
    assert_eq!(add["status"], "added");
    assert_eq!(add["blocked"], true);
    assert_eq!(add["users"].as_array().unwrap().len(), 2);
    api.lost_delete.store(true, Ordering::SeqCst);
    let lost = serde_json::to_value(
        music
            .profile_blacklist(&epoch, "73", change(true, "list"))
            .await
            .unwrap()
            .value,
    )
    .unwrap();
    assert_eq!(lost["status"], "uncertain");
    assert_eq!(lost["blocked"], Value::Null);
    assert_eq!(lost["pending"], json!({"id":"73","desired":false}));
    assert!(matches!(
        music
            .profile_blacklist(&epoch, "73", change(true, "list"))
            .await,
        Err(Error::MutationUncertain)
    ));
    assert!(matches!(
        music
            .profile_follow(
                &epoch,
                "73",
                ProfileFollowCommand::Change { expected: false }
            )
            .await,
        Err(Error::MutationUncertain)
    ));
    drop(music);
    let (other, other_epoch) = client(tmp.path(), api.clone(), "3");
    let read = serde_json::to_value(
        other
            .profile_blacklist_list(&other_epoch)
            .await
            .unwrap()
            .value,
    )
    .unwrap();
    assert_eq!(read["pending"], Value::Null);
    drop(other);
    let (reopen, epoch) = client(tmp.path(), api.clone(), "2");
    let read =
        serde_json::to_value(reopen.profile_blacklist_list(&epoch).await.unwrap().value).unwrap();
    assert_eq!(read["pending"], json!({"id":"73","desired":false}));
    let confirmed = serde_json::to_value(
        reopen
            .profile_blacklist(&epoch, "73", ProfileBlacklistCommand::Reconcile)
            .await
            .unwrap()
            .value,
    )
    .unwrap();
    assert_eq!(confirmed["status"], "removed");
    assert_eq!(confirmed["pending"], Value::Null);
    assert_eq!(confirmed["users"][0]["id"], "74");
    let list2 = || (LIST.into(), json!({"userid":"2"}));
    assert_eq!(
        *api.calls.lock().unwrap(),
        vec![
            list2(),
            list2(),
            (
                "/api/w/v1/user/detail/73".into(),
                json!({"all":true,"userId":"73"})
            ),
            (ADD.into(), json!({"userid":"73"})),
            list2(),
            list2(),
            (DELETE.into(), json!({"userid":"73"})),
            list2(),
            (LIST.into(), json!({"userid":"3"})),
            list2(),
            list2(),
        ]
    );
}

#[tokio::test]
async fn authoritative_absence_directional_privacy_and_full_list_are_fail_closed() {
    let tmp = tempfile::tempdir().unwrap();
    let api = Arc::new(ApiFixture::default());
    let (music, epoch) = client(tmp.path(), api.clone(), "2");
    for id in ["2", "02", "not-user"] {
        assert!(
            music
                .profile_blacklist(&epoch, id, change(false, "profile"))
                .await
                .is_err()
        );
    }
    assert!(
        music
            .profile_blacklist(&epoch, "73", change(false, "list"))
            .await
            .is_err()
    );
    assert!(api.calls.lock().unwrap().is_empty());
    api.partial.store(true, Ordering::SeqCst);
    assert!(
        music
            .profile_blacklist(&epoch, "73", change(false, "profile"))
            .await
            .is_err()
    );
    api.partial.store(false, Ordering::SeqCst);
    let state = async |id: &str| {
        serde_json::to_value(
            music
                .profile_blacklist(&epoch, id, change(false, "profile"))
                .await
                .unwrap()
                .value,
        )
        .unwrap()
    };
    assert_eq!(state("75").await["status"], "unavailable");
    api.full.store(-2, Ordering::SeqCst);
    assert_eq!(state("73").await["status"], "full");
    api.full.store(-1, Ordering::SeqCst);
    assert_eq!(state("73").await["status"], "already_blocked");
    assert_eq!(state("73").await["status"], "unchanged");
    assert_eq!(
        api.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(route, _)| route == ADD)
            .count(),
        2
    );
    assert!(
        !api.calls
            .lock()
            .unwrap()
            .iter()
            .any(|(route, _)| route == DELETE)
    );
}
