//! Local font names only. No font files, account requests, or fingerprint upload.
#[cfg(target_os = "windows")]
mod windows;
use cumulus_core::{
    Error, Result,
    client::{ClientError, MusicClient},
};
use serde::Serialize;
use tauri::{Manager, Runtime};
#[derive(Clone, Debug, Serialize)]
pub struct Family {
    pub name: String,
    pub aliases: Vec<String>,
}
#[derive(Serialize)]
pub struct Catalog {
    epoch: String,
    families: Vec<Family>,
    fallbacks: Vec<String>,
}
#[derive(Default)]
pub(crate) struct Host {
    operation: tokio::sync::Mutex<()>,
}
fn normalize(mut families: Vec<Family>) -> Result<Vec<Family>> {
    if families.len() > 32768 {
        return Err(Error::State("font catalog too large"));
    }
    fn valid(value: &str) -> bool {
        !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
    }
    for family in &mut families {
        family.aliases.retain(|v| valid(v));
        if !valid(&family.name) {
            family.name = family.aliases.first().cloned().unwrap_or_default();
        }
        family.aliases.retain(|v| v != &family.name);
        family.aliases.sort();
        family.aliases.dedup();
    }
    families.retain(|f| valid(&f.name));
    families.sort_by(|a, b| a.name.cmp(&b.name));
    families.dedup_by(|a, b| {
        if a.name != b.name {
            return false;
        }
        b.aliases.append(&mut a.aliases);
        b.aliases.sort();
        b.aliases.dedup();
        true
    });
    Ok(families)
}
fn check<R: Runtime>(app: &tauri::AppHandle<R>, epoch: &str) -> Result<()> {
    if app.state::<MusicClient>().account()?.epoch != epoch {
        return Err(Error::StaleResponse);
    }
    Ok(())
}
#[tauri::command]
pub async fn system_font_catalog<R: Runtime>(
    app: tauri::AppHandle<R>,
    epoch: String,
) -> std::result::Result<Catalog, ClientError> {
    check(&app, &epoch)?;
    let host = app.state::<Host>();
    let _operation = host.operation.try_lock().map_err(|_| Error::MutationBusy)?;
    #[cfg(target_os = "linux")]
    let families = {
        let (send, receive) = tokio::sync::oneshot::channel();
        let worker = app.clone();
        let owner = epoch.clone();
        app.run_on_main_thread(move || {
            let result = (|| -> Result<Vec<Family>> {
                use gtk::prelude::*;
                check(&worker, &owner)?;
                let window = worker
                    .get_webview_window("main")
                    .ok_or(Error::State("font window unavailable"))?;
                let widget = window
                    .gtk_window()
                    .map_err(|_| Error::State("font context unavailable"))?;
                Ok(widget
                    .pango_context()
                    .list_families()
                    .into_iter()
                    .map(|f| Family {
                        name: f.name().to_string(),
                        aliases: Vec::new(),
                    })
                    .collect())
            })();
            let _ = send.send(result);
        })
        .map_err(|_| Error::State("font context unavailable"))?;
        receive
            .await
            .map_err(|_| Error::State("font enumeration stopped"))??
    };
    #[cfg(target_os = "windows")]
    let families = tauri::async_runtime::spawn_blocking(windows::enumerate)
        .await
        .map_err(|_| Error::State("font enumeration stopped"))??;
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    let families: Vec<Family> = return Err(Error::State("system fonts unavailable").into());
    let families = normalize(families)?;
    check(&app, &epoch)?;
    Ok(Catalog {
        epoch,
        families,
        fallbacks: cumulus_core::preferences::desktop_lyrics::font_families(""),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_families_merge_localized_aliases_and_oversized_catalogs_fail() {
        let family = |alias: &str| Family {
            name: "Family".into(),
            aliases: vec![alias.into()],
        };
        let merged =
            normalize(vec![family("Alias B"), family("Alias A"), family("Family")]).unwrap();
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].aliases, ["Alias A", "Alias B"]);
        assert!(normalize(vec![family("Alias"); 32769]).is_err());
        assert!(
            normalize(vec![Family {
                name: "x".repeat(257),
                aliases: vec![]
            }])
            .unwrap()
            .is_empty()
        );
    }
    #[test]
    fn localized_aliases_and_valid_names_survive_without_paths_or_control_characters() {
        let result = normalize(vec![
            Family {
                name: "".into(),
                aliases: vec![
                    "字体\n注入".into(),
                    "字体".into(),
                    "Font".into(),
                    "Font".into(),
                ],
            },
            Family {
                name: "Zed".into(),
                aliases: vec![],
            },
        ])
        .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[1].name, "字体");
        assert_eq!(result[1].aliases, ["Font"]);
    }
}
