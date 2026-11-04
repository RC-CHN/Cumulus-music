use cumulus_core::{
    Error, Result,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::MemberSignCommand},
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering::SeqCst},
};
use tokio::sync::Notify;

const CARD: &str = "/api/vipnewcenter/app/minidesk/music/sign/pc";
const HISTORY: &str = "/api/vipnewcenter/app/level/user/checkin/history/list";
const DETAIL: &str = "/api/vipnewcenter/app/level/user/checkin/history/detail";
const SIGN: &str = "/api/vip-center-bff/task/sign";
const DAY: &str = "2026-11-01";
const NOW: u64 = 1_793_491_200_000;
const RECORD: &str = "009007199254740993";

#[derive(Default)]
enum SignReply {
    #[default]
    Accepted,
    Lost,
    NotSent,
    Hold,
}

#[derive(Default)]
struct Backend {
    clock: AtomicU64,
    signed_card: AtomicBool,
    signed_history: AtomicBool,
    reply: Mutex<SignReply>,
    calls: Mutex<Vec<(&'static str, Value)>>,
    started: Notify,
}

impl Backend {
    fn at_day() -> Arc<Self> {
        Arc::new(Self {
            clock: AtomicU64::new(NOW),
            ..Self::default()
        })
    }

    fn take_calls(&self) -> Vec<(&'static str, Value)> {
        std::mem::take(&mut *self.calls.lock().unwrap())
    }
}

impl Api for Backend {
    fn observed_server_time_ms(&self) -> Option<u64> {
        Some(self.clock.load(SeqCst)).filter(|clock| *clock > 0)
    }

    fn request<'a>(&'a self, path: &'a str, args: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert!(!cache);
            let path = [CARD, HISTORY, DETAIL, SIGN]
                .into_iter()
                .find(|known| *known == path)
                .expect("unrelated request");
            self.calls.lock().unwrap().push((path, args));
            let data = match path {
                CARD => json!({"signInfoList":[
                    {"dayText":"1日","today":true,"sign":self.signed_card.load(SeqCst),"signTime":NOW}
                ]}),
                HISTORY if self.signed_history.load(SeqCst) => {
                    json!([{ "dayTime":NOW,"recordId":RECORD }])
                }
                HISTORY => json!([]),
                DETAIL => json!({"time":NOW,"songInfo":{"songId":17,"songName":"合成乐签"}}),
                SIGN => {
                    let reply = std::mem::take(&mut *self.reply.lock().unwrap());
                    match reply {
                        SignReply::Accepted => json!({}), // Even an empty JS object is truthy.
                        SignReply::Lost => {
                            return Err(Error::Transport("fixture lost response".into()));
                        }
                        SignReply::NotSent => {
                            return Err(Error::RequestNotSent(Box::new(Error::State(
                                "fixture refused preparation",
                            ))));
                        }
                        SignReply::Hold => {
                            self.started.notify_one();
                            return std::future::pending::<Result<Value>>().await;
                        }
                    }
                }
                _ => unreachable!(),
            };
            Ok(json!({"code":200,"data":data}))
        })
    }
}

fn client(path: &std::path::Path, api: Arc<Backend>, uid: u64) -> Arc<MusicClient> {
    let registry = ProfileRegistry::open(path, ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api, json!({"userId":uid}), &session.token().unwrap())
        .unwrap();
    Arc::new(MusicClient::with_session(session, registry))
}

fn check() -> MemberSignCommand {
    serde_json::from_value(json!({"kind":"check_in","dayKey":DAY})).unwrap()
}

#[tokio::test]
async fn explicit_sign_sends_once_and_success_reads_type_one_then_card() {
    let dir = tempfile::tempdir().unwrap();
    let api = Backend::at_day();
    let client = client(dir.path(), api.clone(), 1);
    let card = serde_json::to_value(client.member_sign_card("1").await.unwrap()).unwrap();
    assert_eq!(
        card["value"]["action"],
        json!({"phase":"ready","dayKey":DAY})
    );
    let value =
        serde_json::to_value(client.member_sign_command("1", check()).await.unwrap()).unwrap();
    assert_eq!(
        value["value"]["action"],
        json!({"phase":"signed","dayKey":DAY})
    );
    assert_eq!(value["value"]["detail"]["songId"], "17");
    // The write was acknowledged even though the refreshed slot has not caught up.
    assert_eq!(value["value"]["card"]["days"][0]["signed"], false);
    assert_eq!(value["value"]["card"]["action"]["phase"], "signed");
    assert_eq!(
        api.take_calls(),
        vec![
            (CARD, json!({"type":1})),
            (CARD, json!({"type":1})),
            (SIGN, json!({})),
            (DETAIL, json!({"type":1,"signDayTime":NOW})),
            (CARD, json!({"type":1}))
        ]
    );
    client.member_sign_command("1", check()).await.unwrap();
    assert_eq!(
        api.take_calls(),
        vec![(CARD, json!({"type":1})), (HISTORY, json!({}))]
    );
}

#[tokio::test]
async fn lost_ack_remains_unknown_across_restart_then_recovers_from_single_record_read() {
    let dir = tempfile::tempdir().unwrap();
    let api = Backend::at_day();
    *api.reply.lock().unwrap() = SignReply::Lost;
    let first = client(dir.path(), api.clone(), 1);
    let value =
        serde_json::to_value(first.member_sign_command("1", check()).await.unwrap()).unwrap();
    assert_eq!(value["value"]["action"]["phase"], "unknown");
    assert!(value["value"]["detail"].is_null());
    assert_eq!(
        api.take_calls()
            .iter()
            .map(|(path, _)| *path)
            .collect::<Vec<_>>(),
        [CARD, SIGN, CARD, HISTORY]
    );
    drop(first);

    let reopened = client(dir.path(), api.clone(), 1);
    let card = serde_json::to_value(reopened.member_sign_card("1").await.unwrap()).unwrap();
    assert_eq!(card["value"]["action"]["phase"], "unknown");
    let still_unknown = reopened.member_sign_command("1", check()).await.unwrap();
    assert_eq!(
        serde_json::to_value(still_unknown).unwrap()["value"]["action"]["phase"],
        "unknown"
    );
    assert_eq!(
        api.take_calls()
            .iter()
            .map(|(path, _)| *path)
            .collect::<Vec<_>>(),
        [CARD, CARD, HISTORY]
    );
    api.signed_history.store(true, SeqCst);
    let reconciled = reopened
        .member_sign_command("1", MemberSignCommand::Reconcile)
        .await
        .unwrap();
    let value = serde_json::to_value(reconciled).unwrap();
    assert_eq!(value["value"]["action"]["phase"], "signed");
    assert_eq!(value["value"]["detail"]["song"], "合成乐签");
    assert!(!value.to_string().contains(RECORD));
    assert_eq!(
        api.take_calls(),
        vec![
            (CARD, json!({"type":1})),
            (HISTORY, json!({})),
            (DETAIL, json!({"type":2,"recordId":RECORD}))
        ]
    );
}

#[tokio::test]
async fn abandoned_send_is_persistent_account_local_and_lock_blocks_concurrent_clicks() {
    let dir = tempfile::tempdir().unwrap();
    let api = Backend::at_day();
    *api.reply.lock().unwrap() = SignReply::Hold;
    let first = client(dir.path(), api.clone(), 1);
    let other_host = client(dir.path(), api.clone(), 1);
    let pending = tokio::spawn(async move { first.member_sign_command("1", check()).await });
    api.started.notified().await;
    assert!(matches!(
        other_host.member_sign_command("1", check()).await,
        Err(Error::MutationBusy)
    ));
    pending.abort();
    assert!(pending.await.err().unwrap().is_cancelled());
    let other_account = client(dir.path(), api.clone(), 2);
    let other = serde_json::to_value(other_account.member_sign_card("1").await.unwrap()).unwrap();
    assert_eq!(other["value"]["action"]["phase"], "ready");
    let recovered = other_host
        .member_sign_command("1", MemberSignCommand::Reconcile)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(recovered).unwrap()["value"]["action"]["phase"],
        "unknown"
    );
    assert_eq!(
        api.take_calls()
            .iter()
            .map(|(path, _)| *path)
            .collect::<Vec<_>>(),
        [CARD, SIGN, CARD, CARD, HISTORY]
    );
}

#[tokio::test]
async fn stale_day_and_already_signed_skip_write_and_definitely_not_sent_can_retry_explicitly() {
    let dir = tempfile::tempdir().unwrap();
    let api = Backend::at_day();
    let first = client(dir.path(), api.clone(), 1);
    assert!(matches!(
        first
            .member_sign_command(
                "1",
                MemberSignCommand::CheckIn {
                    day_key: "2026-10-31".into()
                }
            )
            .await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(api.take_calls(), vec![(CARD, json!({"type":1}))]);
    *api.reply.lock().unwrap() = SignReply::NotSent;
    assert!(matches!(
        first.member_sign_command("1", check()).await,
        Err(Error::RequestNotSent(_))
    ));
    assert_eq!(
        api.take_calls()
            .iter()
            .map(|(path, _)| *path)
            .collect::<Vec<_>>(),
        [CARD, SIGN]
    );
    let card = serde_json::to_value(first.member_sign_card("1").await.unwrap()).unwrap();
    assert_eq!(card["value"]["action"]["phase"], "ready");
    assert_eq!(api.take_calls(), vec![(CARD, json!({"type":1}))]);
    api.signed_card.store(true, SeqCst);
    api.signed_history.store(true, SeqCst);
    let signed =
        serde_json::to_value(first.member_sign_command("1", check()).await.unwrap()).unwrap();
    assert_eq!(signed["value"]["action"]["phase"], "signed");
    assert_eq!(
        api.take_calls(),
        vec![
            (CARD, json!({"type":1})),
            (HISTORY, json!({})),
            (DETAIL, json!({"type":2,"recordId":RECORD}))
        ]
    );
}
