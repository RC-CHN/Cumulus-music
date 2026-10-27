//! Existing-account membership + synthetic local music ledger integration.
//! No media URL is requested; this is not a real download authorization proof.
use cumulus_core::{
    Error, Result,
    account::{AccountSession, credentials::Credentials},
    download::offline::{OfflineLibrary, PublishedDownload},
    media::{
        integrity::inspect_file,
        ncm::{NcmWriteOptions, write_file},
    },
    membership::{OfflineDenial, Quality},
    protocol::http::{ApiOrigin, ClientProfile, MusicHttpClient},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, sync::Arc};

async fn run() -> Result<Value> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err(Error::InvalidInput(
            "usage: offline_roundtrip PRIVATE_PC_PROFILE NEW_OUTPUT_DIRECTORY",
        ));
    }
    let profile = PathBuf::from(&args[0]);
    if !profile.join("core.sqlite3").is_file() {
        return Err(Error::InvalidInput("existing private profile required"));
    }
    let output = PathBuf::from(&args[1]);
    fs::create_dir(&output)?;
    let store = Arc::new(StateStore::open(profile)?);
    let credentials = Arc::new(Credentials::open(store.clone(), ApiOrigin::Pc)?);
    let api = Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?);
    let session = Arc::new(AccountSession::new(api, Some(store)));
    session.refresh().await?;
    let token = session.token()?;
    let library = OfflineLibrary::new(session.clone())?;
    let membership = library.refresh_membership(&token).await?;
    let source = output.join("tone.flac");
    fs::write(
        &source,
        include_bytes!("../tests/fixtures/tag_audio/seed.flac"),
    )?;
    let before = inspect_file(&source)?;
    let mut registrations = Vec::new();
    let mut checks = Vec::new();
    let operation:Result<()>=async {
        for (name,encrypted,quality) in [("plain.flac",false,Quality::Lossless),("vip.ncm",true,Quality::Lossless),("plus.ncm",true,Quality::Sky)] {
            let path=output.join(name);if encrypted{write_file(&source,&path,NcmWriteOptions::default())?;}else{fs::copy(&source,&path)?;}
            let task=PublishedDownload{task_id:uuid::Uuid::new_v4().to_string(),song_id:"1".into(),requested_quality:quality,target:path.clone(),song:json!({"id":1,"name":"synthetic offline test","privilege":{"flag":4,"fee":1}}),resource:json!({"id":1,"level":quality.as_str(),"fee":1,"payed":0,"flag":4}),encrypted,integrity:inspect_file(&path)?};
            let record=library.register(&token,&task)?;registrations.push(record.clone());
            let checked=library.check(&token,&path,false).await?;
            let expected=if !encrypted{true}else if quality==Quality::Sky{membership.state.is_plus_vip}else{membership.state.is_vinyl_vip||membership.state.is_music_package};
            assert_eq!(checked.access.can_play,expected);if expected{assert!(checked.integrity_verified);}else if quality==Quality::Sky{assert_eq!(checked.access.reason,Some(OfflineDenial::Svip));}
            assert_eq!(OfflineLibrary::new(session.clone())?.record(&token,&path)?,Some(record));
            checks.push(json!({"case":name,"ok":true,"allowed":checked.access.can_play,"reason":checked.access.reason,"integrity_verified":checked.integrity_verified,"reopen":true}));
        }
        Ok(())
    }.await;
    // Clean only records created here; never remove media or unrelated entries.
    let mut cleanup = true;
    for record in &registrations {
        cleanup &= library.forget(&token, record).is_ok();
    }
    operation?;
    if !cleanup {
        return Err(Error::State("synthetic offline ledger cleanup incomplete"));
    }
    assert_eq!(inspect_file(&source)?, before);
    Ok(
        json!({"schema":1,"implementation":"rust-core","synthetic_media":true,"real_download_authorization":false,"remote_mutations":0,"checks":checks,"ledger_cleanup":cleanup,"source_unchanged":true}),
    )
}
#[tokio::main]
async fn main() {
    match run().await {
        Ok(report) => println!("{}", serde_json::to_string_pretty(&report).expect("report")),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1)
        }
    }
}
