use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::WindowPreferencesCommand},
    preferences::window::{CloseBehavior, WindowSettings},
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
        Box::pin(async { panic!("window preferences must not call music APIs") })
    }
}
#[test]
fn device_close_settings_reject_old_accounts_and_stale_windows_without_initializing_players() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let api = Arc::new(NoNetwork);
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    let client = MusicClient::with_session(session.clone(), registry)
        .with_audio_factory(|| panic!("preferences must not initialize audio"));
    let epoch = session.token().unwrap().generation.to_string();
    let initial = client.window_preferences(&epoch).unwrap();
    assert_eq!(initial.value.revision.as_deref(), Some("0"));
    let settings = WindowSettings {
        close_behavior: CloseBehavior::Exit,
        remember_choice: true,
    };
    let saved = client
        .window_preferences_command(
            &epoch,
            WindowPreferencesCommand::Change {
                expected_revision: "0".into(),
                settings,
            },
        )
        .unwrap();
    assert_eq!(saved.value.settings, settings);
    assert!(
        client
            .window_preferences_command(
                &epoch,
                WindowPreferencesCommand::Change {
                    expected_revision: "0".into(),
                    settings: WindowSettings::default()
                }
            )
            .is_err()
    );
    session
        .adopt(api, json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    assert!(matches!(
        client.window_preferences_command(&epoch, WindowPreferencesCommand::Repair),
        Err(Error::StaleResponse)
    ));
    let newer = session.token().unwrap().generation.to_string();
    assert_eq!(
        client.window_preferences(&newer).unwrap().value.settings,
        settings
    );
}
