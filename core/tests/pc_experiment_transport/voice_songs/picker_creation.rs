use super::*;

const LIST: &str = "/api/user/playlist";
const CREATE: &str = "/api/playlist/create";
const NAME: &str = "合成下载歌单";

fn list(created: bool, private: bool) -> HttpResponse {
    let rows = if created {
        vec![json!({"id":81,"name":NAME,"userId":25,"privacy":if private {10} else {0}})]
    } else {
        vec![]
    };
    json!({"code":200,"playlist":rows,"more":false}).into()
}

#[tokio::test]
async fn explicitly_created_playlist_only_unlocks_after_confirmed_add_to_its_returned_id() {
    let (f, receipt) = fixture([
        list(false, true),
        json!({"code":200,"id":81}).into(),
        list(true, true),
        detail(25, false).into(),
        json!({"code":200}).into(),
        detail(25, true).into(),
        json!({"code":200,"data":{"id":301,"code":403}}).into(),
    ])
    .await;
    assert!(matches!(
        f.client
            .player_voice_song_download_create_playlist(
                &f.epoch, &f.play_id, &receipt, 0, "bad#name", true
            )
            .await,
        Err(Error::InvalidInput(_))
    ));
    calls(&f, &[]);
    let created = f
        .client
        .player_voice_song_download_create_playlist(&f.epoch, &f.play_id, &receipt, 0, NAME, true)
        .await
        .unwrap();
    assert_eq!(created.create_phase, PlaylistPhase::Confirmed);
    assert_eq!(created.playlist_id.as_deref(), Some(PLAYLIST));
    let add = created.add.unwrap();
    assert_ne!(created.create_operation_id, add.operation_id);
    assert_eq!(add.track_id, "301");
    assert_eq!(add.phase, PlaylistPhase::Confirmed);
    let continuation = add.continuation.unwrap();
    let repeated = f
        .client
        .player_voice_song_download_create_playlist(&f.epoch, &f.play_id, &receipt, 0, NAME, true)
        .await
        .unwrap();
    assert_eq!(repeated.create_operation_id, created.create_operation_id);
    assert_eq!(repeated.add.unwrap().operation_id, add.operation_id);
    calls(&f, &[LIST, CREATE, LIST, DETAIL, ADD, DETAIL]);
    let wire = f.http.calls();
    let create = &wire[STARTUP.len() + 3].payload;
    assert_eq!(create["name"], NAME);
    assert_eq!(create["uid"], "25");
    assert_eq!(create["privacy"], "10");
    assert!(create.get("noToast").is_none());
    let add = &wire[STARTUP.len() + 6].payload;
    assert_eq!(add["pid"], PLAYLIST);
    assert_eq!(add["trackIds"], r#"["301"]"#);
    assert!(add.get("imme").is_none());
    f.client
        .download_command(
            &f.epoch,
            DownloadCommand::Configure {
                root: f.root.path().join("output").to_str().unwrap().into(),
                path_type: 0,
                file_type: 0,
            },
        )
        .await
        .unwrap();
    f.client
        .player_voice_song_download_start_from_playlist(
            &f.epoch,
            &f.play_id,
            &receipt,
            0,
            &continuation,
            DownloadOptions::default(),
        )
        .await
        .unwrap();
    f.http.wait(STARTUP.len() + 9).await;
    calls(&f, &[LIST, CREATE, LIST, DETAIL, ADD, DETAIL, AUTH]);
}

#[tokio::test]
async fn add_false_blocks_created_destination_even_though_existing_picker_can_continue() {
    let (f, receipt) = fixture([
        list(false, false),
        json!({"code":200,"id":81}).into(),
        list(true, false),
        detail(25, false).into(),
        json!({"code":403}).into(),
        detail(25, false).into(),
    ])
    .await;
    let result = f
        .client
        .player_voice_song_download_create_playlist(&f.epoch, &f.play_id, &receipt, 0, NAME, false)
        .await
        .unwrap();
    assert_eq!(result.create_phase, PlaylistPhase::Confirmed);
    let add = result.add.unwrap();
    assert_eq!(add.phase, PlaylistPhase::Rejected);
    assert!(add.continuation.is_none());
    assert_eq!(f.http.calls()[STARTUP.len() + 3].payload["privacy"], "0");
    let repeated = f
        .client
        .player_voice_song_download_add_created_playlist(&f.epoch, &f.play_id, &receipt, 0)
        .await
        .unwrap();
    assert_eq!(repeated.add.unwrap().operation_id, add.operation_id);
    calls(&f, &[LIST, CREATE, LIST, DETAIL, ADD, DETAIL]);
    assert!(
        f.store
            .values("25", "download_tasks_v1")
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn explicit_add_false_is_not_upgraded_by_concurrent_membership_readback() {
    let (f, receipt) = fixture([
        list(false, true),
        json!({"code":200,"id":81}).into(),
        list(true, true),
        detail(25, false).into(),
        json!({"code":403}).into(),
        detail(25, true).into(),
    ])
    .await;
    let created = f
        .client
        .player_voice_song_download_create_playlist(&f.epoch, &f.play_id, &receipt, 0, NAME, true)
        .await
        .unwrap();
    let add = created.add.unwrap();
    assert_eq!(add.phase, PlaylistPhase::Confirmed);
    assert!(add.continuation.is_none());
    assert!(add.add_failed);
    assert_eq!(
        f.store
            .get("25", "playlist-mutations", &add.operation_id)
            .unwrap()
            .unwrap()["error_class"],
        "service"
    );
    calls(&f, &[LIST, CREATE, LIST, DETAIL, ADD, DETAIL]);
}

#[tokio::test]
async fn lost_create_ack_reconciles_read_only_and_only_explicit_resume_submits_the_first_add() {
    let (f, receipt) = fixture([
        list(false, true),
        HttpResponse::Ready(Err(Error::Transport("synthetic lost create".into()))),
        list(false, true),
        list(true, true),
        detail(25, false).into(),
        json!({"code":200}).into(),
        detail(25, true).into(),
    ])
    .await;
    let unknown = f
        .client
        .player_voice_song_download_create_playlist(&f.epoch, &f.play_id, &receipt, 0, NAME, true)
        .await
        .unwrap();
    assert_eq!(unknown.create_phase, PlaylistPhase::Uncertain);
    assert!(unknown.add.is_none());
    assert!(matches!(
        f.client
            .player_voice_song_download_add_created_playlist(&f.epoch, &f.play_id, &receipt, 0)
            .await,
        Err(Error::State(_))
    ));
    let recovered = f
        .client
        .player_voice_song_download_creation_status(&f.epoch, &f.play_id, &receipt, 0, true)
        .await
        .unwrap();
    assert_eq!(recovered.create_operation_id, unknown.create_operation_id);
    assert_eq!(recovered.create_phase, PlaylistPhase::Confirmed);
    assert!(recovered.add.is_none());
    let duplicate = f
        .client
        .player_voice_song_download_create_playlist(&f.epoch, &f.play_id, &receipt, 0, NAME, true)
        .await
        .unwrap();
    assert!(duplicate.add.is_none());
    calls(&f, &[LIST, CREATE, LIST, LIST]);
    let resumed = f
        .client
        .player_voice_song_download_add_created_playlist(&f.epoch, &f.play_id, &receipt, 0)
        .await
        .unwrap();
    assert!(resumed.add.unwrap().continuation.is_some());
    calls(&f, &[LIST, CREATE, LIST, LIST, DETAIL, ADD, DETAIL]);
}

#[tokio::test]
async fn a_stale_card_after_create_dispatch_keeps_creation_but_cannot_add() {
    let (held, release) = gate();
    let (f, receipt) = fixture([list(false, true), held, list(true, true)]).await;
    let (client, epoch, play_id) = (f.client.clone(), f.epoch.clone(), f.play_id.clone());
    let work = tokio::spawn(async move {
        client
            .player_voice_song_download_create_playlist(&epoch, &play_id, &receipt, 0, NAME, true)
            .await
    });
    f.http.wait(STARTUP.len() + 4).await;
    f.client
        .player_action(&f.epoch, PlayerAction::Stop)
        .await
        .unwrap();
    assert!(release.send(Ok(reply(json!({"code":200,"id":81})))).is_ok());
    assert!(matches!(work.await.unwrap(), Err(Error::StaleResponse)));
    calls(&f, &[LIST, CREATE, LIST]);
    let records = f.store.values("25", "playlist-mutations").unwrap();
    assert_eq!(records.len(), 1);
    let only = records.values().next().unwrap();
    assert_eq!(only["phase"], "confirmed");
    assert_eq!(only["command"]["kind"], "create");
    assert!(
        f.store
            .values("25", "download_tasks_v1")
            .unwrap()
            .is_empty()
    );
}
