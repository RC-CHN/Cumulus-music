use cumulus_core::preferences::shortcuts::{
    Action, Capture, Chord, Hotkey, KeyInput, Scope, ShortcutSettings,
};
use serde_json::Value;

#[test]
fn recovered_editor_vectors_match_with_explicit_super_and_close_exceptions() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/shortcuts-official.json")).unwrap();
    let mut differences = 0;
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 8192);
    for row in cases {
        let key = row[0].as_u64().unwrap() as u16;
        let flags = row[1].as_u64().unwrap();
        let input = KeyInput {
            key_code: key,
            ctrl_key: flags & 1 != 0,
            shift_key: flags & 2 != 0,
            alt_key: flags & 4 != 0,
            meta_key: flags & 8 != 0,
        };
        let mut expected: Option<Vec<u16>> = serde_json::from_value(row[3].clone()).unwrap();
        if let Some(keys) = &mut expected {
            keys.sort_unstable();
        }
        let actual = match input.capture(row[2].as_bool().unwrap()) {
            Capture::Ignore => None,
            Capture::Clear => Some(vec![]),
            Capture::Set(chord) => Some(chord.keys().to_vec()),
        };
        if input.meta_key || expected.as_deref() == Some(&[18, 115]) {
            assert_eq!(actual, None, "documented cross-platform exception: {row}");
            if expected.is_some() {
                differences += 1;
            }
        } else {
            assert_eq!(actual, expected, "official editor vector {row}");
        }
    }
    assert_eq!(differences, 1310);
    eprintln!(
        "8192 executed official editor vectors; {differences} explicit Meta/Alt+F4 differences"
    );
}
#[test]
fn defaults_and_registration_switches_preserve_the_official_three_groups() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/shortcuts-official.json")).unwrap();
    let mut settings = ShortcutSettings::default();
    for row in fixture["defaults"].as_array().unwrap() {
        let action: Action = serde_json::from_value(row["name"].clone()).unwrap();
        let binding = &settings.bindings[&action];
        for (chord, key) in [(&binding.local, "local"), (&binding.global, "global")] {
            let mut expected: Vec<u16> = serde_json::from_value(row[key].clone()).unwrap();
            expected.sort_unstable();
            assert_eq!(chord.as_ref().unwrap().keys(), expected);
        }
    }
    assert_eq!(settings.registration_plan().unwrap().len(), 23);
    settings.global_enabled = false;
    let plan = settings.registration_plan().unwrap();
    assert_eq!(plan.len(), 15);
    assert_eq!(plan.iter().filter(|r| r.scope == Scope::Global).count(), 4);
    assert!(
        plan.iter()
            .any(|r| r.hotkey == Hotkey::MediaStop && r.chord.keys() == [178])
    );
    settings.media_enabled = false;
    assert_eq!(settings.registration_plan().unwrap().len(), 11);
    settings.global_enabled = true;
    assert_eq!(settings.registration_plan().unwrap().len(), 19);
}
#[test]
fn conflicts_include_disabled_global_and_the_same_action_in_the_other_scope() {
    let mut settings = ShortcutSettings {
        global_enabled: false,
        ..ShortcutSettings::default()
    };
    let before = settings.clone();
    for scope in [Scope::Local, Scope::Global] {
        assert!(
            settings
                .bind(
                    Action::Next,
                    scope,
                    Some(Chord::new(vec![18, 80, 17]).unwrap())
                )
                .is_err()
        );
    }
    assert!(
        settings
            .bind(
                Action::PlayPause,
                Scope::Global,
                Some(Chord::new(vec![17, 80]).unwrap())
            )
            .is_err()
    );
    assert_eq!(before, settings);
    settings
        .bind(
            Action::PlayPause,
            Scope::Local,
            Some(Chord::new(vec![80, 17]).unwrap()),
        )
        .unwrap();
    assert_eq!(before, settings);
    settings
        .bind(Action::PlayPause, Scope::Global, None)
        .unwrap();
    settings
        .bind(
            Action::Next,
            Scope::Local,
            Some(Chord::new(vec![18, 80, 17]).unwrap()),
        )
        .unwrap();
    assert!(Chord::new(vec![17, 17, 80]).is_err());
    assert!(Chord::new(vec![17, 18]).is_err());
    assert!(Chord::new(vec![80, 81]).is_err());
    assert!(
        settings
            .bind(
                Action::Next,
                Scope::Local,
                Some(Chord::new(vec![116]).unwrap())
            )
            .is_err()
    );
}
#[test]
fn translation_gate_preserves_user_edits_and_does_not_take_an_occupied_key() {
    let mut settings = ShortcutSettings::default();
    settings
        .bind(
            Action::Next,
            Scope::Global,
            Some(Chord::new(vec![17, 84]).unwrap()),
        )
        .unwrap();
    settings.translation_gate(true);
    assert!(settings.bindings[&Action::Translate].local.is_none());
    settings
        .bind(
            Action::Translate,
            Scope::Local,
            Some(Chord::new(vec![17, 89]).unwrap()),
        )
        .unwrap();
    settings.translation_gate(true);
    assert_eq!(
        settings.bindings[&Action::Translate]
            .local
            .as_ref()
            .unwrap()
            .keys(),
        [17, 89]
    );
    settings.translation_gate(false);
    assert!(!settings.bindings.contains_key(&Action::Translate));
    settings.bind(Action::Next, Scope::Global, None).unwrap();
    settings.translation_gate(true);
    assert_eq!(
        settings.bindings[&Action::Translate]
            .local
            .as_ref()
            .unwrap()
            .keys(),
        [17, 84]
    );
}

#[test]
fn default_reset_keeps_gated_rows_and_reports_conflicts_without_partial_changes() {
    let mut settings = ShortcutSettings::default();
    settings.translation_gate(true);
    settings.global_enabled = false;
    settings.reset_defaults().unwrap();
    assert!(settings.global_enabled);
    assert_eq!(
        settings.bindings[&Action::Translate]
            .local
            .as_ref()
            .unwrap()
            .keys(),
        [17, 84]
    );
    settings
        .bind(Action::PlayPause, Scope::Local, None)
        .unwrap();
    settings
        .bind(
            Action::Translate,
            Scope::Local,
            Some(Chord::new(vec![17, 80]).unwrap()),
        )
        .unwrap();
    let prior = settings.clone();
    assert!(settings.reset_defaults().is_err());
    assert_eq!(settings, prior);
}
