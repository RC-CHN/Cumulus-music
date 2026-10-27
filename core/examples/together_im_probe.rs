//! Single-account native SDK proof, with the same isolated-room guard as the
//! REST proof. Never sends SDK messages/invitations or touches an existing room.
#[path = "support/together.rs"]
mod proof;
use cumulus_core::{
    Error, Result,
    account::AccountSession,
    listen_together::{native_im::*, *},
};
use proof::{cleanup, now, save, setup};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

fn record(root: &Path, event: &ImEvent) -> Result<()> {
    let value = match event {
        ImEvent::Login {
            step,
            code,
            retrying,
        } => json!({"kind":"login","step":step,"code":code,"retrying":retrying}),
        ImEvent::Grant { code } => json!({"kind":"grant","code":code}),
        ImEvent::Enter { step, code, .. } => json!({"kind":"enter","step":step,"code":code}),
        ImEvent::Exit { code, reason, .. } => json!({"kind":"exit","code":code,"reason":reason}),
        ImEvent::Link { condition, .. } => json!({"kind":"link","condition":condition}),
        ImEvent::Message { body, .. } => json!({"kind":"message","body":body}),
        ImEvent::Notification { body, .. } => json!({"kind":"notification","body":body}),
        ImEvent::Business { messages } => {
            json!({"kind":"business","messages":messages.iter().map(|m|json!({"type":m.kind.code(),"content":m.content})).collect::<Vec<_>>()})
        }
        ImEvent::Kicked { reason } => json!({"kind":"kicked","reason":reason}),
        ImEvent::Disconnected => json!({"kind":"disconnected"}),
    };
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("events.jsonl"))?;
    serde_json::to_writer(&mut file, &json!({"at_ms":now(),"event":value}))?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}
fn await_phase(im: &mut NativeIm, root: &Path, entering: bool) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(45);
    while Instant::now() < deadline {
        let Some(event) = im.next_event(Duration::from_millis(250))? else {
            continue;
        };
        record(root, &event)?;
        match event {
            ImEvent::Login {
                step: 3, code: 200, ..
            } if !entering => return Ok(()),
            ImEvent::Enter {
                step: 5, code: 200, ..
            } if entering => return Ok(()),
            ImEvent::Login {
                code,
                retrying: false,
                ..
            } if code != 200 => return Err(Error::State("NIM login rejected; see event code")),
            ImEvent::Grant { code } | ImEvent::Enter { code, .. } if code != 200 => {
                return Err(Error::State("NIM entry rejected; see event code"));
            }
            ImEvent::Kicked { .. } => return Err(Error::State("NIM connection was kicked")),
            _ => {}
        }
    }
    Err(Error::State("NIM login/entry deadline exceeded"))
}
fn checked(operation: TogetherOperation) -> Result<()> {
    if operation.phase == TogetherPhase::Confirmed {
        Ok(())
    } else {
        Err(Error::MutationUncertain)
    }
}
async fn flow(
    session: Arc<AccountSession>,
    root: PathBuf,
    sdk: PathBuf,
    keys: PathBuf,
) -> Result<Value> {
    let keys: Value = serde_json::from_slice(&fs::read(keys)?)?;
    let key = keys
        .as_array()
        .and_then(|keys| keys.iter().find(|k| k["pointer"] == "0x18219eb08"))
        .and_then(|key| key["value"].as_str())
        .ok_or(Error::InvalidInput(
            "official default IM application configuration missing",
        ))?
        .to_owned();
    let token = session.token()?;
    let service = TogetherService::new(session.clone())?;
    let operation = service
        .create_for(&token, "cumulus_native_im_proof", now())
        .await?;
    if operation.phase != TogetherPhase::Confirmed {
        return Err(Error::MutationUncertain);
    }
    let room = operation
        .created_room
        .ok_or(Error::State("no acknowledged isolated new room"))?;
    let credentials = session
        .run_for(&token, |api, _| async move {
            TogetherApi::new(api).im_credentials().await
        })
        .await?;
    let database_key = uuid::Uuid::new_v4().simple().to_string();
    save(
        &root,
        "sdk-private-config.json",
        &json!({"database_key":database_key}),
    )?;
    let worker_root = root.clone();
    let sdk_token = token.clone();
    let sdk_session = session.clone();
    let reopen_sdk = sdk.clone();
    let reopen_key = key.clone();
    let reopen_database_key = database_key.clone();
    let im = tokio::task::spawn_blocking(move || {
        let mut im = NativeIm::open(
            NimConfig {
                library_directory: sdk,
                data_root: worker_root.join("sdk-data"),
                app_key: key,
                database_key,
            },
            sdk_session,
            sdk_token,
            credentials,
        )?;
        await_phase(&mut im, &worker_root, false)?;
        im.enter(&room)?;
        await_phase(&mut im, &worker_root, true)?;
        Ok::<_, Error>((im, room))
    })
    .await
    .map_err(|_| Error::State("NIM worker failed"))??;
    let (im, room) = im;
    // The adapter's generation is the same account as this durable service.
    // Observe whether shared REST writes produce messages for their sender.
    // A successful REST response alone cannot establish a message echo.
    let writes = async {
        service.refresh_for(&token, now()).await?;
        let ids = vec!["2725685941".into(), "2693867212".into()];
        checked(
            service
                .queue_for(
                    &token,
                    &room.room_id,
                    ListCommand {
                        command_type: ListCommandKind::Replace,
                        version: Vec::new(),
                        play_mode: SharedMode::OrderLoop,
                        anchor_song_id: String::new(),
                        anchor_position: -1,
                        random_list: ids.clone(),
                        display_list: ids.clone(),
                    },
                    now(),
                )
                .await?,
        )?;
        let command =
            PlayCommand::at_seconds(CommandKind::Pause, 0.0, PlayState::Pause, &ids[0], &ids[0])?;
        let operation = service
            .play_for(&token, &room.room_id, command, false, now())
            .await?
            .ok_or(Error::State("sample pause was unexpectedly filtered"))?;
        checked(operation)?;
        Ok::<_, Error>(())
    }
    .await;
    let worker_root = root.clone();
    let expected_room_id = room.room_id.clone();
    let transport = tokio::task::spawn_blocking(move || {
        let mut im = im;
        let observed = (|| {
            let deadline = Instant::now() + Duration::from_secs(45);
            let mut messages = 0;
            let mut notification_messages = 0;
            let mut business_types=std::collections::BTreeMap::<u32,u64>::new();
            let mut current_room_business=0;
            while Instant::now() < deadline {
                if let Some(event) = im.next_event(Duration::from_millis(250))? {
                    record(&worker_root, &event)?;
                    if let ImEvent::Message { body, .. } = event {
                        messages += 1;
                        if body["msg_type"] == 5 { notification_messages += 1; }
                    } else if let ImEvent::Business{messages}=event {
                        for message in messages {
                            *business_types.entry(message.kind.code()).or_default()+=1;
                            if message.belongs_to(&expected_room_id){current_room_business+=1;}
                        }
                    }
                }
            }
            Ok::<_,Error>((messages, notification_messages,business_types,current_room_business))
        })();
        let closed = im.close();
        save(&worker_root, "sdk-close.json", &json!({"closed":closed.is_ok(),"error":closed.as_ref().err().map(ToString::to_string)}))?;
        closed?;
        observed
    }).await.map_err(|_| Error::State("NIM worker failed"))?;
    writes?;
    let (messages, notification_messages, business_types, current_room_business) = transport?;
    // Same process, same persisted encrypted cache, newly fetched IM token.
    // This exercises SDK cleanup/reinitialization, not a second account.
    let credentials = session
        .run_for(&token, |api, _| async move {
            TogetherApi::new(api).im_credentials().await
        })
        .await?;
    let worker_root = root.clone();
    tokio::task::spawn_blocking(move || {
        save(
            &worker_root,
            "reopen-start.json",
            &json!({"same_process":true,"same_account":true}),
        )?;
        let mut im = NativeIm::open(
            NimConfig {
                library_directory: reopen_sdk,
                data_root: worker_root.join("sdk-data"),
                app_key: reopen_key,
                database_key: reopen_database_key,
            },
            session,
            token,
            credentials,
        )?;
        await_phase(&mut im, &worker_root, false)?;
        im.enter(&room)?;
        await_phase(&mut im, &worker_root, true)?;
        im.close()?;
        im.close()?;
        save(
            &worker_root,
            "reopen-result.json",
            &json!({"login":true,"room_entry":true,"closed":true,"close_idempotent":true}),
        )
    })
    .await
    .map_err(|_| Error::State("NIM reopen worker failed"))??;
    Ok(
        json!({"native_login":true,"native_room_entry":true,"messages_received":messages,"sdk_notification_messages":notification_messages,"business_types":business_types,"current_room_business":current_room_business,"same_process_reopen":true,"business_play_sync_verified":false,"dual_account_verified":false}),
    )
}

#[tokio::main]
async fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 4 && !(args.len() == 5 && args[4] == "--restore") {
        eprintln!(
            "usage: together_im_probe EXISTING_PC_PROFILE NEW_PRIVATE_ROOT SDK_LIB_DIRECTORY RECOVERED_APPKEYS_JSON [--restore]"
        );
        std::process::exit(2);
    }
    let profile = PathBuf::from(&args[0]);
    let root = PathBuf::from(&args[1]);
    let sdk = PathBuf::from(&args[2]);
    let keys = PathBuf::from(&args[3]);
    let restoring = args.len() == 5;
    let result = async {
        if !profile.is_absolute() || !profile.join("core.sqlite3").is_file() || !root.is_absolute() || (!restoring && root.exists()) {
            return Err(Error::InvalidInput("existing absolute profile and fresh absolute proof root required"));
        }
        if !restoring { fs::create_dir(&root)?; }
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        }
        let (session, guard) = setup(&profile, &root, restoring, false).await?;
        let ran = if restoring { Ok(json!({"restore_only":true})) } else { flow(session.clone(), root.clone(), sdk, keys).await };
        let restored = cleanup(session, &guard).await;
        save(&root, if restoring {"restore-result.json"} else {"result.json"}, &json!({"flow":ran.as_ref().ok(),"error":ran.as_ref().err().map(ToString::to_string),"restored":restored.is_ok(),"cleanup_error":restored.as_ref().err().map(ToString::to_string)}))?;
        restored?; ran?; Ok::<_,Error>(())
    }.await;
    if let Err(error) = result {
        eprintln!("native IM proof failed: {error}; private proof retains cleanup responsibility");
        std::process::exit(1);
    }
    println!("native IM probe finished; inspect sanitized result for actual message coverage");
}
