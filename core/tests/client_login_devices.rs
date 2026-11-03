mod support;

use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{ClientError, MusicClient},
    protocol::http::ApiOrigin,
};
use serde_json::{Value, json};
use std::sync::Arc;
use support::{Reply, ScriptApi};

fn world(
    replies: impl IntoIterator<Item = Reply>,
) -> (
    tempfile::TempDir,
    Arc<ScriptApi>,
    Arc<AccountSession>,
    Arc<MusicClient>,
) {
    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let api = Arc::new(ScriptApi::new(replies));
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    let client = Arc::new(MusicClient::with_session(session.clone(), registry));
    (root, api, session, client)
}

fn identity() -> Reply {
    json!({"code":200,"profile":{"userId":1,"nickname":"Synthetic account"}}).into()
}

fn list(rows: Value) -> Value {
    json!({"code":200,"data":{"userDeviceList":rows}})
}

#[tokio::test]
async fn reads_official_device_groups_and_projects_only_bounded_display_fields() {
    let reply = json!({
        "code":"200",
        "data":{
            "userDeviceList":[
                {"deviceKey":"SYNTHETIC_RAW_OLD", "deviceName":"Previous computer", "status":0,
                 "deviceId":"SYNTHETIC_RAW_ID", "os":"forged\nsecond line",
                 "lastActiveTime":"99999999999999999999"},
                {"deviceKey":"SYNTHETIC_RAW_PHONE", "deviceName":"Phone", "status":"1",
                 "region":"Example region", "os":"Android", "loginType":2},
                {"deviceKey":"SYNTHETIC_RAW_CURRENT", "deviceName":"This computer", "status":1,
                 "ifLoginDevice":true, "loginPlace":"Example city", "region":"Unselected region",
                 "loginType":"扫码登录", "lastActiveTime":"1720000000000", "os":"Windows"}
            ],
            "userDeviceLimitInfoDTO":{
                "currentNum":"2", "deviceLimit":8,
                "familyLink":"https://example.invalid/?token=SYNTHETIC_PRIVATE",
                "deviceLimitRule":{"15":10}
            }
        }
    });
    let (_root, api, session, client) = world([identity(), reply.into()]);
    session.refresh().await.unwrap();
    let epoch = client.account().unwrap().epoch;
    let wire = serde_json::to_value(client.login_devices(&epoch).await.unwrap()).unwrap();
    assert_eq!(wire["epoch"], epoch);
    let rows = wire["value"]["devices"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0]["name"], "This computer");
    assert_eq!(rows[0]["location"], "Example city");
    assert_eq!(rows[0]["current"], true);
    assert_eq!(rows[0]["signedIn"], true);
    assert_eq!(rows[0]["lastActiveTime"], 1_720_000_000_000u64);
    assert_eq!(rows[1]["name"], "Phone");
    assert_eq!(rows[1]["location"], "Example region");
    assert_eq!(rows[1]["loginType"], "2");
    assert_eq!(rows[1]["signedIn"], true);
    assert_eq!(rows[2]["name"], "Previous computer");
    assert_eq!(rows[2]["signedIn"], false);
    assert!(rows[2]["os"].is_null());
    assert!(rows[2]["lastActiveTime"].is_null());
    assert_ne!(rows[0]["id"], rows[1]["id"]);
    assert_eq!(
        wire["value"]["limits"],
        json!({"currentNum":2,"deviceLimit":8})
    );
    let serialized = wire.to_string();
    for excluded in [
        "SYNTHETIC_RAW",
        "SYNTHETIC_PRIVATE",
        "deviceKey",
        "familyLink",
    ] {
        assert!(!serialized.contains(excluded));
    }
    let calls = api.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1].path, "/api/middle/user/device/list");
    assert_eq!(calls[1].payload, json!({"excStatus":9}));
    assert!(!calls[1].cache);
}

#[tokio::test]
async fn a_valid_empty_list_is_distinct_from_invalid_or_rejected_data() {
    let too_many = Value::Array(
        (0..129)
            .map(|id| json!({"deviceKey":id.to_string()}))
            .collect(),
    );
    let (_root, _, session, client) = world([
        identity(),
        list(json!([])).into(),
        json!({"code":200,"data":{}}).into(),
        list(json!([{"deviceKey":"same"},{"deviceKey":"same"}])).into(),
        list(json!([
            {"deviceKey":"first","ifLoginDevice":true},
            {"deviceKey":"second","ifLoginDevice":"1"}
        ]))
        .into(),
        list(too_many).into(),
        json!({"code":403,"message":"SYNTHETIC_PRIVATE_ERROR"}).into(),
    ]);
    session.refresh().await.unwrap();
    let epoch = client.account().unwrap().epoch;
    let empty = client.login_devices(&epoch).await.unwrap().value;
    assert!(empty.devices.is_empty());
    assert!(empty.limits.is_none());
    for _ in 0..4 {
        assert!(matches!(
            client.login_devices(&epoch).await,
            Err(Error::Response(_))
        ));
    }
    let Err(error) = client.login_devices(&epoch).await else {
        panic!("rejected device list became an empty success");
    };
    assert!(error.is_service_code(403));
    let public = serde_json::to_string(&ClientError::from(error)).unwrap();
    assert!(!public.contains("SYNTHETIC_PRIVATE_ERROR"));
}

#[tokio::test]
async fn rejects_unauthenticated_and_stale_reads_without_publishing_old_devices() {
    let (pending, send) = support::gate();
    let (_root, api, session, client) = world([identity(), pending]);
    let guest = client.account().unwrap().epoch;
    let Err(guest_error) = client.login_devices(&guest).await else {
        panic!("guest account unexpectedly read login devices");
    };
    assert!(guest_error.is_service_code(301));
    assert!(api.calls().is_empty());
    session.refresh().await.unwrap();
    let epoch = client.account().unwrap().epoch;
    assert!(matches!(
        client.login_devices(&guest).await,
        Err(Error::StaleResponse)
    ));
    let read = {
        let client = client.clone();
        tokio::spawn(async move { client.login_devices(&epoch).await })
    };
    api.wait_for_calls(2).await;
    let other = Arc::new(ScriptApi::new([
        json!({"code":200,"profile":{"userId":2,"nickname":"Other account"}}).into(),
        list(json!([])).into(),
    ]));
    session.attach(other.clone()).unwrap();
    session.refresh().await.unwrap();
    send.send(Ok(list(json!([{"deviceKey":"SYNTHETIC_OLD_DEVICE"}]))))
        .unwrap();
    assert!(matches!(read.await.unwrap(), Err(Error::StaleResponse)));
    let current = client.account().unwrap();
    assert_eq!(current.nickname.as_deref(), Some("Other account"));
    assert!(
        client
            .login_devices(&current.epoch)
            .await
            .unwrap()
            .value
            .devices
            .is_empty()
    );
    assert_eq!(other.calls().len(), 2);
}
