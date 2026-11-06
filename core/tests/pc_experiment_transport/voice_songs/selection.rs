use super::{
    ApiOrigin, CARD, COOKIE, Error, Fixture, PlaybackMode, PlaybackStatus, PlayerAction,
    PlayerResourceKind, Quality, QueueClickPreferencesCommand, SONG, STARTUP, VOICE,
};
use cumulus_core::client::dto::{VoiceSongSelectionAction, VoiceSongSelectionKind};
use serde_json::{Value, json};

const MEDIA: &str = "/api/song/enhance/player/url/v1";

fn mixed_card() -> Value {
    json!({"code":200,"tracks":[
        {"id":301,"name":"第一行","album":{},"privilege":{"pl":320000}},
        {"id":777,"name":"无权限","album":{},"privilege":{"pl":0}},
        {"id":SONG,"name":"同数字普通曲目","album":{},"privilege":{"pl":320000}},
        {"id":302,"name":"重复较早行","album":{},"privilege":{"pl":320000}},
        {"id":302,"name":"重复先选行","album":{},"privilege":{"pl":320000}},
        {"id":778,"name":"负版权状态","album":{},"privilege":{"pl":320000,"st":-200}},
    ]})
}

fn media() -> Value {
    let data = ["301", "302", SONG].map(|id| {
        json!({"id":id,"code":200,"url":format!("https://m1.music.126.net/song-{id}.mp3"),
            "md5":"def","level":"standard","expi":120})
    });
    json!({"code":200,"data":data})
}

fn selection(
    kind: VoiceSongSelectionKind,
    index: usize,
    indices: Vec<usize>,
) -> VoiceSongSelectionAction {
    VoiceSongSelectionAction {
        kind,
        index,
        indices,
    }
}

fn mixed(kind: VoiceSongSelectionKind) -> VoiceSongSelectionAction {
    selection(kind, 2, vec![4, 1, 2, 0, 3, 5])
}

async fn receipt(f: &Fixture) -> String {
    f.client
        .player_voice_songs(&f.epoch, &f.play_id)
        .await
        .unwrap()
        .receipt
        .unwrap()
}

#[tokio::test]
async fn voice_songs_selection_play_uses_order_anchor_and_double_click_preference() {
    use VoiceSongSelectionKind::{DoubleClick, MenuPlay};
    for (kind, replace, expected) in [
        (MenuPlay, true, vec![SONG, "302", SONG, "301"]),
        (MenuPlay, false, vec![SONG, "302", SONG, "301"]),
        (DoubleClick, true, vec!["302", SONG, "301"]),
        (DoubleClick, false, vec![SONG, SONG]),
    ] {
        let f = Fixture::new(
            &[COOKIE],
            [mixed_card().into()]
                .into_iter()
                .chain(std::iter::repeat_with(|| media().into()).take(8)),
        )
        .await;
        if !replace {
            f.client
                .queue_click_preferences_command(
                    &f.epoch,
                    QueueClickPreferencesCommand::Change {
                        expected_revision: "0".into(),
                        replace_queue: false,
                    },
                )
                .unwrap();
        }
        let old_queue = f.client.player_queue(&f.epoch).await.unwrap();
        let old_entry = old_queue.current.as_deref().unwrap();
        let receipt = receipt(&f).await;
        let result = f
            .client
            .player_voice_songs_selection(
                &f.epoch,
                &f.play_id,
                &receipt,
                mixed(kind),
                Quality::Standard,
            )
            .await
            .unwrap();
        assert_eq!(result.state, PlaybackStatus::Playing);
        assert_ne!(result.play_id.as_deref(), Some(f.play_id.as_str()));
        assert_eq!(result.resource_kind, Some(PlayerResourceKind::Catalog));
        assert_eq!(
            result.track.unwrap().id,
            SONG,
            "start at the trigger, not the first selected row"
        );
        let queue = f.client.player_queue(&f.epoch).await.unwrap();
        assert_eq!(
            queue
                .tracks
                .iter()
                .map(|row| row.track.id.as_str())
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(queue.current, result.entry_id);
        assert_ne!(queue.current.as_deref(), Some(old_entry));
        if kind == MenuPlay || !replace {
            assert_eq!(queue.tracks[0].entry_id, old_entry);
            assert_eq!(queue.tracks[0].track.voice_id.as_deref(), Some(VOICE));
        } else {
            assert!(queue.tracks.iter().all(|row| row.track.voice_id.is_none()));
        }
        let calls = f.http.calls();
        assert_eq!(calls[STARTUP.len()].path, CARD);
        assert_eq!(calls[STARTUP.len() + 1].path, MEDIA);
        let selected_ids: Vec<Value> =
            serde_json::from_str(calls[STARTUP.len() + 1].payload["ids"].as_str().unwrap())
                .unwrap();
        assert_eq!(selected_ids.len(), 1);
        assert_eq!(
            cumulus_core::protocol::api::identifier(&selected_ids[0]).unwrap(),
            SONG
        );
        assert_eq!(f.device.state.lock().unwrap().loads, 2);
    }
}

#[tokio::test]
async fn voice_songs_selection_next_keeps_paused_decoder_and_pins_order_in_shuffle() {
    use VoiceSongSelectionKind::NextPlay;
    let f = Fixture::new(
        &[COOKIE],
        [mixed_card().into()]
            .into_iter()
            .chain(std::iter::repeat_with(|| media().into()).take(8)),
    )
    .await;
    let receipt = receipt(&f).await;
    f.client
        .player_action(
            &f.epoch,
            PlayerAction::Mode {
                mode: PlaybackMode::Random,
            },
        )
        .await
        .unwrap();
    f.client
        .player_action(&f.epoch, PlayerAction::Seek { seconds: 25.0 })
        .await
        .unwrap();
    let before = f
        .client
        .player_action(&f.epoch, PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let counters = || {
        let engine = f.device.state.lock().unwrap();
        (
            engine.loads,
            engine.stops,
            engine.pauses.clone(),
            engine.view.entry_id,
        )
    };
    let native_before = counters();
    let first = f
        .client
        .player_voice_songs_selection(
            &f.epoch,
            &f.play_id,
            &receipt,
            mixed(NextPlay),
            Quality::Standard,
        )
        .await
        .unwrap();
    assert_eq!(first.state, PlaybackStatus::Paused);
    assert_eq!(first.mode, PlaybackMode::Random);
    assert_eq!(first.play_id, before.play_id);
    assert_eq!(first.entry_id, before.entry_id);
    assert_eq!(first.position, Some(25.0));
    assert_eq!(counters(), native_before);
    let queue = f.client.player_queue(&f.epoch).await.unwrap();
    assert_eq!(queue.current, before.entry_id);
    assert_eq!(queue.tracks[0].track.voice_id.as_deref(), Some(VOICE));
    assert_eq!(queue.tracks[1].track.name, "重复先选行");
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|row| row.track.id.as_str())
            .collect::<Vec<_>>(),
        [SONG, "302", SONG, "301"]
    );
    let ordered_entries = queue
        .tracks
        .iter()
        .map(|row| row.entry_id.as_str())
        .collect::<Vec<_>>();
    let persisted = cumulus_core::playback::PlaybackQueue::new(Some(f.store.clone()), "25")
        .unwrap()
        .snapshot();
    assert_eq!(persisted.order, ordered_entries);
    let same = f
        .client
        .player_voice_songs_selection(
            &f.epoch,
            &f.play_id,
            &receipt,
            mixed(NextPlay),
            Quality::Standard,
        )
        .await
        .unwrap();
    assert_eq!(
        same.queue_revision, first.queue_revision,
        "repeating the same selection does not write again"
    );
    assert_eq!(counters(), native_before);
}

#[tokio::test]
async fn voice_songs_selection_rejects_invalid_anchors_and_stale_receipts_without_transport() {
    use VoiceSongSelectionKind::{MenuPlay, NextPlay};
    let f = Fixture::new(&[COOKIE], [mixed_card().into(), mixed_card().into()]).await;
    let old = receipt(&f).await;
    let current = receipt(&f).await;
    let initial = serde_json::to_value(f.client.player_queue(&f.epoch).await.unwrap()).unwrap();
    for action in [
        selection(MenuPlay, 2, vec![]),
        selection(MenuPlay, 2, vec![2; 101]),
        selection(MenuPlay, 2, vec![2, 2]),
        selection(MenuPlay, 2, vec![3, 4]),
        selection(MenuPlay, 2, vec![2, usize::MAX]),
    ] {
        assert!(matches!(
            f.client
                .player_voice_songs_selection(
                    &f.epoch,
                    &f.play_id,
                    &current,
                    action,
                    Quality::Standard
                )
                .await,
            Err(Error::InvalidInput("invalid vinyl episode song selection"))
        ));
    }
    for (kind, index) in [(MenuPlay, 1), (MenuPlay, 5), (NextPlay, 5)] {
        assert!(matches!(
            f.client
                .player_voice_songs_selection(
                    &f.epoch,
                    &f.play_id,
                    &current,
                    selection(kind, index, vec![2, index]),
                    Quality::Standard
                )
                .await,
            Err(Error::State("vinyl episode song is unavailable online"))
        ));
    }
    assert!(matches!(
        f.client
            .player_voice_songs_selection(
                &f.epoch,
                &f.play_id,
                &old,
                mixed(NextPlay),
                Quality::Standard
            )
            .await,
        Err(Error::StaleResponse)
    ));
    let target = ApiOrigin::Pc.url();
    let revision = f.credentials.request_credentials(&target).unwrap().revision;
    f.credentials
        .record_response(
            &target,
            &["MUSIC_U=rotated-song-selection; Path=/; Secure".into()],
            None,
            &revision,
        )
        .unwrap();
    assert!(matches!(
        f.client
            .player_voice_songs_selection(
                &f.epoch,
                &f.play_id,
                &current,
                mixed(NextPlay),
                Quality::Standard
            )
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(
        initial,
        serde_json::to_value(f.client.player_queue(&f.epoch).await.unwrap()).unwrap()
    );
    assert_eq!(f.device.state.lock().unwrap().loads, 1);
    assert_eq!(f.http.calls().len(), STARTUP.len() + 2);
}
