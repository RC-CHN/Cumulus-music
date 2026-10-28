use cumulus_core::{Error, preferences::window::*, storage::StateStore};
use serde_json::json;
#[test]
fn official_default_asks_and_remembered_tray_cannot_hide_without_a_host() {
    let settings = WindowSettings::default();
    assert_eq!(settings.close_behavior, CloseBehavior::Tray);
    assert!(!settings.remember_choice);
    assert_eq!(settings.plan(true), ClosePlan::Ask);
    assert_eq!(settings.plan(false), ClosePlan::Ask);
    let remembered = WindowSettings {
        remember_choice: true,
        ..settings
    };
    assert_eq!(remembered.plan(true), ClosePlan::Hide);
    assert_eq!(remembered.plan(false), ClosePlan::Ask);
    let exit = WindowSettings {
        close_behavior: CloseBehavior::Exit,
        ..remembered
    };
    assert_eq!(exit.plan(false), ClosePlan::Exit);
}
#[test]
fn close_policy_persists_on_device_and_competing_windows_do_not_overwrite_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path()).unwrap();
    let policy = WindowSettings {
        close_behavior: CloseBehavior::Exit,
        remember_choice: true,
    };
    let saved = WindowPreferences::replace(&store, 0, policy).unwrap();
    drop(store);
    let reopened = StateStore::open(dir.path()).unwrap();
    assert_eq!(WindowPreferences::read(&reopened).unwrap(), saved);
    assert!(WindowPreferences::replace(&reopened, 0, WindowSettings::default()).is_err());
    assert!(
        reopened
            .get("account-a", "window-behavior-preferences-v1", "current")
            .unwrap()
            .is_none()
    );
}
#[test]
fn missing_or_corrupt_policy_requires_explicit_repair_without_revision_reuse() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path()).unwrap();
    let saved = WindowPreferences::replace(&store, 0, WindowSettings::default()).unwrap();
    store
        .delete("local-device", "window-behavior-preferences-v1", "current")
        .unwrap();
    assert!(matches!(
        WindowPreferences::read(&store),
        Err(Error::Json(_))
    ));
    assert!(WindowPreferences::replace(&store, 0, WindowSettings::default()).is_err());
    let repaired = WindowPreferences::repair(&store).unwrap();
    assert!(repaired.revision > saved.revision);
    assert!(!repaired.settings.remember_choice);
    store.put("local-device","window-behavior-preferences-v1","current",&json!({"revision":repaired.revision,"settings":{"closeBehavior":"unknown","rememberChoice":true}})).unwrap();
    assert!(WindowPreferences::read(&store).is_err());
    let next = WindowPreferences::repair(&store).unwrap();
    assert!(next.revision > repaired.revision);
    assert_eq!(
        store
            .values("local-device", "window-behavior-recovery-v1")
            .unwrap()
            .len(),
        2
    );
}
#[test]
fn unknown_fields_and_failed_storage_do_not_become_remembered_exit() {
    assert!(
        serde_json::from_value::<WindowSettings>(
            json!({"closeBehavior":"exit","rememberChoice":true,"extra":true})
        )
        .is_err()
    );
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path()).unwrap();
    let saved = WindowPreferences::replace(&store, 0, WindowSettings::default()).unwrap();
    store
        .put(
            "local-device",
            "window-behavior-preferences-v1",
            "revision",
            &json!(u64::MAX),
        )
        .unwrap();
    assert!(
        WindowPreferences::replace(
            &store,
            saved.revision,
            WindowSettings {
                close_behavior: CloseBehavior::Exit,
                remember_choice: true
            }
        )
        .is_err()
    );
    assert_eq!(WindowPreferences::read(&store).unwrap(), saved);
}
