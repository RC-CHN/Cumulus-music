//! Explicit native integration probe; never run by `cargo test`.
//! Uses Rust HTTPS and domain services. Prints counts/status only, no credentials.
use cumulus_core::{
    Error, Result,
    account::{
        AccountApi, AccountSession, credential_import::LegacySession, credentials::Credentials,
    },
    catalog::{CatalogApi, PlayOptions, SearchOptions},
    cloud::CloudApi,
    membership::MembershipApi,
    playlist::PlaylistApi,
    protocol::{
        api::Api,
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{future::Future, path::PathBuf, sync::Arc};

struct Options {
    profile: PathBuf,
    origin: ApiOrigin,
    import: Option<PathBuf>,
}

fn options() -> Result<Options> {
    let mut args = std::env::args().skip(1);
    let (mut profile, mut origin, mut import) = (None, None, None);
    while let Some(arg) = args.next() {
        let value = args
            .next()
            .ok_or(Error::InvalidInput("missing option value"))?;
        match arg.as_str() {
            "--profile" if profile.is_none() => profile = Some(PathBuf::from(value)),
            "--origin" if origin.is_none() => {
                origin = Some(match value.as_str() {
                    "pc" => ApiOrigin::Pc,
                    "interface" => ApiOrigin::Interface,
                    _ => return Err(Error::InvalidInput("origin must be pc or interface")),
                })
            }
            "--import-legacy" if import.is_none() => import = Some(PathBuf::from(value)),
            _ => return Err(Error::InvalidInput("unknown or duplicate option")),
        }
    }
    Ok(Options {
        profile: profile.ok_or(Error::InvalidInput("--profile is required"))?,
        origin: origin.ok_or(Error::InvalidInput("--origin is required"))?,
        import,
    })
}

async fn check<T, F, Fut, S>(
    session: &AccountSession,
    report: &mut Value,
    name: &str,
    action: F,
    summary: S,
) -> Option<T>
where
    F: FnOnce(Arc<dyn Api>, String) -> Fut,
    Fut: Future<Output = Result<T>>,
    S: FnOnce(&T) -> Value,
{
    match session.run(action).await {
        Ok((_, value)) => {
            report["checks"][name] = json!({"ok":true,"summary":summary(&value)});
            Some(value)
        }
        Err(error) => {
            report["checks"][name] = json!({"ok":false,"error":error.to_string()});
            None
        }
    }
}

async fn run() -> Result<Value> {
    let options = options()?;
    let store = Arc::new(StateStore::open(&options.profile)?);
    let credentials = Arc::new(Credentials::open(store.clone(), options.origin)?);
    if let Some(source) = &options.import {
        credentials.import_legacy(LegacySession::read(source, options.origin)?)?;
    }
    let client = Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?);
    let session = AccountSession::new(client.clone(), Some(store));
    let mut report = json!({"schema":1,"implementation":"rust-core","origin":options.origin,"remote_mutations":0,"checks":{}});
    match session.refresh().await {
        Ok(_) => report["checks"]["identity"] = json!({"ok":true}),
        Err(error) => {
            report["checks"]["identity"] = json!({"ok":false,"error":error.to_string()});
            return Ok(report);
        }
    }
    check(
        &session,
        &mut report,
        "membership",
        |api, uid| async move { MembershipApi::new(api).info(Some(&json!(uid))).await },
        |data| json!({"fields":data.as_object().map(|o|o.len())}),
    )
    .await;
    let playlists = check(
        &session,
        &mut report,
        "playlists",
        |api, uid| async move { PlaylistApi::new(api).list(&json!(uid)).await },
        |rows| json!({"count":rows.len()}),
    )
    .await;
    let liked = check(
        &session,
        &mut report,
        "liked_songs",
        |api, uid| async move { AccountApi::new(api).liked_ids(&json!(uid)).await },
        |rows| json!({"count":rows.len()}),
    )
    .await;
    check(
        &session,
        &mut report,
        "collected_albums",
        |api, _| async move { AccountApi::new(api).collected_albums().await },
        |rows| json!({"count":rows.len()}),
    )
    .await;
    check(
        &session,
        &mut report,
        "collected_artists",
        |api, _| async move { AccountApi::new(api).collected_artists().await },
        |rows| json!({"count":rows.len()}),
    )
    .await;
    check(
        &session,
        &mut report,
        "settings",
        |api, _| async move { AccountApi::new(api).settings().await },
        |data| json!({"fields":data.as_object().map(|o|o.len())}),
    )
    .await;
    check(
        &session,
        &mut report,
        "cloud",
        |api, _| async move { CloudApi::new(api).list().await },
        |rows| json!({"count":rows.len()}),
    )
    .await;
    check(
        &session,
        &mut report,
        "search",
        |api, _| async move {
            CatalogApi::new(api)
                .search(
                    "音乐",
                    SearchOptions {
                        limit: 5,
                        ..Default::default()
                    },
                )
                .await
        },
        |data| json!({"songs":data["result"]["songs"].as_array().map(|v|v.len())}),
    )
    .await;
    check(
        &session,
        &mut report,
        "daily_recommendations",
        |api, _| async move { CatalogApi::new(api).daily().await },
        |data| json!({"songs":data["data"]["dailySongs"].as_array().map(|v|v.len())}),
    )
    .await;
    if let Some(pid) = playlists
        .as_ref()
        .and_then(|v| v.first())
        .map(|v| v["id"].clone())
    {
        check(&session,&mut report,"playlist_tracks",|api,_| async move {
            PlaylistApi::new(api).tracks(&pid).await
        },|data|json!({"songs":data.songs.songs.len(),"missing":data.songs.missing.len(),"local_tracks":data.local_tracks.len()})).await;
    }
    if let Some(sid) = liked.as_ref().and_then(|v| v.first()).map(|s| json!(s)) {
        let id = sid.clone();
        check(&session,&mut report,"lyrics",|api,_| async move {
            CatalogApi::new(api).lyric(&id).await
        },|data|json!({"lrc_present":data["lrc"]["lyric"].is_string(),"yrc_present":data["yrc"]["lyric"].is_string()})).await;
        let id = sid.clone();
        check(&session,&mut report,"lossless_play_resource",|api,_| async move {
            CatalogApi::new(api).play_url(&id,PlayOptions {level:"lossless",..Default::default()}).await
        },|data|json!({"resource_found":data.is_some(),"url_present":data.as_ref().is_some_and(|d|d["url"].as_str().is_some_and(|s|!s.is_empty())),"returned_level":data.as_ref().and_then(|d|d["level"].as_str()),"trial":data.as_ref().is_some_and(|d|!d["freeTrialInfo"].is_null())})).await;
        check(&session,&mut report,"lossless_download_resource",|api,_| async move {
            CatalogApi::new(api).download_url(&sid,"lossless").await
        },|data|json!({"resource_found":data.is_some(),"url_present":data.as_ref().is_some_and(|d|d["url"].as_str().is_some_and(|s|!s.is_empty())),"returned_level":data.as_ref().and_then(|d|d["level"].as_str())})).await;
    }
    Ok(report)
}

#[tokio::main]
async fn main() {
    match run().await {
        Ok(report) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&report).expect("JSON report")
            );
            if report["checks"]
                .as_object()
                .is_some_and(|checks| checks.values().any(|c| c["ok"] != true))
            {
                std::process::exit(1);
            }
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}
