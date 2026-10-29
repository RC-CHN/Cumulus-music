use cumulus_core::preferences::shortcuts::{
    Action, Hotkey, RegistrationLedger, RegistrationStatus as Status, Scope, ShortcutSettings,
};
#[test]
fn registration_requires_actual_acknowledgement_and_retires_every_old_callback() {
    let mut settings = ShortcutSettings::default();
    let mut ledger = RegistrationLedger::new(&settings).unwrap();
    let ticket = ledger.ticket();
    let key = Hotkey::Configured(Action::PlayPause);
    assert!(!ledger.active(&ticket, key, Scope::Global));
    assert!(ledger.acknowledge(&ticket, key, Scope::Global, Status::Conflict { code: 1409 }));
    assert!(!ledger.active(&ticket, key, Scope::Global));
    assert!(ledger.acknowledge(&ticket, key, Scope::Local, Status::Active));
    assert!(ledger.active(&ticket, key, Scope::Local));
    settings.global_enabled = false;
    ledger.replace(&settings).unwrap();
    assert!(!ledger.acknowledge(&ticket, key, Scope::Local, Status::Active));
    assert!(!ledger.active(&ticket, key, Scope::Local));
    let disabled = ledger.ticket();
    assert!(!ledger.acknowledge(&disabled, key, Scope::Global, Status::Active));
    assert!(ledger.acknowledge(
        &disabled,
        Hotkey::MediaStop,
        Scope::Global,
        Status::Unavailable { code: 1 }
    ));
    settings.global_enabled = true;
    ledger.replace(&settings).unwrap();
    let enabled = ledger.ticket();
    assert_ne!(enabled, disabled);
    assert!(!ledger.acknowledge(&disabled, key, Scope::Global, Status::Active));
    assert!(ledger.acknowledge(&enabled, key, Scope::Global, Status::Active));
    assert!(ledger.active(&enabled, key, Scope::Global));
    settings.bindings.remove(&Action::Next);
    assert!(ledger.replace(&settings).is_err());
    assert!(ledger.active(&enabled, key, Scope::Global));
    assert!(
        !RegistrationLedger::new(&ShortcutSettings::default())
            .unwrap()
            .active(&enabled, key, Scope::Global)
    );
}
