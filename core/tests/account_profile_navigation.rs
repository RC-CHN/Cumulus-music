mod support;

use cumulus_core::{
    account::{AccountSession, profiles::ProfileRegistry},
    client::MusicClient,
    protocol::http::ApiOrigin,
};
use serde_json::{json, to_value};
use std::sync::Arc;
use support::ScriptApi;

#[test]
fn profile_navigation_is_available_only_for_the_verified_current_account() {
    let directory = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(directory.path(), ApiOrigin::Pc).unwrap();
    let api = Arc::new(ScriptApi::new([]));
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    let client = MusicClient::with_session(session.clone(), registry);
    let shortcut = || to_value(client.account().unwrap()).unwrap();
    assert!(shortcut().get("profileShortcut").is_none());
    session
        .adopt(
            api.clone(),
            json!({"userId":"9007199254740993","nickname":"合成账号",
                   "eventCount":3,"follows":0,"followeds":9007199254740994_u64,
                   "private":"not projected"}),
            &session.token().unwrap(),
        )
        .unwrap();
    assert_eq!(
        shortcut()["profileShortcut"],
        json!({"id":"9007199254740993","notes":3,"following":0,"followers":null})
    );
    assert!(!shortcut().to_string().contains("not projected"));
    session.detach().unwrap();
    assert!(shortcut().get("profileShortcut").is_none());
    assert!(api.calls().is_empty());
}
