//! Recover the explicitly isolated playlist_roundtrip journal, not arbitrary
//! resources. Verify original ownership/private/name and complete baseline.
use cumulus_core::{
    Error, Result,
    account::{AccountSession, credentials::Credentials},
    playlist::*,
    protocol::http::{ApiOrigin, ClientProfile, MusicHttpClient},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};
async fn run() -> Result<Value> {
    let a: Vec<_> = std::env::args().skip(1).collect();
    if a.len() != 2 {
        return Err(Error::InvalidInput(
            "usage: playlist_cleanup EXISTING_PC_PROFILE AUDIT_STATE",
        ));
    }
    let profile = PathBuf::from(&a[0]);
    let root = PathBuf::from(&a[1]);
    if !profile.join("core.sqlite3").is_file() || !root.join("core.sqlite3").is_file() {
        return Err(Error::InvalidInput(
            "existing profile and audit state required",
        ));
    }
    let credentials = Arc::new(Credentials::open(
        Arc::new(StateStore::open(profile)?),
        ApiOrigin::Pc,
    )?);
    let api = Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?);
    let store = Arc::new(StateStore::open(root)?);
    let session = Arc::new(AccountSession::new(api, Some(store.clone())));
    session.refresh().await?;
    let token = session.token()?;
    let uid = token.uid.as_deref().ok_or(Error::State("no account"))?;
    let baseline = store
        .get(uid, "playlist-audit", "baseline")?
        .ok_or(Error::State("no audit baseline for this owner"))?;
    let name = baseline["name"]
        .as_str()
        .ok_or(Error::State("missing audit name"))?;
    let renamed = baseline["renamed"]
        .as_str()
        .ok_or(Error::State("missing audit name"))?;
    let ids: Vec<String> = serde_json::from_value(baseline["ids"].clone())?;
    let service = PlaylistService::new(session.clone())?;
    let mut reconciled = 0;
    for r in service.operations(&token)? {
        if r.phase.pending() {
            let next = service.reconcile(&token, &r.operation_id).await?;
            if next.phase.pending() {
                return Err(Error::MutationUncertain);
            }
            reconciled += 1;
        }
    }
    let current = service.list(&token).await?;
    let mut cleared = 0;
    let mut deleted = 0;
    for p in &current.playlists {
        if p.owner != uid
            || !p.private
            || p.favorite
            || ![name, renamed].contains(&p.name.as_str())
            || ids.contains(&p.id)
        {
            continue;
        }
        let detail = service.detail(&token, &p.id).await?;
        let r = service
            .execute(
                &token,
                PlaylistCommand::Tracks {
                    id: p.id.clone(),
                    revision: detail.revision,
                    change: TrackChange::Clear,
                    ids: vec![],
                },
            )
            .await?;
        if !matches!(r.phase, PlaylistPhase::Confirmed | PlaylistPhase::Unchanged) {
            return Err(Error::MutationUncertain);
        }
        cleared += 1;
        let detail = service.detail(&token, &p.id).await?;
        let r = service
            .execute(
                &token,
                PlaylistCommand::Delete {
                    id: p.id.clone(),
                    revision: detail.revision,
                },
            )
            .await?;
        if r.phase != PlaylistPhase::Confirmed {
            return Err(Error::MutationUncertain);
        }
        deleted += 1;
    }
    if let Some(foreign) = baseline["foreign"].as_str() {
        let added = service.operations(&token)?.iter().any(|r| {
            matches!(&r.command,PlaylistCommand::Subscribe{id,subscribed:true} if id==foreign)
                && r.phase != PlaylistPhase::Unchanged
                && r.before["playlists"]
                    .as_array()
                    .is_some_and(|ps| !ps.iter().any(|p| p["id"] == foreign))
        });
        if added && current.playlists.iter().any(|p| p.id == foreign) {
            let r = service
                .execute(
                    &token,
                    PlaylistCommand::Subscribe {
                        id: foreign.into(),
                        subscribed: false,
                    },
                )
                .await?;
            if !matches!(r.phase, PlaylistPhase::Confirmed | PlaylistPhase::Unchanged) {
                return Err(Error::MutationUncertain);
            }
        }
    }
    let after = service.list(&token).await?;
    if after
        .playlists
        .iter()
        .map(|p| p.id.clone())
        .collect::<Vec<_>>()
        != ids
    {
        return Err(Error::State("original complete playlist order changed"));
    }
    store.put(
        uid,
        "playlist-audit",
        "cleanup",
        &json!({"complete":true,"full_order_restored":true}),
    )?;
    Ok(
        json!({"ok":true,"reconciled":reconciled,"cleared":cleared,"deleted_isolated":deleted,"final_count":after.playlists.len(),"cleanup_complete":true,"full_order_restored":true}),
    )
}
#[tokio::main]
async fn main() {
    match run().await {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap()),
        Err(e) => {
            eprintln!("playlist cleanup: {e}");
            std::process::exit(1);
        }
    }
}
