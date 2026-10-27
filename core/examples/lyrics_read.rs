//! Explicit read-only Rust integration probe. Emits statistics, not lyric text.
use cumulus_core::{
    Error, Result,
    account::{AccountApi, AccountSession, credentials::Credentials},
    cloud::CloudApi,
    lyrics::{
        LyricDemand, LyricRequest, LyricSnapshot, LyricTarget, LyricsService, parse_yrc, render_lrc,
    },
    protocol::api::identifier,
    protocol::http::{ApiOrigin, ClientProfile, MusicHttpClient},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};

fn request(target: LyricTarget) -> LyricRequest {
    LyricRequest {
        target,
        demand: LyricDemand {
            force: true,
            ..Default::default()
        },
        downloaded_audio: None,
        track_mv: false,
    }
}
fn summary(view: LyricSnapshot) -> Result<Value> {
    let parsed = view.lyric;
    let rendered = render_lrc(&parsed.lyric_lines)?;
    let yrc_lines = parsed
        .yrc_info
        .yrc
        .as_ref()
        .map(|s| parse_yrc(s).lyric_lines.len());
    Ok(
        json!({"ok":!view.is_lyric_fetch_failed&&!view.network_failed,"source":parsed.current_used_lyric,"lines":parsed.lyric_lines.len(),"translation_lines":parsed.tlyric_lines.len(),"roma_lines":parsed.roma_lyric_lines.len(),"yrc_lines":yrc_lines,"scrollable":parsed.scrollable,"export_bytes":rendered.len(),"network_failed":view.network_failed,"from_cache":view.from_cache}),
    )
}

async fn run() -> Result<Value> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err(Error::InvalidInput(
            "usage: lyrics_read <private-profile-directory> <pc|interface>",
        ));
    }
    let origin = match args[1].as_str() {
        "pc" => ApiOrigin::Pc,
        "interface" => ApiOrigin::Interface,
        _ => return Err(Error::InvalidInput("unsupported origin")),
    };
    let path = PathBuf::from(&args[0]);
    if !path.join("core.sqlite3").is_file() {
        return Err(Error::InvalidInput("existing private profile required"));
    }
    let store = Arc::new(StateStore::open(path)?);
    let credentials = Arc::new(Credentials::open(store.clone(), origin)?);
    let client = Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?);
    let session = Arc::new(AccountSession::new(client, Some(store)));
    session.refresh().await?;
    let (_, ids) = session
        .run(|api, uid| async move { AccountApi::new(api).liked_ids(&json!(uid)).await })
        .await?;
    let mut checks = Vec::new();
    let first_id = ids.first().cloned();
    let service = LyricsService::new(session.clone())?;
    for id in ids.into_iter().take(5) {
        let result = service
            .fetch(request(LyricTarget::Song(id)))
            .await
            .and_then(summary);
        checks.push(match result {
            Ok(value) => value,
            Err(error) => json!({"ok":false,"error":error.to_string()}),
        });
    }
    if let Some(id) = first_id {
        let reopened = LyricsService::new(session.clone())?;
        let mut value = match reopened
            .fetch(request(LyricTarget::Song(id)))
            .await
            .and_then(summary)
        {
            Ok(value) => value,
            Err(error) => json!({"ok":false,"error":error.to_string()}),
        };
        value["case"] = json!("reopen_cached_version_request");
        checks.push(value);
    }
    let (_, cloud) = session
        .run(|api, _| async move { CloudApi::new(api).list().await })
        .await?;
    if let Some(row) = cloud.first() {
        let id = identifier(&row["songId"])?;
        let mut value = match service
            .fetch(request(LyricTarget::Cloud(id)))
            .await
            .and_then(summary)
        {
            Ok(value) => value,
            Err(error) => json!({"ok":false,"error":error.to_string()}),
        };
        value["case"] = json!("cloud_lyric");
        checks.push(value);
    }
    Ok(json!({"schema":1,"implementation":"rust-core","remote_mutations":0,"checks":checks}))
}

#[tokio::main]
async fn main() {
    match run().await {
        Ok(report) => {
            println!("{}", serde_json::to_string_pretty(&report).expect("report"));
            if report["checks"]
                .as_array()
                .is_none_or(|c| c.is_empty() || c.iter().any(|v| v["ok"] != true))
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
