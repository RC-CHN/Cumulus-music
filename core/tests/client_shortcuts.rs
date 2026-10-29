use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{ShortcutEdit, ShortcutSettingsCommand as Command},
    },
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
        Box::pin(async { panic!("shortcuts requested music HTTP") })
    }
}
#[test]
fn device_shortcuts_have_account_fences_and_never_construct_audio_or_request_network() {
    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let api = Arc::new(NoNetwork);
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    let client = MusicClient::with_session(session.clone(), registry)
        .with_audio_factory(|| panic!("shortcut settings initialized audio"));
    let initial = client.shortcut_settings_native().unwrap().current.unwrap();
    let epoch = session.token().unwrap().generation.to_string();
    let saved = client
        .shortcut_settings_command(
            &epoch,
            Command::Change {
                expected_revision: initial.revision,
                edit: ShortcutEdit::Global { enabled: false },
            },
        )
        .unwrap()
        .value
        .current
        .unwrap();
    session
        .adopt(api, json!({"userId":20}), &session.token().unwrap())
        .unwrap();
    assert!(matches!(
        client.shortcut_settings_command(&epoch, Command::Repair),
        Err(Error::StaleResponse)
    ));
    let epoch = session.token().unwrap().generation.to_string();
    assert_eq!(
        client
            .shortcut_settings(&epoch)
            .unwrap()
            .value
            .current
            .unwrap(),
        saved
    );
    let reset = client
        .shortcut_settings_command(
            &epoch,
            Command::Change {
                expected_revision: saved.revision.clone(),
                edit: ShortcutEdit::Reset,
            },
        )
        .unwrap()
        .value
        .current
        .unwrap();
    assert!(reset.global_enabled && reset.media_enabled);
    assert!(
        client
            .shortcut_settings_command(
                &epoch,
                Command::Change {
                    expected_revision: saved.revision,
                    edit: ShortcutEdit::Media { enabled: false }
                }
            )
            .is_err()
    );
    assert_eq!(
        client
            .shortcut_settings(&epoch)
            .unwrap()
            .value
            .current
            .unwrap(),
        reset
    );
}
