//! Synthetic probe controls; never included in production IPC.
use cumulus_core::{Error, Result, client::MusicClient, storage::StateStore};
use serde_json::Value;
use tauri::Manager;
pub fn command<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    store: &StateStore,
    epoch: &str,
    value: &Value,
) -> Result<()> {
    match value["kind"].as_str().unwrap() {
        "shortcut_change" => cumulus_desktop::shortcuts::settings::change(
            app,
            epoch,
            serde_json::from_value(value["command"].clone())?,
        )
        .map(|_| ()),
        "shortcut_read" => cumulus_desktop::shortcuts::settings::read(app, epoch).map(|_| ()),
        "shortcut_damage" => {
            let sql = rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap();
            sql.execute("UPDATE state SET value='{broken' WHERE owner='local-device' AND kind='shortcut-settings-v1' AND key='current'",[]).unwrap();
            cumulus_desktop::shortcuts::settings::read(app, epoch).map(|_| ())
        }
        "shortcut_reject" | "shortcut_allow" => {
            let sql = rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap();
            sql.execute_batch(if value["kind"]=="shortcut_reject" {"CREATE TRIGGER reject_shortcut BEFORE INSERT ON state WHEN NEW.kind='shortcut-settings-v1' BEGIN SELECT RAISE(ABORT,'synthetic shortcut setting failure'); END;"} else {"DROP TRIGGER reject_shortcut;"}).unwrap();
            Ok(())
        }
        _ => Err(Error::InvalidInput("unknown shortcut proof control")),
    }
}
pub fn observe<R: tauri::Runtime>(app: &tauri::AppHandle<R>, state: &mut Value) {
    state["shortcuts"] = serde_json::to_value(cumulus_desktop::shortcuts::status(app)).unwrap();
    state["shortcutSettings"] = serde_json::to_value(
        app.state::<MusicClient>()
            .shortcut_settings_native()
            .unwrap(),
    )
    .unwrap();
}
