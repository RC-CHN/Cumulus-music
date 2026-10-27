//! Isolated current-account REST proof. Never sends invitations or uses an
//! existing room. --restore can finish only this proof's acknowledged NEW_ROOM.
#[path = "support/together.rs"]
mod proof;
use cumulus_core::{
    Error, Result, account::AccountSession, catalog::CatalogApi, listen_together::*,
};
use proof::{cleanup, now, save, setup};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, sync::Arc};

fn checked(op: TogetherOperation) -> Result<()> {
    if op.phase == TogetherPhase::Confirmed {
        Ok(())
    } else {
        Err(Error::MutationUncertain)
    }
}
async fn flow(session: Arc<AccountSession>) -> Result<Value> {
    let token = session.token()?;
    let service = TogetherService::new(session.clone())?;
    let ids = vec!["2725685941".to_owned(), "2693867212".to_owned()];
    session
        .run_for(&token, |api, _| async {
            let batch = CatalogApi::new(api)
                .songs(&ids.iter().map(|v| json!(v)).collect::<Vec<_>>(), 2)
                .await?;
            if !batch.missing.is_empty() {
                return Err(Error::State("public song samples unavailable"));
            }
            Ok(())
        })
        .await?;
    let created = service
        .create_for(&token, "cumulus_rust_isolated_proof", now())
        .await?;
    if created.phase != TogetherPhase::Confirmed {
        return Err(Error::MutationUncertain);
    }
    let room = created
        .created_room
        .ok_or(Error::State("proof has no new room acknowledgement"))?;
    let credential_room = &room;
    let credentials=session.run_for(&token,|api,owner|async move {
        let api=TogetherApi::new(api);let check=api.check(&credential_room.room_id).await?;let im=api.im_credentials().await?;
        if im.uid!=owner {return Err(Error::Response("IM token account changed"));}let rtc=api.rtc_credentials(credential_room).await?;
        Ok(json!({"im_present":!im.token.is_empty(),"rtc_present":!rtc.token.is_empty(),"rtc_expiration":rtc.expires_at,"joinable":check["joinable"]}))
    }).await?;
    let initial = service.refresh_for(&token, now()).await?;
    if initial.snapshot.playlist.is_some() {
        return Err(Error::State("fresh isolated room unexpectedly has a queue"));
    }
    let changes = [
        (ListCommandKind::Replace, SharedMode::OrderLoop, ids.clone()),
        (
            ListCommandKind::PlaymodeChange,
            SharedMode::Random,
            ids.clone(),
        ),
        (
            ListCommandKind::PlaymodeChange,
            SharedMode::SingleLoop,
            ids.clone(),
        ),
        (
            ListCommandKind::PlaymodeChange,
            SharedMode::OrderLoop,
            ids.clone(),
        ),
        (
            ListCommandKind::Delete,
            SharedMode::OrderLoop,
            vec![ids[1].clone()],
        ),
        (
            ListCommandKind::Add,
            SharedMode::OrderLoop,
            vec![ids[1].clone()],
        ),
    ];
    for (kind, mode, display) in changes {
        let state = service.state_for(&token)?;
        let version = state
            .snapshot
            .playlist
            .as_ref()
            .map(|p| p.version.clone())
            .unwrap_or_default();
        let random = if mode == SharedMode::Random {
            display.iter().rev().cloned().collect()
        } else {
            display.clone()
        };
        let cmd = ListCommand {
            command_type: kind,
            version,
            play_mode: mode,
            anchor_song_id: if kind == ListCommandKind::Add {
                ids[0].clone()
            } else {
                String::new()
            },
            anchor_position: if kind == ListCommandKind::Add { 0 } else { -1 },
            random_list: random,
            display_list: display,
        };
        checked(service.queue_for(&token, &room.room_id, cmd, now()).await?)?;
    }
    let pause =
        PlayCommand::at_seconds(CommandKind::Pause, 0.0, PlayState::Pause, &ids[0], &ids[0])?;
    if service
        .play_for(&token, &room.room_id, pause, false, now())
        .await
        .is_ok()
    {
        return Err(Error::State(
            "lost acknowledgement injection did not execute",
        ));
    }
    let pending = service
        .operations_for(&token)?
        .into_iter()
        .filter(|o| o.phase.pending())
        .collect::<Vec<_>>();
    if pending.len() != 1 {
        return Err(Error::State("lost command responsibility missing"));
    }
    checked(service.reconcile_for(&token, &pending[0].id, now()).await?)?;
    for (kind, state) in [
        (CommandKind::Progress, PlayState::Pause),
        (CommandKind::Play, PlayState::Play),
        (CommandKind::Pause, PlayState::Pause),
    ] {
        let command = PlayCommand::at_seconds(kind, 8.25, state, &ids[0], &ids[0])?;
        checked(
            service
                .play_for(&token, &room.room_id, command, false, now())
                .await?
                .ok_or(Error::State("expected explicit command was suppressed"))?,
        )?;
    }
    let current = service
        .state_for(&token)?
        .playback
        .latest
        .ok_or(Error::State("latest together command missing"))?
        .command;
    let notice = session
        .run_for(&token, |api, _| async {
            let api = TogetherApi::new(api);
            api.heartbeat(&room.room_id, &current).await?;
            match api.sync_notice(&room.room_id).await {
                Ok(()) => Ok("acknowledged"),
                Err(e) if e.is_service_code(488) => Ok("single_account_488"),
                Err(e) => Err(e),
            }
        })
        .await?;
    Ok(
        json!({"ok":true,"credentials":credentials,"queue_changes":6,"play_commands":4,"lost_successful_acknowledgement":true,"heartbeat":true,"sync_notice":notice,"im_rtc_sdk_connected":false,"two_account_realtime_verified":false}),
    )
}
#[tokio::main]
async fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let restoring = args.len() == 3 && args[2] == "--restore";
    if args.len() != 2 && !restoring {
        eprintln!("usage: together_roundtrip EXISTING_PC_PROFILE NEW_PRIVATE_ROOT [--restore]");
        std::process::exit(2);
    }
    let profile = PathBuf::from(&args[0]);
    let root = PathBuf::from(&args[1]);
    if !profile.is_absolute()
        || !profile.join("core.sqlite3").is_file()
        || !root.is_absolute()
        || (!restoring && root.exists())
    {
        eprintln!("absolute existing profile and fresh proof directory required");
        std::process::exit(2);
    }
    if !restoring {
        fs::create_dir(&root).expect("create proof directory");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).expect("private directory");
    }
    let result=async {
        let (session,guard)=setup(&profile,&root,restoring,!restoring).await?;
        let result=if restoring {Ok(json!({"restore_only":true}))} else {flow(session.clone()).await};
        let restored=cleanup(session,&guard).await;
        save(&root,if restoring {"restore-result.json"} else {"result.json"},&json!({"flow":result.as_ref().ok(),"error":result.as_ref().err().map(ToString::to_string),"restored":restored.is_ok(),"cleanup_error":restored.as_ref().err().map(ToString::to_string)}))?;
        restored?;result?;Ok::<_,Error>(())
    }.await;
    if result.is_err() {
        eprintln!("together proof failed; inspect private result and cleanup obligation");
        std::process::exit(1);
    }
    println!("isolated together REST proof and cleanup passed");
}
