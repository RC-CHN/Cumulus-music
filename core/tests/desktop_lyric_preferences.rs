use cumulus_core::{Error, preferences::desktop_lyrics::*, storage::StateStore};
use serde_json::{Value, json};
const OWNER: &str = "local-device";
const KIND: &str = "desktop-lyric-preferences-v1";
#[test]
fn recovered_style_tables_match_executed_official_m81() {
    let vectors: Value =
        serde_json::from_str(include_str!("fixtures/desktop_lyric_styles.json")).unwrap();
    for v in vectors["palettes"].as_array().unwrap() {
        let key = if v["preset"] == "default" {
            json!("red")
        } else {
            v["preset"].clone()
        };
        let preset: Preset = serde_json::from_value(key).unwrap();
        assert_eq!(
            serde_json::to_value(preset.colors()).unwrap(),
            json!({"text":v["text"],"outline":v["outline"]})
        );
    }
    let modes = [
        Alignment::Center,
        Alignment::Split,
        Alignment::Left,
        Alignment::Right,
    ];
    for v in vectors["alignments"].as_array().unwrap() {
        let single = v["single"].as_bool().unwrap();
        assert_eq!(
            json!(modes[v["mode"].as_u64().unwrap() as usize].pair(single)),
            v["calls"][0][1]["value"]
        );
        assert_eq!(json!(single), v["calls"][1][1]["status"]);
    }
    for v in vectors["styles"].as_array().unwrap() {
        let official = &v["style"];
        let style = Appearance {
            font: official["lyricFont"].as_str().unwrap().into(),
            font_size: 36,
            bold: true,
            auxiliary: serde_json::from_value(official["lyricMode"].clone()).unwrap(),
            single_line: official["lyricLineMode"] == 1,
            composition: serde_json::from_value(official["lyricComposition"].clone()).unwrap(),
            single_alignment: Alignment::Left,
            double_alignment: Alignment::Split,
            ..Default::default()
        };
        style.validate().unwrap();
        let calls = v["calls"].as_array().unwrap();
        assert_eq!(json!(style.alignments()), calls[0][1]["value"]);
        assert_eq!(json!(style.single_line), calls[1][1]["status"]);
        assert_eq!(json!(style.always_on_top), calls[2][1]["status"]);
        assert_eq!(serde_json::to_value(style.auxiliary).unwrap(), calls[3][1]);
        assert_eq!(json!(style.palette.colors().text), calls[4][1]["value"]);
        assert_eq!(json!(style.palette.colors().outline), calls[5][1]["value"]);
        assert_eq!(
            json!([style.outline, style.outline, false, false]),
            calls[6][1]["value"]
        );
        assert_eq!(
            json!(style.composition == Composition::Horizontal),
            calls[7][1]["status"]
        );
        assert_eq!(
            json!([
                style.font_size.to_string(),
                u8::from(style.bold).to_string(),
                style.font
            ]),
            calls[8][1]["value"]
        );
        assert_eq!(calls[9], json!(["lock", false]));
    }
    assert_eq!(rgba("ff9f8953").unwrap(), [159, 137, 83, 255]);
    assert_eq!(rgba("809f8953").unwrap(), [159, 137, 83, 128]);
    assert_eq!(rgba("EeEeEe").unwrap(), [238, 238, 238, 255]);
    for bad in ["#ff0000", "red", "12345", "00000000;url(x)", "gggggg"] {
        assert!(rgba(bad).is_err());
    }
}
#[test]
fn settings_are_atomic_across_reopen_and_old_window_revisions() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path()).unwrap();
    let other = StateStore::open(dir.path()).unwrap();
    let first = DesktopLyricPreferences::read(&store).unwrap();
    assert!(!first.enabled);
    assert_eq!(first.appearance.font_size, 36);
    let enabled =
        DesktopLyricPreferences::change(&store, "initial", Edit::Enabled { value: true }).unwrap();
    assert!(
        DesktopLyricPreferences::change(&other, "initial", Edit::Enabled { value: false }).is_err()
    );
    let custom = Colors {
        text: ["010203", "040506", "070809", "0a0b0c"].map(str::to_owned),
        outline: ["ffffff", "000000"].map(str::to_owned),
    };
    let saved = DesktopLyricPreferences::change(
        &store,
        &enabled.revision,
        Edit::CustomColors {
            colors: custom.clone(),
        },
    )
    .unwrap();
    assert_eq!(saved.custom_palette, Some(custom.clone()));
    assert_eq!(saved.appearance.palette.colors(), custom);
    let appearance = Appearance {
        palette: Palette::Preset {
            preset: Preset::Blue,
        },
        ..saved.appearance.clone()
    };
    let preset = DesktopLyricPreferences::change(
        &other,
        &saved.revision,
        Edit::Appearance { value: appearance },
    )
    .unwrap();
    assert_eq!(preset.custom_palette, Some(custom));
    let geometry = Geometry {
        x: -100,
        y: 500,
        width: 900,
        height: 160,
    };
    let moved = DesktopLyricPreferences::change(
        &store,
        &preset.revision,
        Edit::Geometry {
            value: Some(geometry),
        },
    )
    .unwrap();
    drop(store);
    drop(other);
    let reopened = StateStore::open(dir.path()).unwrap();
    assert_eq!(DesktopLyricPreferences::read(&reopened).unwrap(), moved);
    let reset = DesktopLyricPreferences::change(&reopened, &moved.revision, Edit::Reset).unwrap();
    assert!(!reset.enabled && reset.geometry.is_none() && reset.custom_palette.is_none());
    assert_ne!(reset.revision, moved.revision);
}
#[test]
fn invalid_values_and_failed_transactions_leave_the_previous_style_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path()).unwrap();
    let initial = DesktopLyricPreferences::read(&store).unwrap();
    for appearance in [
        Appearance {
            font_size: 0,
            ..Default::default()
        },
        Appearance {
            font: "bad\nfont".into(),
            ..Default::default()
        },
        Appearance {
            palette: Palette::Custom {
                colors: Colors {
                    text: [
                        "red".into(),
                        "000000".into(),
                        "000000".into(),
                        "000000".into(),
                    ],
                    outline: ["ffffff".into(), "000000".into()],
                },
            },
            ..Default::default()
        },
    ] {
        assert!(
            DesktopLyricPreferences::change(
                &store,
                "initial",
                Edit::Appearance { value: appearance }
            )
            .is_err()
        );
        assert_eq!(DesktopLyricPreferences::read(&store).unwrap(), initial);
    }
    assert!(
        DesktopLyricPreferences::change(
            &store,
            "initial",
            Edit::Geometry {
                value: Some(Geometry {
                    x: i32::MIN,
                    y: 0,
                    width: 1000,
                    height: 100
                })
            }
        )
        .is_err()
    );
    let sql = rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap();
    sql.execute_batch("CREATE TRIGGER reject_lyrics BEFORE INSERT ON state WHEN NEW.kind='desktop-lyric-preferences-v1' BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
    assert!(
        DesktopLyricPreferences::change(&store, "initial", Edit::Enabled { value: true }).is_err()
    );
    assert_eq!(DesktopLyricPreferences::read(&store).unwrap(), initial);
}
#[test]
fn damage_and_missing_markers_require_explicit_repair_with_raw_backups() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path()).unwrap();
    let before =
        DesktopLyricPreferences::change(&store, "initial", Edit::Enabled { value: true }).unwrap();
    let sql = rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap();
    sql.execute(
        "UPDATE state SET value='{broken' WHERE kind=? AND key='current'",
        [KIND],
    )
    .unwrap();
    assert!(matches!(
        DesktopLyricPreferences::read(&store),
        Err(Error::Json(_))
    ));
    let repaired = DesktopLyricPreferences::repair(&store).unwrap();
    assert!(!repaired.enabled && repaired.revision != before.revision);
    let backup = store.values(OWNER, "desktop-lyric-recovery-v1").unwrap();
    assert_eq!(backup.values().next().unwrap()["current_raw"], "{broken");
    assert!(DesktopLyricPreferences::repair(&store).is_err());
    store.delete(OWNER, KIND, "saved").unwrap();
    assert!(DesktopLyricPreferences::read(&store).is_err());
    assert!(DesktopLyricPreferences::change(&store, &repaired.revision, Edit::Reset).is_err());
    assert!(!DesktopLyricPreferences::repair(&store).unwrap().enabled);
}
