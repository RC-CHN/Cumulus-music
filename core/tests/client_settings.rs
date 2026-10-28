mod support;
use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::*},
    protocol::http::ApiOrigin,
};
use serde_json::json;
use std::sync::Arc;
use support::ScriptApi;
#[tokio::test]
async fn whitelist_projection_never_exposes_raw_setting_fields_or_silently_defaults() {
    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let api=Arc::new(ScriptApi::new([json!({"code":200,"profile":{"userId":1}}).into(),json!({"code":200,"setting":{"shareSetting":1,"playRecordSetting":99,"personalServiceSetting":"0","allowLikedNotify":false,"allowPeopleCanSeeMyPlayRecord":true,"allowFollowedCanSeeMyPlayRecord":false,"socialPlaylistSetting":7,"privateToken":"SYNTHETIC_SECRET","url":"https://example.invalid/?authorization=SECRET"}}).into()]));
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session.refresh().await.unwrap();
    let client = MusicClient::with_session(session, registry);
    let epoch = client.account().unwrap().epoch;
    let blank = client.account_settings_page(&epoch, false).await.unwrap();
    assert!(blank.value.revision.is_none());
    let result = client.account_settings_page(&epoch, true).await.unwrap();
    let wire = serde_json::to_value(result).unwrap();
    let text = wire.to_string();
    for secret in ["SECRET", "authorization", "privateToken", "url"] {
        assert!(!text.contains(secret));
    }
    let fields = wire["value"]["fields"].as_array().unwrap();
    assert_eq!(
        fields[0],
        json!({"key":"shareSetting","value":1,"state":"available"})
    );
    assert_eq!(fields[1]["state"], "unsupported");
    assert!(fields[1]["value"].is_null());
    assert_eq!(fields[3]["state"], "missing");
    assert!(fields[3]["value"].is_null());
    assert!(wire["value"]["linkedPrivacy"]["socialPlaylistSetting"].is_null());
    assert!(matches!(
        client.account_settings_page("0", true).await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        client
            .account_settings_command(
                "0",
                AccountSettingsCommand::Change {
                    id: uuid::Uuid::new_v4().to_string(),
                    expected_revision: "a".repeat(64),
                    change: SettingChange {
                        key: SettingKey::ShareSetting,
                        value: SettingValue::Number(0)
                    }
                }
            )
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(api.calls().len(), 2);
}

#[tokio::test]
async fn old_intent_recovery_is_included_even_when_newer_operations_fill_the_page() {
    use cumulus_core::account::settings::SettingsMutation;
    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let api = Arc::new(ScriptApi::new([
        json!({"code":200,"profile":{"userId":1}}).into()
    ]));
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session.refresh().await.unwrap();
    let change = SettingChange {
        key: SettingKey::ShareSetting,
        value: SettingValue::Number(1),
    };
    for i in 1..=25 {
        let id = uuid::Uuid::from_u128(i).to_string();
        let op = SettingsMutation {
            id: id.clone(),
            change: change.clone(),
            expected_revision: "a".repeat(64),
            before: None,
            phase: SettingsPhase::Cancelled,
            created_at: i as u64,
            last_read_at: None,
            read_revision: None,
            observed: None,
            service_code: None,
        };
        registry
            .store()
            .put(
                "1",
                "account-settings-intents-v1",
                &id,
                &serde_json::to_value(op).unwrap(),
            )
            .unwrap();
    }
    let client = MusicClient::with_session(session, registry);
    let epoch = client.account().unwrap().epoch;
    let id = uuid::Uuid::from_u128(1).to_string();
    let page = client.account_settings_page(&epoch, false).await.unwrap();
    assert_eq!(page.value.operations.len(), 20);
    assert!(!page.value.operations.iter().any(|op| op.id == id));
    let result = client
        .account_settings_command(
            &epoch,
            AccountSettingsCommand::CancelUnseen {
                id: id.clone(),
                expected_revision: "a".repeat(64),
                change,
            },
        )
        .await
        .unwrap();
    assert_eq!(result.value.operations.len(), 20);
    assert_eq!(
        result
            .value
            .operations
            .iter()
            .find(|op| op.id == id)
            .unwrap()
            .phase,
        SettingsPhase::Cancelled
    );
    assert_eq!(api.calls().len(), 1);
}
