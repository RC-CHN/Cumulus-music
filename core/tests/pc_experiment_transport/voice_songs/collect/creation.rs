use super::*;
use cumulus_core::client::dto::VoiceSongCollectCreation;

const HOST_LIST: &str = "/api/user/playlist";
const CREATE: &str = "/api/playlist/create";
const NAME: &str = "合成新收藏歌单";

fn summary(id: u64, owner: u64, private: bool, name: &str) -> Value {
    json!({"id":id,"userId":owner,"privacy":if private {10} else {0},"name":name})
}

fn host_list(playlists: &[Value]) -> Value {
    json!({"code":200,"playlist":playlists,"more":false})
}

fn fresh_list(private: bool) -> HttpResponse {
    host_list(&[summary(91, 25, private, NAME)]).into()
}

fn created_detail(private: bool, tracks: &[u64]) -> Value {
    let mut row = summary(91, 25, private, NAME);
    row["tags"] = json!([]);
    row["trackIds"] = json!(tracks.iter().map(|id| json!({"id":id})).collect::<Vec<_>>());
    json!({"code":200,"playlist":row})
}

async fn create(
    f: &Fixture,
    card: &str,
    selection: &str,
    name: &str,
    private: bool,
) -> cumulus_core::Result<VoiceSongCollectCreation> {
    f.client
        .player_voice_songs_collect_create(&f.epoch, &f.play_id, card, selection, name, private)
        .await
}

async fn status(
    f: &Fixture,
    card: &str,
    selection: &str,
    reconcile: bool,
) -> cumulus_core::Result<VoiceSongCollectCreation> {
    f.client
        .player_voice_songs_collect_creation_status(
            &f.epoch, &f.play_id, card, selection, reconcile,
        )
        .await
}

async fn add(
    f: &Fixture,
    card: &str,
    selection: &str,
) -> cumulus_core::Result<VoiceSongCollectCreation> {
    f.client
        .player_voice_songs_collect_add_created(&f.epoch, &f.play_id, card, selection)
        .await
}

#[tokio::test]
async fn cancelling_prepared_selection_sends_nothing_and_revokes_exact_receipt() {
    let (f, card) = setup(&[COOKIE], []).await;
    let selected = prepare(&f, &card, &[2, 0]).selection_receipt.unwrap();
    assert!(matches!(
        create(&f, &card, &selected, "invalid#name", true).await,
        Err(Error::InvalidInput(_))
    ));
    f.client
        .player_voice_songs_collect_cancel_prepared(&f.epoch, &f.play_id, &card, &selected)
        .unwrap();
    assert!(matches!(
        create(&f, &card, &selected, NAME, true).await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        status(&f, &card, &selected, true).await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        add(&f, &card, &selected).await,
        Err(Error::StaleResponse)
    ));
    assert_ne!(
        prepare(&f, &card, &[2, 0]).selection_receipt.as_deref(),
        Some(selected.as_str())
    );
    calls(&f, &[]);
    assert!(
        f.store
            .values("25", "playlist-mutations")
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn acknowledged_create_uses_returned_id_and_exact_filtered_group_once() {
    let duplicate = summary(88, 25, true, NAME);
    let cookies = [
        COOKIE,
        "MUSIC_U=foreign; Path=/eapi/song/enhance/download/url/v1; Secure",
        "MUSIC_U=foreign; Path=/eapi/song/like/get; Secure",
    ];
    let (f, card) = setup(
        &cookies,
        [
            list(81),
            host_list(std::slice::from_ref(&duplicate)).into(),
            json!({"code":200,"id":91}).into(),
            host_list(&[duplicate, summary(91, 25, true, NAME)]).into(),
            created_detail(true, &[]).into(),
            json!({"code":200}).into(),
            created_detail(true, &[303, 301]).into(),
        ],
    )
    .await;
    assert!(matches!(
        f.client
            .player_voice_song_download_prepare(&f.epoch, &f.play_id, &card, 2)
            .await,
        Err(Error::StaleResponse)
    ));
    let plan = prepare(&f, &card, &[2, 1, 0, 4, 3, 5, 7]);
    assert_eq!((plan.selected_count, plan.eligible_count), (7, 2));
    assert_eq!(plan.omitted_indices, [1, 4, 3, 5, 7]);
    let selection = plan.selection_receipt.unwrap();
    destinations(&f, &card, &selection, "81").await;
    let created = create(&f, &card, &selection, &format!(" {NAME} "), true)
        .await
        .unwrap();
    assert_eq!(created.epoch, f.epoch);
    assert_eq!(created.play_id, f.play_id);
    assert_eq!(created.selection_receipt, selection);
    assert_eq!(created.create_phase, PlaylistPhase::Confirmed);
    assert_eq!(created.playlist_id.as_deref(), Some("91"));
    assert!(!created.can_add && !created.add_failed);
    let added = created.add.unwrap();
    assert_eq!(added.playlist_id, "91");
    assert_eq!(added.phase, PlaylistPhase::Confirmed);
    assert_ne!(added.operation_id, created.create_operation_id);
    for again in [
        create(&f, &card, &selection, NAME, true).await.unwrap(),
        status(&f, &card, &selection, true).await.unwrap(),
        add(&f, &card, &selection).await.unwrap(),
    ] {
        assert_eq!(again.create_operation_id, created.create_operation_id);
        assert_eq!(again.add.unwrap().operation_id, added.operation_id);
    }
    assert!(matches!(
        create(&f, &card, &selection, "不同歌单", true).await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        create(&f, &card, &selection, NAME, false).await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        f.client
            .player_voice_songs_collect_existing(&f.epoch, &f.play_id, &card, &selection, "81")
            .await,
        Err(Error::StaleResponse)
    ));
    let trace = f.http.calls();
    let unpack = |payload: &Value| {
        let mut body = payload.clone();
        let header = body.as_object_mut().unwrap().remove("header").unwrap();
        let header: Value = serde_json::from_str(header.as_str().unwrap()).unwrap();
        assert_eq!(header["os"], "pc");
        assert_eq!(header["appver"], "3.1.42.205594");
        assert_eq!(header["clientSign"], "synthetic-signature");
        assert!(!header["deviceId"].as_str().unwrap().is_empty());
        (body, header)
    };
    let create_call = trace.iter().find(|call| call.path == CREATE).unwrap();
    let add_call = trace.iter().find(|call| call.path == ADD).unwrap();
    let (created_wire, create_header) = unpack(&create_call.payload);
    let (add_wire, add_header) = unpack(&add_call.payload);
    assert_eq!(create_header["deviceId"], add_header["deviceId"]);
    assert!(
        create_call
            .cookie
            .as_deref()
            .unwrap()
            .contains("MUSIC_U=synthetic-voice")
    );
    assert_eq!(create_call.cookie, add_call.cookie);
    assert_eq!(created_wire, json!({"uid":"25","name":NAME,"privacy":"10"}));
    assert_eq!(
        add_wire,
        json!({"pid":"91","trackIds":"[\"303\",\"301\"]","op":"add"})
    );
    calls(
        &f,
        &[LIST, HOST_LIST, CREATE, HOST_LIST, DETAIL, ADD, DETAIL],
    );
    let records = f.store.values("25", "playlist-mutations").unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(
        records[&created.create_operation_id]["command"]["kind"],
        "create"
    );
    assert_eq!(
        records[&added.operation_id]["command"]["ids"],
        json!(["303", "301"])
    );
}

#[tokio::test]
async fn unknown_create_only_reconciles_and_a_list_inferred_id_never_authorizes_add() {
    let (f, card) = setup(
        &[COOKIE],
        [
            host_list(&[]).into(),
            HttpResponse::Ready(Err(Error::Transport(
                "synthetic lost group create ack".into(),
            ))),
            host_list(&[]).into(),
            fresh_list(true),
        ],
    )
    .await;
    let selection = prepare(&f, &card, &[2, 0]).selection_receipt.unwrap();
    let initial = create(&f, &card, &selection, NAME, true).await.unwrap();
    assert_eq!(initial.create_phase, PlaylistPhase::Uncertain);
    assert!(!initial.can_add && initial.add.is_none());
    assert!(matches!(
        f.client
            .player_voice_songs_collect_prepare(&f.epoch, &f.play_id, &card, &[0, 2]),
        Err(Error::MutationUncertain)
    ));
    assert!(matches!(
        add(&f, &card, &selection).await,
        Err(Error::StaleResponse)
    ));
    let found = status(&f, &card, &selection, true).await.unwrap();
    assert_eq!(found.create_operation_id, initial.create_operation_id);
    assert_eq!(found.create_phase, PlaylistPhase::Confirmed);
    assert!(!found.can_add && found.playlist_id.is_none() && found.add.is_none());
    assert!(matches!(
        add(&f, &card, &selection).await,
        Err(Error::StaleResponse)
    ));
    assert!(
        !create(&f, &card, &selection, NAME, true)
            .await
            .unwrap()
            .can_add
    );
    calls(&f, &[HOST_LIST, CREATE, HOST_LIST, HOST_LIST]);
    let records = f.store.values("25", "playlist-mutations").unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[&found.create_operation_id]["playlist_id"], "91");
}

#[tokio::test]
async fn recovered_acknowledged_create_requires_explicit_first_add_and_only_reads_back_lost_add() {
    let (f, card) = setup(
        &[COOKIE],
        [
            host_list(&[]).into(),
            json!({"code":200,"id":91}).into(),
            // A different same-name candidate cannot substitute for the ack ID.
            host_list(&[summary(88, 25, false, NAME)]).into(),
            fresh_list(false),
            created_detail(false, &[]).into(),
            HttpResponse::Ready(Err(Error::Transport("synthetic lost group add ack".into()))),
            created_detail(false, &[]).into(),
            created_detail(false, &[301, 303]).into(),
        ],
    )
    .await;
    let selection = prepare(&f, &card, &[0, 2]).selection_receipt.unwrap();
    let first = create(&f, &card, &selection, NAME, false).await.unwrap();
    assert_eq!(first.create_phase, PlaylistPhase::Uncertain);
    assert!(first.add.is_none());
    let read = status(&f, &card, &selection, true).await.unwrap();
    assert_eq!(read.create_phase, PlaylistPhase::Confirmed);
    assert_eq!(read.playlist_id.as_deref(), Some("91"));
    assert!(read.can_add && read.add.is_none());
    let repeated = create(&f, &card, &selection, NAME, false).await.unwrap();
    assert!(repeated.can_add && repeated.add.is_none());
    calls(&f, &[HOST_LIST, CREATE, HOST_LIST, HOST_LIST]);
    let pending = add(&f, &card, &selection).await.unwrap();
    assert!(!pending.can_add);
    let pending = pending.add.unwrap();
    assert_eq!(pending.phase, PlaylistPhase::Uncertain);
    assert!(matches!(
        f.client
            .player_voice_songs_collect_prepare(&f.epoch, &f.play_id, &card, &[2, 0]),
        Err(Error::MutationUncertain)
    ));
    let read_only = status(&f, &card, &selection, false)
        .await
        .unwrap()
        .add
        .unwrap();
    assert_eq!(read_only.operation_id, pending.operation_id);
    assert_eq!(read_only.phase, PlaylistPhase::Uncertain);
    assert_eq!(
        add(&f, &card, &selection)
            .await
            .unwrap()
            .add
            .unwrap()
            .operation_id,
        pending.operation_id
    );
    let verified = status(&f, &card, &selection, true).await.unwrap();
    assert_eq!(verified.create_operation_id, first.create_operation_id);
    let verified = verified.add.unwrap();
    assert_eq!(verified.operation_id, pending.operation_id);
    assert_eq!(verified.phase, PlaylistPhase::Confirmed);
    calls(
        &f,
        &[
            HOST_LIST, CREATE, HOST_LIST, HOST_LIST, DETAIL, ADD, DETAIL, DETAIL,
        ],
    );
    let trace = f.http.calls();
    let created_wire = &trace
        .iter()
        .find(|call| call.path == CREATE)
        .unwrap()
        .payload;
    assert_eq!(created_wire["privacy"], "0");
}

#[tokio::test]
async fn fresh_detail_does_not_add_to_changed_or_foreign_creation() {
    for (key, value) in [
        ("name", json!("新歌单已改名")),
        ("privacy", json!(0)),
        ("id", json!(92)),
        ("userId", json!(76)),
        ("creator", json!({"userId":76})),
        ("specialType", json!(5)),
        ("trackIds", json!([{"id":777,"v":-1}])),
    ] {
        let mut detail = created_detail(true, &[]);
        detail["playlist"][key] = value;
        let (f, card) = setup(
            &[COOKIE],
            [
                host_list(&[]).into(),
                json!({"code":200,"id":91}).into(),
                fresh_list(true),
                detail.into(),
            ],
        )
        .await;
        let selection = prepare(&f, &card, &[2, 0]).selection_receipt.unwrap();
        assert!(
            matches!(
                create(&f, &card, &selection, NAME, true).await,
                Err(Error::StaleResponse
                    | Error::InvalidInput(_)
                    | Error::State(_)
                    | Error::Response(_))
            ),
            "{key}"
        );
        let read = status(&f, &card, &selection, false).await.unwrap();
        assert_eq!(read.create_phase, PlaylistPhase::Confirmed);
        assert!(read.add.is_none());
        calls(&f, &[HOST_LIST, CREATE, HOST_LIST, DETAIL]);
        assert_eq!(f.store.values("25", "playlist-mutations").unwrap().len(), 1);
    }
}

#[tokio::test]
async fn retiring_the_card_during_create_keeps_its_durable_record_without_add() {
    let (held, release) = gate();
    let (f, card) = setup(&[COOKIE], [host_list(&[]).into(), held, fresh_list(true)]).await;
    let selection = prepare(&f, &card, &[2, 0]).selection_receipt.unwrap();
    let pending = create(&f, &card, &selection, NAME, true);
    tokio::pin!(pending);
    assert!(
        poll_fn(|cx| Poll::Ready(pending.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    f.http.wait(STARTUP.len() + 3).await;
    f.client
        .player_action(&f.epoch, PlayerAction::Stop)
        .await
        .unwrap();
    assert!(release.send(Ok(reply(json!({"code":200,"id":91})))).is_ok());
    assert!(matches!(pending.await, Err(Error::StaleResponse)));
    assert!(matches!(
        add(&f, &card, &selection).await,
        Err(Error::StaleResponse)
    ));
    calls(&f, &[HOST_LIST, CREATE, HOST_LIST]);
    let records = f.store.values("25", "playlist-mutations").unwrap();
    assert_eq!(records.len(), 1);
    let record = records.values().next().unwrap();
    assert_eq!(record["phase"], "confirmed");
    assert_eq!(record["command"]["kind"], "create");
}

#[tokio::test]
async fn group_authority_alone_does_not_admit_create_with_foreign_scoped_cookie() {
    let (f, card) = setup(
        &[
            COOKIE,
            "MUSIC_U=foreign; Path=/eapi/playlist/create; Secure",
        ],
        [],
    )
    .await;
    let selection = prepare(&f, &card, &[2, 0]).selection_receipt.unwrap();
    assert!(matches!(
        create(&f, &card, &selection, NAME, true).await,
        Err(Error::StaleResponse)
    ));
    calls(&f, &[]);
    assert!(
        f.store
            .values("25", "playlist-mutations")
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn membership_readback_does_not_turn_a_known_add_failure_into_success() {
    let (f, card) = setup(
        &[COOKIE],
        [
            host_list(&[]).into(),
            json!({"code":200,"id":91}).into(),
            fresh_list(true),
            created_detail(true, &[]).into(),
            json!({"code":403}).into(),
            created_detail(true, &[303, 301]).into(),
        ],
    )
    .await;
    let selection = prepare(&f, &card, &[2, 0]).selection_receipt.unwrap();
    let created = create(&f, &card, &selection, NAME, true).await.unwrap();
    assert!(created.add_failed && !created.can_add);
    let operation = created.add.unwrap();
    assert_eq!(operation.phase, PlaylistPhase::Confirmed);
    let checked = status(&f, &card, &selection, true).await.unwrap();
    assert!(checked.add_failed && !checked.can_add);
    assert_eq!(checked.add.unwrap().operation_id, operation.operation_id);
    calls(&f, &[HOST_LIST, CREATE, HOST_LIST, DETAIL, ADD, DETAIL]);
}
