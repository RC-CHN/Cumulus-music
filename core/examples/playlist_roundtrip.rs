//! Explicit isolated live playlist audit. Never auto-run by tests or startup.
//! Only creates one uniquely named PRIVATE playlist and restores subscriptions
//! added by this run; cleanup requires original owner/name/private evidence.
use cumulus_core::{
    Error, Result,
    account::{AccountApi, AccountSession, credentials::Credentials},
    catalog::{CatalogApi, SearchOptions},
    playlist::*,
    protocol::{
        api::identifier,
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};

async fn execute(
    service: &PlaylistService,
    token: &cumulus_core::account::SessionToken,
    command: PlaylistCommand,
) -> Result<PlaylistMutation> {
    let mut record = service.execute(token, command).await?;
    for _ in 0..3 {
        if !record.phase.pending() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        record = service.reconcile(token, &record.operation_id).await?;
    }
    if !matches!(
        record.phase,
        PlaylistPhase::Confirmed | PlaylistPhase::Unchanged
    ) {
        return Err(Error::MutationUncertain);
    }
    Ok(record)
}
fn ids(list: &PlaylistList) -> Vec<String> {
    list.playlists.iter().map(|p| p.id.clone()).collect()
}
async fn run() -> Result<Value> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err(Error::InvalidInput(
            "usage: playlist_roundtrip EXISTING_PC_PROFILE NEW_PRIVATE_STATE_DIRECTORY",
        ));
    }
    let profile = PathBuf::from(&args[0]);
    let root = PathBuf::from(&args[1]);
    if !profile.join("core.sqlite3").is_file() || root.exists() {
        return Err(Error::InvalidInput(
            "existing profile and new isolated directory required",
        ));
    }
    let credentials = Arc::new(Credentials::open(
        Arc::new(StateStore::open(profile)?),
        ApiOrigin::Pc,
    )?);
    let api = Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?);
    let store = Arc::new(StateStore::open(&root)?);
    let session = Arc::new(AccountSession::new(api, Some(store.clone())));
    session.refresh().await?;
    let service = PlaylistService::new(session.clone())?;
    let token = session.token()?;
    let uid = token
        .uid
        .as_deref()
        .ok_or(Error::State("missing account"))?;
    let baseline = service.list(&token).await?;
    let (_, liked) = session
        .run(|api, uid| async move { AccountApi::new(api).liked_ids(&json!(uid)).await })
        .await?;
    if liked.len() < 3 {
        return Err(Error::State("three existing liked songs required"));
    }
    let (_, search) = session
        .run(|api, _| async move {
            CatalogApi::new(api)
                .search(
                    "纯音乐",
                    SearchOptions {
                        kind: 1000,
                        limit: 20,
                        offset: 0,
                    },
                )
                .await
        })
        .await?;
    let foreign = search["result"]["playlists"]
        .as_array()
        .and_then(|rows| {
            rows.iter().find_map(|p| {
                let id = identifier(&p["id"]).ok()?;
                (!baseline.playlists.iter().any(|old| old.id == id)).then_some(id)
            })
        })
        .ok_or(Error::Response("no uncollected public playlist candidate"))?;
    let foreign_detail = service.detail(&token, &foreign).await?;
    if foreign_detail.summary.private || foreign_detail.summary.owner == uid {
        return Err(Error::State(
            "candidate must be another user's public playlist",
        ));
    }
    let suffix = &uuid::Uuid::new_v4().simple().to_string()[..8];
    let name = format!("CumulusRust{suffix}");
    let renamed = format!("CumulusEdit{suffix}");
    store.put(uid,"playlist-audit","baseline",&json!({"ids":ids(&baseline),"name":name,"renamed":renamed,"foreign":foreign,"cleanup_required":true}))?;
    let mut checks = Vec::new();
    let mut created = None;
    let result: Result<()> = async {
        let r = execute(
            &service,
            &token,
            PlaylistCommand::Create {
                name: name.clone(),
                private: true,
            },
        )
        .await?;
        let pid = r
            .playlist_id
            .ok_or(Error::Response("missing confirmed created id"))?;
        created = Some(pid.clone());
        store.put(uid, "playlist-audit", "created", &json!({"id":pid}))?;
        checks.push("create_private");
        let p = service.detail(&token, &pid).await?;
        execute(
            &service,
            &token,
            PlaylistCommand::Metadata {
                id: pid.clone(),
                revision: p.revision,
                name: renamed.clone(),
                tags: vec!["电子".into()],
                description: "Rust core isolated verification".into(),
            },
        )
        .await?;
        checks.push("metadata");
        for (change, wanted, label) in [
            (TrackChange::Add, liked[..3].to_vec(), "add_tracks"),
            (
                TrackChange::Add,
                vec![liked[0].clone()],
                "duplicate_add_noop",
            ),
        ] {
            let p = service.detail(&token, &pid).await?;
            execute(
                &service,
                &token,
                PlaylistCommand::Tracks {
                    id: pid.clone(),
                    revision: p.revision,
                    change,
                    ids: wanted,
                },
            )
            .await?;
            checks.push(label);
        }
        let p = service.detail(&token, &pid).await?;
        let mut order = p.tracks.clone();
        order.reverse();
        execute(
            &service,
            &token,
            PlaylistCommand::Tracks {
                id: pid.clone(),
                revision: p.revision,
                change: TrackChange::Reorder,
                ids: order.clone(),
            },
        )
        .await?;
        checks.push("reorder_tracks");
        for label in ["delete_track", "absent_delete_noop"] {
            let p = service.detail(&token, &pid).await?;
            execute(
                &service,
                &token,
                PlaylistCommand::Tracks {
                    id: pid.clone(),
                    revision: p.revision,
                    change: TrackChange::Delete,
                    ids: vec![order[0].clone()],
                },
            )
            .await?;
            checks.push(label);
        }
        let list = service.list(&token).await?;
        let mut order = list.group(uid, PlaylistGroup::Created);
        order.retain(|id| id != &pid);
        order.push(pid.clone());
        execute(
            &service,
            &token,
            PlaylistCommand::Order {
                group: PlaylistGroup::Created,
                revision: list.revision,
                ids: order,
            },
        )
        .await?;
        checks.push("playlist_order");
        let p = service.detail(&token, &pid).await?;
        execute(
            &service,
            &token,
            PlaylistCommand::Tracks {
                id: pid.clone(),
                revision: p.revision,
                change: TrackChange::Clear,
                ids: vec![],
            },
        )
        .await?;
        checks.push("clear_tracks");
        let p = service.detail(&token, &pid).await?;
        execute(
            &service,
            &token,
            PlaylistCommand::Metadata {
                id: pid.clone(),
                revision: p.revision,
                name: renamed.clone(),
                tags: vec![],
                description: String::new(),
            },
        )
        .await?;
        checks.push("clear_metadata");
        let subscribed = execute(
            &service,
            &token,
            PlaylistCommand::Subscribe {
                id: foreign.clone(),
                subscribed: true,
            },
        )
        .await?;
        if subscribed.phase == PlaylistPhase::Unchanged {
            return Err(Error::State(
                "candidate was independently subscribed; preserve it",
            ));
        }
        checks.push("subscribe");
        execute(
            &service,
            &token,
            PlaylistCommand::Subscribe {
                id: foreign.clone(),
                subscribed: false,
            },
        )
        .await?;
        checks.push("unsubscribe");
        Ok(())
    }
    .await;
    // Reconcile uncertain writes by reads before cleanup; no duplicate create.
    for record in service.operations(&token)? {
        if record.phase.pending() {
            let _ = service.reconcile(&token, &record.operation_id).await;
        }
    }
    let current = service.list(&token).await?;
    let added_by_this_run = service.operations(&token)?.iter().any(|r| {
        matches!(&r.command,PlaylistCommand::Subscribe{id,subscribed:true} if id==&foreign)
            && r.phase != PlaylistPhase::Unchanged
            && r.before["playlists"]
                .as_array()
                .is_some_and(|rows| !rows.iter().any(|p| p["id"] == foreign))
    });
    if added_by_this_run && current.playlists.iter().any(|p| p.id == foreign) {
        execute(
            &service,
            &token,
            PlaylistCommand::Subscribe {
                id: foreign.clone(),
                subscribed: false,
            },
        )
        .await?;
    }
    if created.is_none() {
        let candidates = current
            .playlists
            .iter()
            .filter(|p| {
                p.owner == uid
                    && p.private
                    && !p.favorite
                    && p.name == name
                    && !baseline.playlists.iter().any(|b| b.id == p.id)
            })
            .collect::<Vec<_>>();
        if candidates.len() == 1 {
            created = Some(candidates[0].id.clone());
        }
    }
    if let Some(pid) = created {
        let p = service.detail(&token, &pid).await?;
        if p.summary.owner != uid
            || !p.summary.private
            || p.summary.favorite
            || ![name, renamed].contains(&p.summary.name)
            || baseline.playlists.iter().any(|b| b.id == pid)
        {
            return Err(Error::State(
                "cleanup ownership/name/private evidence mismatch",
            ));
        }
        execute(
            &service,
            &token,
            PlaylistCommand::Delete {
                id: pid,
                revision: p.revision,
            },
        )
        .await?;
        checks.push("delete_isolated_playlist");
    }
    let after = service.list(&token).await?;
    if ids(&after) != ids(&baseline) {
        return Err(Error::State("full original playlist order not restored"));
    }
    store.put(
        uid,
        "playlist-audit",
        "cleanup",
        &json!({"complete":true,"full_order_restored":true}),
    )?;
    let error = result.err().map(|e| e.to_string());
    Ok(
        json!({"ok":error.is_none(),"checks":checks,"check_count":checks.len(),"baseline_count":baseline.playlists.len(),"final_count":after.playlists.len(),"cleanup_complete":true,"full_order_restored":true,"error":error,"cover_upload_tested":false,"publish_tested":false}),
    )
}
#[tokio::main]
async fn main() {
    match run().await {
        Ok(report) => {
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
            if report["ok"] != true {
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("playlist audit: {e}");
            std::process::exit(1);
        }
    }
}
