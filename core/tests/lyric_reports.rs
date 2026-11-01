mod support;
use cumulus_core::{Error, account::AccountSession, lyrics::reports::*, storage::StateStore};
use serde_json::{Value, json};
use std::sync::Arc;
use support::{Reply, ScriptApi, gate};

fn fixture(
    replies: impl IntoIterator<Item = Reply>,
) -> (
    tempfile::TempDir,
    Arc<StateStore>,
    Arc<AccountSession>,
    Arc<ScriptApi>,
    Arc<Reports>,
) {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(dir.path()).unwrap());
    let api = Arc::new(ScriptApi::new(replies));
    let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
    session
        .adopt(api.clone(), json!({"userId":11}), &session.token().unwrap())
        .unwrap();
    let reports = Arc::new(Reports::new(session.clone(), session.token().unwrap()).unwrap());
    (dir, store, session, api, reports)
}
fn target() -> Target {
    Target {
        song_id: "7".into(),
        title: "合成歌曲".into(),
        lyric_version: Some(json!(17)),
    }
}
fn types() -> Value {
    json!({"code":200,"data":{"1":"discard1","10":"其他","3":"discard3","4":"歌词有误","2":"discard2","5":"歌词不同步"}})
}
async fn ready(reports: &Reports) -> Record {
    let r = reports.open(target()).unwrap();
    reports.types(&r.id, &r.revision).await.unwrap()
}
#[test]
fn official_type_exclusion_numeric_order_and_utf16_boundaries() {
    let choices = parse_types(&types()).unwrap();
    assert_eq!(
        choices.iter().map(|v| v.key.as_str()).collect::<Vec<_>>(),
        ["4", "5", "10"]
    );
    for text in ["12345", "     ", &"😀".repeat(25)] {
        validate_content(text).unwrap();
    }
    for text in ["1234", &"😀".repeat(26)] {
        assert!(validate_content(text).is_err());
    }
    assert!(parse_types(&json!({"data":{"4":null}})).is_err());
    assert!(parse_types(&json!({"data":{"wrong":"bad"}})).is_err());
}
#[test]
fn frozen_public_types_keep_all_eleven_server_labels_and_numeric_order() {
    let expected: Vec<CorrectionType> =
        serde_json::from_str(include_str!("fixtures/lyric_report_types.json")).unwrap();
    let data = expected
        .iter()
        .rev()
        .map(|v| (v.key.clone(), json!(v.label)))
        .collect::<serde_json::Map<String, Value>>();
    let parsed = parse_types(&json!({"data":data})).unwrap();
    assert_eq!(parsed, expected);
    assert_eq!(parsed.len(), 11);
    assert_eq!(
        parsed.iter().find(|v| v.value == 10).unwrap().label,
        "歌词缺失/错误"
    );
    assert_eq!(
        parsed.iter().find(|v| v.value == 14).unwrap().label,
        "歌词时间轴错误"
    );
}
#[tokio::test]
async fn exact_official_payload_single_send_and_reopen_preserve_receipts() {
    let (dir, _store, _session, api, reports) =
        fixture([types().into(), json!({"code":200}).into()]);
    let r = ready(&reports).await;
    assert!(
        reports
            .submit(&r.id, &r.revision, "1", "12345")
            .await
            .is_err()
    );
    assert!(
        reports
            .submit(&r.id, &r.revision, "4", "1234")
            .await
            .is_err()
    );
    assert_eq!(api.calls().len(), 1);
    let sent = reports
        .submit(&r.id, &r.revision, "4", "歌词内容有误")
        .await
        .unwrap();
    assert_eq!(sent.phase, Phase::Sent);
    let calls = api.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].path, TYPES_PATH);
    assert_eq!(calls[0].payload, json!({}));
    assert_eq!(calls[1].path, SUBMIT_PATH);
    assert_eq!(
        calls[1].payload,
        json!({"songId":"7","lyricVersion":17,"content":"歌词内容有误","correctType":4})
    );
    assert!(!calls[1].cache);
    assert!(
        reports
            .submit(&r.id, &r.revision, "4", "歌词内容有误")
            .await
            .is_err()
    );
    let store = Arc::new(StateStore::open(dir.path()).unwrap());
    let session = Arc::new(AccountSession::new(api.clone(), Some(store)));
    session
        .adopt(api.clone(), json!({"userId":11}), &session.token().unwrap())
        .unwrap();
    let reopened = Reports::new(session.clone(), session.token().unwrap()).unwrap();
    assert_eq!(reopened.read(&r.id).unwrap().phase, Phase::Sent);
    assert_ne!(reopened.open(target()).unwrap().id, r.id);
    assert_eq!(api.calls().len(), 2);
}
#[tokio::test]
async fn rejection_and_preparation_failure_keep_form_and_allow_only_explicit_retry() {
    let (_dir, _store, _session, api, reports) = fixture([
        types().into(),
        json!({"code":403}).into(),
        Reply::Ready(Err(Error::RequestNotSent(Box::new(Error::State(
            "synthetic prepare failure",
        ))))),
        json!({"code":200}).into(),
    ]);
    let r = ready(&reports).await;
    let rejected = reports
        .submit(&r.id, &r.revision, "5", "歌词不同步了")
        .await
        .unwrap();
    assert_eq!(rejected.phase, Phase::Rejected);
    assert_eq!(rejected.issue, Some(Issue::Rejected));
    assert_eq!(rejected.content, "歌词不同步了");
    assert_eq!(rejected.selected.as_deref(), Some("5"));
    assert_eq!(reports.open(target()).unwrap().id, r.id);
    assert_eq!(api.calls().len(), 2);
    let not_sent = reports
        .submit(&r.id, &rejected.revision, "5", "歌词不同步了")
        .await
        .unwrap();
    assert_eq!(not_sent.issue, Some(Issue::NotSent));
    let sent = reports
        .submit(&r.id, &not_sent.revision, "5", "歌词不同步了")
        .await
        .unwrap();
    assert_eq!(sent.phase, Phase::Sent);
    assert_eq!(api.calls().len(), 4);
}
#[tokio::test]
async fn unknown_acknowledgement_never_replays_even_after_new_lyric_version_or_restart() {
    let (_dir, _store, session, api, reports) = fixture([
        types().into(),
        Reply::Ready(Err(Error::Transport("synthetic lost reply".into()))),
    ]);
    let r = ready(&reports).await;
    let unknown = reports
        .submit(&r.id, &r.revision, "4", "歌词内容有误")
        .await
        .unwrap();
    assert_eq!(unknown.phase, Phase::Uncertain);
    let reopened = Reports::new(session.clone(), session.token().unwrap()).unwrap();
    let mut next = target();
    next.lyric_version = Some(json!(99));
    assert_eq!(reopened.open(next.clone()).unwrap().id, r.id);
    assert!(
        reopened
            .submit(&r.id, &unknown.revision, "4", "歌词内容有误")
            .await
            .is_err()
    );
    assert!(reopened.types(&r.id, &unknown.revision).await.is_err());
    assert_eq!(api.calls().len(), 2);
    let archived = reopened.abandon(&r.id, &unknown.revision).unwrap();
    assert_eq!(archived.phase, Phase::Abandoned);
    assert_eq!(archived.issue, Some(Issue::Unknown));
    assert_ne!(reopened.open(next).unwrap().id, r.id);
    assert_eq!(api.calls().len(), 2);
}
#[tokio::test]
async fn cancellation_and_other_hosts_cannot_repeat_an_inflight_report() {
    let (pending, _finish) = gate();
    let (_dir, _store, _session, api, reports) = fixture([types().into(), pending]);
    let r = ready(&reports).await;
    let worker = reports.clone();
    let id = r.id.clone();
    let revision = r.revision.clone();
    let task =
        tokio::spawn(async move { worker.submit(&id, &revision, "4", "歌词内容有误").await });
    api.wait_for_calls(2).await;
    assert!(matches!(
        reports
            .submit(&r.id, &r.revision, "4", "歌词内容有误")
            .await,
        Err(Error::MutationBusy)
    ));
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let saved = reports.open(target()).unwrap();
    assert_eq!(saved.phase, Phase::Sending);
    assert!(matches!(
        reports
            .submit(&r.id, &saved.revision, "4", "歌词内容有误")
            .await,
        Err(Error::MutationUncertain)
    ));
    assert_eq!(api.calls().len(), 2);
}
#[tokio::test]
async fn late_success_is_saved_under_original_account_and_never_delivered_to_replacement() {
    let (pending, finish) = gate();
    let (_dir, store, session, api, reports) = fixture([types().into(), pending]);
    let r = ready(&reports).await;
    let worker = reports.clone();
    let id = r.id.clone();
    let revision = r.revision.clone();
    let task =
        tokio::spawn(async move { worker.submit(&id, &revision, "4", "歌词内容有误").await });
    api.wait_for_calls(2).await;
    session
        .adopt(api.clone(), json!({"userId":22}), &session.token().unwrap())
        .unwrap();
    finish.send(Ok(json!({"code":200}))).unwrap();
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(
        store.get("11", "lyric-reports-v1", &r.id).unwrap().unwrap()["phase"],
        "sent"
    );
    let other = Reports::new(session.clone(), session.token().unwrap()).unwrap();
    assert!(other.read(&r.id).is_err());
    assert_eq!(api.calls().len(), 2);
}
#[tokio::test]
async fn persistent_failure_before_send_prevents_request_and_after_ack_stays_unconfirmed() {
    let (pending, finish) = gate();
    let (dir, _store, _session, api, reports) = fixture([types().into(), pending]);
    let r = ready(&reports).await;
    let sql = rusqlite::Connection::open(dir.path().join("core.sqlite3")).unwrap();
    let reject = "CREATE TRIGGER reject_lyric_report BEFORE INSERT ON state WHEN NEW.kind='lyric-reports-v1' BEGIN SELECT RAISE(ABORT,'synthetic report storage failure'); END;";
    sql.execute_batch(reject).unwrap();
    assert!(
        reports
            .submit(&r.id, &r.revision, "4", "歌词内容有误")
            .await
            .is_err()
    );
    assert_eq!(api.calls().len(), 1);
    sql.execute_batch("DROP TRIGGER reject_lyric_report;")
        .unwrap();
    let worker = reports.clone();
    let id = r.id.clone();
    let revision = r.revision.clone();
    let task =
        tokio::spawn(async move { worker.submit(&id, &revision, "4", "歌词内容有误").await });
    api.wait_for_calls(2).await;
    sql.execute_batch(reject).unwrap();
    finish.send(Ok(json!({"code":200}))).unwrap();
    assert_eq!(task.await.unwrap().unwrap().phase, Phase::Sending);
    assert_eq!(reports.read(&r.id).unwrap().phase, Phase::Sending);
    assert_eq!(api.calls().len(), 2);
}
