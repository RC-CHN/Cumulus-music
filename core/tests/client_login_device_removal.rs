mod support;

use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::LoginDeviceRemovalCommand as Command},
    protocol::http::ApiOrigin,
};
use serde_json::{Value, json};
use std::sync::Arc;
use support::{Reply, ScriptApi};

const LIST: &str = "/api/middle/user/device/list";
const KICKOFF: &str = "/api/middle/user/security/device/kickoff";
const SMS: &str = "/api/sms/captcha/safe/sent";
const BINDINGS: &str = "/api/w/v1/user/bindings/1";
const TARGET: &str = "SYNTHETIC_TARGET_KEY";

fn list(target_present: bool) -> Reply {
    let mut rows = vec![json!({
        "deviceKey":"SYNTHETIC_CURRENT_KEY","ifLoginDevice":true,"status":1
    })];
    if target_present {
        rows.push(json!({"deviceKey":TARGET,"deviceName":"Old phone","status":0}));
    }
    json!({"code":200,"data":{"userDeviceList":rows}}).into()
}

fn binding() -> Reply {
    json!({"code":200,"bindings":[{"type":1,"tokenJsonStr":json!({
        "cellphone":"1234567890","countrycode":"1242","access_token":"SYNTHETIC_SECRET"
    }).to_string()}]})
    .into()
}

struct World {
    _dir: tempfile::TempDir,
    registry: Arc<ProfileRegistry>,
    session: Arc<AccountSession>,
    api: Arc<ScriptApi>,
    client: Arc<MusicClient>,
    epoch: String,
    target: String,
    current: String,
}

async fn world(replies: impl IntoIterator<Item = Reply>) -> World {
    let dir = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let api = Arc::new(ScriptApi::new(
        [
            json!({"code":200,"profile":{"userId":1}}).into(),
            list(true),
        ]
        .into_iter()
        .chain(replies),
    ));
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session.refresh().await.unwrap();
    let client = Arc::new(MusicClient::with_session(session.clone(), registry.clone()));
    let epoch = client.account().unwrap().epoch;
    let devices = client.login_devices(&epoch).await.unwrap().value.devices;
    let current = devices.iter().find(|row| row.current).unwrap().id.clone();
    let target = devices.iter().find(|row| !row.current).unwrap().id.clone();
    World {
        _dir: dir,
        registry,
        session,
        api,
        client,
        epoch,
        target,
        current,
    }
}

async fn command(world: &World, command: Command) -> Value {
    let view = world
        .client
        .login_device_removal_command(&world.epoch, command)
        .await
        .unwrap();
    serde_json::to_value(view.value).unwrap()
}

fn begin(world: &World) -> Command {
    Command::Begin {
        device_id: world.target.clone(),
    }
}

fn send(world: &World) -> Command {
    Command::SendSms {
        operation_id: world.target.clone(),
    }
}

fn reconcile(world: &World) -> Command {
    Command::Reconcile {
        operation_id: world.target.clone(),
    }
}

#[tokio::test]
async fn historical_device_removal_is_confirmed_by_fresh_absence_and_never_replayed() {
    let w = world([
        binding(),
        list(true),
        json!({"code":"200"}).into(),
        list(false),
    ])
    .await;
    let view = command(&w, begin(&w)).await;
    assert_eq!(view["phase"], "observed_absent");
    assert_eq!(view["observedPresent"], false);
    assert_eq!(command(&w, begin(&w)).await["phase"], "observed_absent");
    let calls = w.api.calls();
    assert_eq!(calls.len(), 6);
    assert_eq!(calls[4].path, KICKOFF);
    assert_eq!(calls[4].payload, json!({"deviceKey":TARGET}));
    assert_eq!(calls[5].path, LIST);
    assert_eq!(calls[5].payload, json!({"excStatus":9}));
    let journal = w
        .registry
        .store()
        .get("1", "login-device-removal-v1", "active")
        .unwrap()
        .unwrap();
    for excluded in [TARGET, "1234567890", "SYNTHETIC_SECRET"] {
        assert!(!journal.to_string().contains(excluded));
        assert!(!view.to_string().contains(excluded));
    }
}

#[tokio::test]
async fn sms_uses_only_bound_country_then_the_same_kickoff_and_read_only_recovery() {
    let w = world([
        binding(),
        list(true),
        Reply::Ready(Err(Error::HttpStatus {
            status: 401,
            endpoint: Some(KICKOFF.into()),
            body: Some(Box::new(json!({"code":"401"}))),
        })),
        list(true),
        binding(),
        json!({"code":200}).into(),
        binding(),
        list(true),
        json!({"code":200}).into(),
        list(true),
        list(false),
    ])
    .await;
    let challenge = command(&w, begin(&w)).await;
    assert_eq!(challenge["phase"], "needs_sms");
    assert_eq!(challenge["phoneTail"], "7890");
    assert_eq!(w.api.calls().len(), 5);
    assert_eq!(command(&w, send(&w)).await["phase"], "sms_ready");
    assert!(
        w.client
            .login_device_removal_command(&w.epoch, send(&w))
            .await
            .is_err()
    );
    assert_eq!(w.api.calls().len(), 8);
    let submitted = command(
        &w,
        Command::SubmitSms {
            operation_id: w.target.clone(),
            code: "001234".into(),
        },
    )
    .await;
    assert_eq!(submitted["phase"], "recovery_required");
    assert_eq!(submitted["observedPresent"], true);
    let reopened = MusicClient::with_session(w.session.clone(), w.registry.clone());
    let found = reopened
        .login_device_removal(&w.epoch)
        .unwrap()
        .value
        .unwrap();
    assert_eq!(found.id, w.target);
    let checked = reopened
        .login_device_removal_command(&w.epoch, reconcile(&w))
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(checked.value).unwrap()["phase"],
        "observed_absent"
    );
    let calls = w.api.calls();
    assert_eq!(calls.len(), 13);
    assert_eq!(calls[7].path, SMS);
    assert_eq!(calls[7].payload, json!({"ctCode":"1242"}));
    assert_eq!(calls[10].path, KICKOFF);
    assert_eq!(
        calls[10].payload,
        json!({"deviceKey":TARGET,"captcha":"001234"})
    );
    assert_eq!(calls[12].path, LIST);
    let journal = w
        .registry
        .store()
        .get("1", "login-device-removal-v1", "active")
        .unwrap()
        .unwrap();
    assert!(!journal.to_string().contains("001234"));
}

#[tokio::test]
async fn dropped_kickoff_is_durable_and_cannot_be_cancelled_or_sent_again() {
    let (reply, sender) = support::gate();
    let w = world([binding(), list(true), reply, list(true)]).await;
    let task = {
        let client = w.client.clone();
        let epoch = w.epoch.clone();
        let command = begin(&w);
        tokio::spawn(async move { client.login_device_removal_command(&epoch, command).await })
    };
    w.api.wait_for_calls(5).await;
    task.abort();
    assert!(task.await.is_err());
    assert!(sender.send(Ok(json!({"code":200}))).is_err());
    assert_eq!(command(&w, begin(&w)).await["phase"], "recovery_required");
    assert!(
        w.client
            .login_device_removal_command(
                &w.epoch,
                Command::Cancel {
                    operation_id: w.target.clone()
                }
            )
            .await
            .is_err()
    );
    let checked = command(&w, reconcile(&w)).await;
    assert_eq!(checked["observedPresent"], true);
    assert_eq!(checked["phase"], "recovery_required");
    assert_eq!(
        w.api
            .calls()
            .iter()
            .filter(|call| call.path == KICKOFF)
            .count(),
        1
    );
}

#[tokio::test]
async fn current_and_unbound_devices_do_not_reach_kickoff() {
    let w = world([json!({"code":200,"bindings":[]}).into()]).await;
    assert!(
        w.client
            .login_device_removal_command(
                &w.epoch,
                Command::Begin {
                    device_id: w.current.clone()
                }
            )
            .await
            .is_err()
    );
    assert_eq!(w.api.calls().len(), 2);
    assert_eq!(command(&w, begin(&w)).await["phase"], "needs_binding");
    assert_eq!(w.api.calls().len(), 3);
    assert_eq!(w.api.calls()[2].path, BINDINGS);
}

#[tokio::test]
async fn bare_http_unauthorized_is_not_a_device_sms_challenge() {
    let w = world([
        binding(),
        list(true),
        Reply::Ready(Err(Error::HttpStatus {
            status: 401,
            endpoint: Some(KICKOFF.into()),
            body: None,
        })),
    ])
    .await;
    assert_eq!(command(&w, begin(&w)).await["phase"], "recovery_required");
    assert!(
        w.client
            .login_device_removal_command(&w.epoch, send(&w))
            .await
            .is_err()
    );
    assert_eq!(w.api.calls().len(), 5);
}

#[tokio::test]
async fn failed_binding_read_cannot_leave_a_pending_removal() {
    let w = world([json!({"code":503}).into()]).await;
    let result = w
        .client
        .login_device_removal_command(&w.epoch, begin(&w))
        .await;
    assert!(matches!(result, Err(Error::RequestNotSent(_))));
    assert!(
        w.client
            .login_device_removal(&w.epoch)
            .unwrap()
            .value
            .is_none()
    );
    assert_eq!(w.api.calls().last().unwrap().path, BINDINGS);
    assert_eq!(w.api.calls().len(), 3);
}
