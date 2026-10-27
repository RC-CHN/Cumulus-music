mod support;
use cumulus_core::{Error, listen_together::*};
use serde_json::{Value, json};
use std::sync::Arc;
use support::ScriptApi;
fn command(kind: CommandKind, seconds: f64) -> PlayCommand {
    PlayCommand::at_seconds(kind, seconds, PlayState::Pause, "91", "91").unwrap()
}
fn room() -> Value {
    json!({"roomId":"opaque-room","creatorId":1,"chatRoomId":"31","agoraChannelId":"41","roomUsers":[{"userId":1,"nickname":"one"}]})
}

#[test]
fn progress_duplicate_window_expires_without_extending_on_suppressed_requests() {
    let allowed = vec!["91".into()];
    let progress = command(CommandKind::Progress, 23.456);
    let mut sync = PlaybackSync::default();
    assert!(
        sync.outgoing(progress.clone(), &allowed, false, 1000)
            .unwrap()
            .is_some()
    );
    for now in [1000, 1250, 1999] {
        assert!(
            sync.outgoing(progress.clone(), &allowed, false, now)
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(sync.latest.as_ref().unwrap().at_ms, 1000);
    // A busy host may process the next identical request after this boundary;
    // the official policy permits reporting it instead of extending the guard.
    assert!(
        sync.outgoing(progress, &allowed, false, 2000)
            .unwrap()
            .is_some()
    );
    assert_eq!(sync.sent.len(), 2);
}

#[test]
fn delayed_snapshot_cannot_rewind_received_sequence_or_refresh_duplicate_clock() {
    let allowed = vec!["91".into()];
    let mut incoming = command(CommandKind::Progress, 12.0);
    incoming.server_seq = Some(42);
    incoming.user_id = Some("2".into());
    let mut sync = PlaybackSync::default();
    assert!(
        sync.incoming(incoming.clone(), &allowed, Some("91"), true, 2000)
            .unwrap()
    );
    sync.restore(incoming.clone(), "1", 5000).unwrap();
    assert_eq!(sync.received.as_ref().unwrap().at_ms, 2000);
    let mut older = incoming.clone();
    older.progress = 0;
    older.server_seq = Some(40);
    sync.restore(older, "1", 6000).unwrap();
    assert_eq!(sync.latest.as_ref().unwrap().command, incoming);
    let mut delayed = incoming;
    delayed.server_seq = Some(41);
    delayed.progress = 20000;
    assert!(
        !sync
            .incoming(delayed, &allowed, Some("91"), true, 9000)
            .unwrap()
    );
}

#[test]
fn executed_official_filters_windows_and_version_comparison() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/together-official.json")).unwrap();
    assert_eq!(fixture["filterCases"].as_array().unwrap().len(), 22);
    for c in fixture["filterCases"].as_array().unwrap() {
        let state = PlaybackSync {
            client_seq: 0,
            latest: serde_json::from_value(c["latest"].clone()).unwrap(),
            received: serde_json::from_value(c["received"].clone()).unwrap(),
            sent: serde_json::from_value(c["sent"].clone()).unwrap(),
        };
        let direction = if c["direction"] == "in" {
            Direction::In
        } else {
            Direction::Out
        };
        assert_eq!(
            state.filtered(
                direction,
                &serde_json::from_value(c["command"].clone()).unwrap(),
                &serde_json::from_value::<Vec<String>>(c["allowed"].clone()).unwrap(),
                c["playing"].as_bool().unwrap(),
                c["now"].as_u64().unwrap()
            ),
            c["filtered"].as_bool().unwrap(),
            "{}",
            c["name"]
        );
    }
    for c in fixture["windows"].as_array().unwrap() {
        let ids: Vec<String> = serde_json::from_value(c["ids"].clone()).unwrap();
        assert_eq!(
            json!(shared_window(&ids, c["current"].as_str()).unwrap()),
            c["expected"]
        );
    }
    for c in fixture["versions"].as_array().unwrap() {
        assert_eq!(
            versions_equal(
                &Version::parse(&c["a"]).unwrap(),
                &Version::parse(&c["b"]).unwrap()
            ),
            c["expected"]
        );
    }
    assert_eq!(fixture["versions"][3]["official"], false);
    assert_eq!(fixture["versions"][3]["expected"], false);
}
#[test]
fn explicit_zero_seek_preserves_wire_echo_and_allowed_song_guards() {
    let allowed = vec!["91".into()];
    let mut prior = command(CommandKind::Play, 12.0);
    prior.play_status = PlayState::Play;
    prior.user_id = Some("1".into());
    let mut zero = command(CommandKind::Progress, 0.0);
    zero.play_status = PlayState::Play;
    let mut sync = PlaybackSync::default();
    sync.restore(prior, "1", 1000).unwrap();
    assert!(
        sync.outgoing(zero.clone(), &allowed, true, 3000)
            .unwrap()
            .is_none()
    );
    let sent = sync
        .outgoing_seek(zero.clone(), &allowed, true, 3000)
        .unwrap()
        .unwrap();
    assert_eq!(sent.command_type, CommandKind::Goto);
    assert_eq!(sent.progress, 0);
    assert!(
        sync.outgoing_seek(zero.clone(), &allowed, true, 3100)
            .unwrap()
            .is_none()
    );
    let mut echo = sent;
    echo.user_id = Some("2".into());
    sync.received = Some(TimedCommand {
        command: echo,
        at_ms: 4000,
    });
    assert!(
        sync.outgoing_seek(zero.clone(), &allowed, true, 4100)
            .unwrap()
            .is_none()
    );
    assert!(
        sync.outgoing_seek(zero.clone(), &[], true, 8000)
            .unwrap()
            .is_none()
    );
    assert!(
        sync.outgoing_seek(command(CommandKind::Next, 0.0), &allowed, true, 8000)
            .is_err()
    );
    assert!(
        sync.outgoing_seek(zero, &allowed, true, 8000)
            .unwrap()
            .is_some()
    );
}
#[test]
fn version_vectors_preserve_peers_and_window_deduplicates_after_cropping() {
    let versions =
        Version::parse(&json!([{"userId":1,"version":9},{"userId":"2","version":5}])).unwrap();
    assert_eq!(Version::increment(&versions, "2").unwrap()[0].version, 9);
    assert_eq!(Version::increment(&versions, "2").unwrap()[1].version, 6);
    assert_eq!(Version::increment(&versions, "3").unwrap()[2].version, 1);
    assert!(Version::parse(&json!([{"userId":1,"version":1},{"userId":1,"version":2}])).is_err());
    assert!(
        Version::increment(
            &[Version {
                user_id: "1".into(),
                version: u64::MAX
            }],
            "1"
        )
        .is_err()
    );
    let ids = vec!["91".into(); 1001];
    assert_eq!(shared_window(&ids, Some("91")).unwrap(), vec!["91"]);
    assert!(shared_window(&["/private/local.mp3".into()], None).is_err());
}
#[test]
fn zero_restore_ordering_target_and_echo_are_distinct_guards() {
    assert_eq!(
        command(CommandKind::Progress, 0.0).command_type,
        CommandKind::Goto
    );
    assert_eq!(command(CommandKind::Progress, 1.2345).progress, 1235);
    for seconds in [f64::NAN, -1.0, f64::INFINITY] {
        assert!(
            PlayCommand::at_seconds(CommandKind::Play, seconds, PlayState::Play, "91", "91")
                .is_err()
        );
    }
    let mut sync = PlaybackSync::default();
    let allowed = vec!["91".into(), "92".into()];
    let mut restored = command(CommandKind::Pause, 0.0);
    restored.client_seq = 7;
    restored.user_id = Some("1".into());
    restored.server_seq = Some(0);
    sync.restore(restored, "1", 1000).unwrap();
    assert_eq!(sync.latest.as_ref().unwrap().command.progress, 0);
    assert_eq!(sync.client_seq, 7);
    let mut incoming = command(CommandKind::Progress, 5.0);
    incoming.user_id = Some("2".into());
    incoming.client_seq = 8;
    incoming.server_seq = Some(12);
    assert!(
        sync.incoming(incoming.clone(), &allowed, Some("91"), false, 7000)
            .unwrap()
    );
    incoming.progress = 9000;
    assert!(
        !sync
            .incoming(incoming.clone(), &allowed, Some("91"), false, 9000)
            .unwrap()
    );
    incoming.server_seq = Some(13);
    incoming.target_song_id = "92".into();
    assert!(
        !sync
            .incoming(incoming.clone(), &allowed, Some("91"), false, 9000)
            .unwrap()
    );
    incoming.command_type = CommandKind::Goto;
    assert!(
        sync.incoming(incoming.clone(), &allowed, Some("91"), false, 9000)
            .unwrap()
    );
    incoming.server_seq = None;
    assert!(
        sync.incoming(incoming, &allowed, Some("92"), false, 11000)
            .is_err()
    );
}
#[test]
fn initial_empty_snapshot_nullable_random_and_room_members_are_checked() {
    assert_eq!(
        TogetherSnapshot::from_official(&json!({})).unwrap(),
        TogetherSnapshot::default()
    );
    let good = json!({"playlist":{"displayList":{"result":[91,92],"changed":true},"randomList":null,"playMode":"ORDER_LOOP","replace":true,"version":[{"userId":1,"version":1}]},"playCommand":{"commandType":"PAUSE","progress":0,"playStatus":"PAUSE","formerSongId":91,"targetSongId":"91","clientSeq":1,"serverSeq":0,"userId":1}});
    let snapshot = TogetherSnapshot::from_official(&good).unwrap();
    assert_eq!(snapshot.play_command.unwrap().progress, 0);
    assert!(snapshot.playlist.unwrap().random.is_none());
    let mut bad = good;
    bad["playlist"]["randomList"] = json!({"result":[91,93]});
    assert!(TogetherSnapshot::from_official(&bad).is_ok());
    bad["playlist"]["playMode"] = json!("RANDOM");
    assert!(TogetherSnapshot::from_official(&bad).is_err());
    let r = Room::from_official(&room()).unwrap();
    assert_eq!(r.role_for("1").unwrap(), RoomRole::Waiting);
    assert!(r.role_for("2").is_err());
    let mut r = room();
    r["roomUsers"] = json!([{ "userId":1},{"userId":1}]);
    assert!(Room::from_official(&r).is_err());
}
#[tokio::test]
async fn exact_native_payloads_and_tokens_never_serialize_or_debug_secrets() {
    let api = Arc::new(ScriptApi::new([
        json!({"code":200,"data":{"result":true}}).into(),
        json!({"code":200,"data":{"result":true}}).into(),
        json!({"code":200,"data":{"success":true}}).into(),
        json!({"code":200,"data":{"uid":"1","accId":"acc","token":"im-secret"}}).into(),
        json!({"code":200,"data":{"token":"rtc-secret","expireTime":9999999}}).into(),
        json!({"code":488,"data":{}}).into(),
    ]));
    let client = TogetherApi::new(api.clone());
    let room = Room::from_official(&room()).unwrap();
    let cmd = ListCommand {
        command_type: ListCommandKind::Replace,
        version: vec![Version {
            user_id: "9007199254740993".into(),
            version: 1,
        }],
        play_mode: SharedMode::OrderLoop,
        anchor_song_id: String::new(),
        anchor_position: -1,
        random_list: vec!["91".into()],
        display_list: vec!["91".into()],
    };
    client.list_command(&room.room_id, &cmd).await.unwrap();
    client
        .heartbeat(&room.room_id, &command(CommandKind::Pause, 0.0))
        .await
        .unwrap();
    client.end(&room.room_id, Some(3)).await.unwrap();
    let im = client.im_credentials().await.unwrap();
    let rtc = client.rtc_credentials(&room).await.unwrap();
    assert_eq!(im.token, "im-secret");
    assert!(!format!("{im:?} {rtc:?}").contains("secret"));
    assert!(matches!(
        client.sync_notice(&room.room_id).await,
        Err(Error::Service { .. })
    ));
    let calls = api.calls();
    assert!(calls.iter().all(|c| !c.cache));
    let wire: Value =
        serde_json::from_str(calls[0].payload["playlistParam"].as_str().unwrap()).unwrap();
    assert_eq!(wire["version"][0]["userId"], 9007199254740993u64);
    assert_eq!(calls[1].payload["progress"], 1);
    assert_eq!(calls[2].payload["shareInfo"], "{\"thisListenCount\":3}");
    assert_eq!(
        calls[4].payload,
        json!({"roomId":"opaque-room","channelId":"41"})
    );
}
