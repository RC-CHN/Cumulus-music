//! Explicit Rust service verification for membership, center and recommendation history.
use cumulus_core::{
    Error, Result,
    account::{AccountSession, credentials::Credentials},
    membership::{MembershipManager, center::MemberCenter},
    protocol::http::{ApiOrigin, ClientProfile, MusicHttpClient},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};

async fn run() -> Result<Value> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err(Error::InvalidInput(
            "usage: membership_read <private-profile-directory> <pc|interface>",
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
    let manager = MembershipManager::new(session.clone())?;
    let center = MemberCenter::new(session)?;
    let mut report =
        json!({"schema":1,"implementation":"rust-core","remote_mutations":0,"checks":{}});
    match manager.refresh().await {
        Ok(snapshot) => {
            report["checks"]["entitlements"] = json!({"ok":true,"vinyl":snapshot.state.is_vinyl_vip,"vinyl_level":snapshot.state.vinyl_vip_level,"plus":snapshot.state.is_plus_vip,"music_package":snapshot.state.is_music_package,"album_vip":snapshot.state.is_album_vip,"voice_book_vip":snapshot.state.is_voice_book_vip})
        }
        Err(error) => {
            report["checks"]["entitlements"] = json!({"ok":false,"error":error.to_string()})
        }
    }
    match center.refresh().await {
        Ok(value) => {
            report["checks"]["member_center"] = json!({"ok":true,"module_types":value["moduleTypes"],"loaded_modules":value["results"].as_array().map(|v|v.len()),"carousel_items":value["carouselList"]["carousels"].as_array().or_else(||value["carouselList"].as_array()).map(|v|v.len())})
        }
        Err(error) => {
            report["checks"]["member_center"] = json!({"ok":false,"error":error.to_string()})
        }
    }
    match center.history(None).await {
        Ok(dates) => {
            report["checks"]["history_dates"] = json!({"ok":true,"count":dates.len()});
            if let Some(date) = dates.first() {
                match center.history(Some(date.clone())).await {
                    Ok(songs) => {
                        report["checks"]["history_detail"] = json!({"ok":true,"songs":songs.len()})
                    }
                    Err(error) => {
                        report["checks"]["history_detail"] =
                            json!({"ok":false,"error":error.to_string()})
                    }
                }
            } else {
                report["history_detail_condition"] =
                    json!("no dates returned; detail not exercised");
            }
        }
        Err(error) => {
            report["checks"]["history_dates"] = json!({"ok":false,"error":error.to_string()})
        }
    }
    Ok(report)
}
#[tokio::main]
async fn main() {
    match run().await {
        Ok(report) => {
            println!("{}", serde_json::to_string_pretty(&report).expect("report"));
            if report["checks"]
                .as_object()
                .is_some_and(|checks| checks.values().any(|v| v["ok"] != true))
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
