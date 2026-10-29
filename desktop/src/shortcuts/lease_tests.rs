use super::*;
use cumulus_core::preferences::shortcuts::{Action, Chord, Hotkey, ShortcutSettings};
use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};

fn fixture() -> (tauri::App<MockRuntime>, String) {
    let app = mock_builder().build(mock_context(noop_assets())).unwrap();
    let ledger = RegistrationLedger::new(&ShortcutSettings::default()).unwrap();
    let ticket = ledger.ticket().id();
    app.manage(Host {
        operations: Mutex::new(()),
        inner: Mutex::new(Inner {
            revision: 0,
            settings_revision: Some("initial".into()),
            phase: Phase::Ready,
            ledger: Some(ledger),
            local: BTreeMap::new(),
            local_serial: 0,
            recording: None,
        }),
        sender: Mutex::new(None),
        busy: AtomicBool::new(false),
        stopped: AtomicBool::new(false),
    });
    (app, ticket)
}
#[test]
fn independent_window_leases_cannot_detach_or_impersonate_each_other() {
    let (app, ticket) = fixture();
    let host = app.state::<Host>();
    let main = attach(app.handle(), &host, "main", "1".into(), ticket.clone()).unwrap();
    let mini = attach(
        app.handle(),
        &host,
        "mini-player-1",
        "1".into(),
        ticket.clone(),
    )
    .unwrap();
    let chord = Chord::new(vec![17, 77]).unwrap();
    let key =
        |label, lease: &local::Lease| local_key(&host, label, "1", &lease.id, &ticket, &chord);
    assert_eq!(
        key("main", &main).unwrap(),
        Hotkey::Configured(Action::MiniMode)
    );
    assert_eq!(
        key("mini-player-1", &mini).unwrap(),
        Hotkey::Configured(Action::MiniMode)
    );
    assert!(key("main", &mini).is_err());
    detach(app.handle(), "main", Some(&mini.id));
    key("main", &main).unwrap();
    let replacement = attach(
        app.handle(),
        &host,
        "mini-player-1",
        "1".into(),
        ticket.clone(),
    )
    .unwrap();
    detach(app.handle(), "mini-player-1", Some(&mini.id));
    key("mini-player-1", &replacement).unwrap();
    assert!(key("mini-player-1", &mini).is_err());
    detach(app.handle(), "main", None);
    assert!(key("main", &main).is_err());
    key("mini-player-1", &replacement).unwrap();
    assert!(status(app.handle()).rows.iter().any(|r| r.registration.scope == Scope::Local && r.status == RegistrationStatus::Active));
    detach(app.handle(), "mini-player-1", None);
    assert!(
        status(app.handle())
            .rows
            .iter()
            .filter(|r| r.registration.scope == Scope::Local)
            .all(|r| r.status != RegistrationStatus::Active)
    );
}
#[test]
fn recording_account_epoch_and_retired_settings_reject_both_windows() {
    let (app, ticket) = fixture();
    let host = app.state::<Host>();
    let main = attach(app.handle(), &host, "main", "1".into(), ticket.clone()).unwrap();
    let mini = attach(
        app.handle(),
        &host,
        "mini-player-2",
        "2".into(),
        ticket.clone(),
    )
    .unwrap();
    let chord = Chord::new(vec![17, 77]).unwrap();
    assert!(local_key(&host, "mini-player-2", "1", &mini.id, &ticket, &chord).is_err());
    record(&host).unwrap();
    for (label, epoch, lease) in [("main", "1", &main), ("mini-player-2", "2", &mini)] {
        assert!(local_key(&host, label, epoch, &lease.id, &ticket, &chord).is_err());
    }
    retire(app.handle(), Phase::Damaged);
    let mut state = host.inner.lock().unwrap();
    assert!(state.local.is_empty());
    state.ledger = Some(RegistrationLedger::new(&Default::default()).unwrap());
    state.phase = Phase::Ready;
    let newer = state.ledger.as_ref().unwrap().ticket().id();
    drop(state);
    assert_ne!(ticket, newer);
    assert!(attach(app.handle(), &host, "main", "1".into(), ticket.clone()).is_err());
    let current = attach(app.handle(), &host, "main", "1".into(), newer.clone()).unwrap();
    assert!(local_key(&host, "main", "1", &main.id, &ticket, &chord).is_err());
    local_key(&host, "main", "1", &current.id, &newer, &chord).unwrap();
}
