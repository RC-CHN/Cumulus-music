use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::MiniPreferencesCommand as Command},
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::sync::Arc;
struct NoNetwork;
impl Api for NoNetwork {
    fn request<'a>(&'a self, _: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async { panic!("device setting cannot call music APIs") })
    }
}
#[test]
fn guest_account_switch_and_damage_are_fenced_without_starting_a_player() {
    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let store = registry.store();
    let api = Arc::new(NoNetwork);
    let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
    let client = MusicClient::with_session(session.clone(), registry)
        .with_audio_factory(|| panic!("preference read initialized a decoder"));
    assert!(client.mini_preferences_native().unwrap().always_on_top);
    let epoch = session.token().unwrap().generation.to_string();
    let saved = client
        .mini_preferences_command(
            &epoch,
            Command::Change {
                expected_revision: "initial".into(),
                always_on_top: false,
            },
        )
        .unwrap();
    session
        .adopt(api, json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    assert!(matches!(
        client.mini_preferences_command(&epoch, Command::Repair),
        Err(Error::StaleResponse)
    ));
    let newer = session.token().unwrap().generation.to_string();
    assert_eq!(client.mini_preferences(&newer).unwrap().value, saved.value);
    store
        .put(
            "local-device",
            "mini-preferences-v1",
            "current",
            &json!({"always_on_top":true}),
        )
        .unwrap();
    let damaged = client.mini_preferences(&newer).unwrap().value;
    assert!(damaged.damaged && !damaged.always_on_top && damaged.revision.is_none());
    let restored = client
        .mini_preferences_command(&newer, Command::Repair)
        .unwrap()
        .value;
    assert!(!restored.damaged && restored.always_on_top);
    assert_ne!(restored.revision, saved.value.revision);
}
