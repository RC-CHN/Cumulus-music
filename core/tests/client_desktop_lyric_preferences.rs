use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::DesktopLyricPreferencesCommand as Command},
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
fn desktop_policy_needs_no_player_and_old_accounts_cannot_enable_or_repair_it() {
    use cumulus_core::preferences::desktop_lyrics::Edit;
    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let store = registry.store();
    let api = Arc::new(NoNetwork);
    let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
    let client = MusicClient::with_session(session.clone(), registry)
        .with_audio_factory(|| panic!("desktop lyric policy initialized audio"));
    let epoch = session.token().unwrap().generation.to_string();
    assert!(
        !client
            .desktop_lyric_preferences_native()
            .unwrap()
            .current
            .unwrap()
            .enabled
    );
    let saved = client
        .desktop_lyric_preferences_command(
            &epoch,
            Command::Change {
                expected_revision: "initial".into(),
                edit: Box::new(Edit::Enabled { value: true }),
            },
        )
        .unwrap();
    session
        .adopt(api, json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    assert!(matches!(
        client.desktop_lyric_preferences_command(&epoch, Command::Repair),
        Err(Error::StaleResponse)
    ));
    let newer = session.token().unwrap().generation.to_string();
    assert_eq!(
        client.desktop_lyric_preferences(&newer).unwrap().value,
        saved.value
    );
    store
        .put(
            "local-device",
            "desktop-lyric-preferences-v1",
            "current",
            &json!({"enabled":true}),
        )
        .unwrap();
    let damaged = client.desktop_lyric_preferences(&newer).unwrap().value;
    assert!(damaged.damaged && damaged.current.is_none());
    let repaired = client
        .desktop_lyric_preferences_command(&newer, Command::Repair)
        .unwrap()
        .value;
    assert!(!repaired.damaged && !repaired.current.unwrap().enabled);
}
