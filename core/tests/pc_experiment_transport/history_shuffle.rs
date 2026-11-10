//! Closed signed-PC fixture for the one native resume anchor in History Add.
use super::{ASSIGNMENT_PATH, HttpResponse, playback, reply, vinyl_http_with_credentials};
use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{PlaySelection, PlayerAction},
    },
    history::{HistoryKind, HistoryRecord, HistoryService, HistorySyncAction},
    membership::Quality,
    playback::PlaybackMode,
    protocol::{api::identifier, http::ApiOrigin},
};
use serde_json::{Value, json};
use std::sync::Arc;

const RESUME: &str = "/api/dj/user/setting/resumeplay/get";
const RIGHTS: &str = "/api/dj/play/url/check";
const URL: &str = "/api/song/enhance/player/url/v1";
const OWNER: &str = "71";
const COOKIE: &str = "MUSIC_U=synthetic-history-shuffle; Path=/; Secure";

fn voice(id: u64) -> Value {
    json!({"id":id,"name":format!("合成节目 {id}"),"duration":120000,
        "mainSong":{"id":id+100,"duration":120000},
        "radio":{"id":800,"name":"合成播单"}})
}

#[tokio::test]
async fn history_shuffle_requires_the_first_programs_current_account_checkpoint() {
    for (case, owner, media, group, enabled, anchored) in [
        ("matched", OWNER, "151", "t1", true, true),
        ("other-account", "72", "151", "t1", true, false),
        ("wrong-main-track", OWNER, "251", "t1", true, false),
        ("setting-off", OWNER, "151", "t1", false, false),
        ("retired-during-read", OWNER, "151", "t1", true, false),
        ("known-control", OWNER, "151", "c", true, true),
    ] {
        let root = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(&root.path().join("host"), ApiOrigin::Pc).unwrap();
        let store = registry.store();
        let replies = (0..20).map(|_| {
            let store = store.clone();
            HttpResponse::ForCall(Box::new(move |call| {
            let value = match call.path.as_str() {
                ASSIGNMENT_PATH => json!({"code":200,"data":[{"expName":"Podcast_ResumePlaySwitch","expGroupName":group}]}),
                RESUME => {
                    assert_eq!(call.payload["radioId"], "800");
                    if case == "retired-during-read" {
                        store.put(OWNER, "podcast-latest-voice-v1", "800", &json!({
                            "id":"52", "mainTrackId":"152", "name":"合成节目 52", "time":12, "duration":120,
                        })).unwrap();
                    }
                    json!({"code":200,"data":{"resumePlayValue":u8::from(enabled)}})
                }
                RIGHTS | "/api/dj/program/detail" => {
                    let field = if call.path == RIGHTS { "voiceId" } else { "id" };
                    let id: u64 = identifier(&call.payload[field]).unwrap().parse().unwrap();
                    assert!([51,52].contains(&id));
                    if call.path == RIGHTS {
                        json!({"code":200,"data":{"voiceId":id,"songId":id+100,"canPlay":true}})
                    } else {
                        json!({"code":200,"program":voice(id)})
                    }
                }
                URL => {
                    let ids: Vec<String> = serde_json::from_str(call.payload["ids"].as_str().unwrap()).unwrap();
                    assert_eq!(ids.len(), 1);
                    assert!(["151","152"].contains(&ids[0].as_str()));
                    json!({"code":200,"data":[{"id":ids[0],"code":200,"url":"https://m1.music.126.net/history.mp3?fixture=private","md5":"abc","level":"standard","expi":120}]})
                }
                _ => panic!("unexpected history shuffle request: {}", call.path),
            };
            Ok(reply(value))
            }))
        });
        let (native, http, _) = vinyl_http_with_credentials(root.path(), &[COOKIE], replies);
        let session = Arc::new(AccountSession::new(native.clone(), Some(store.clone())));
        session
            .adopt(native, json!({"userId":71}), &session.token().unwrap())
            .unwrap();
        let token = session.token().unwrap();
        let epoch = token.generation.to_string();
        let history = HistoryService::new(session.clone()).unwrap();
        let client = MusicClient::with_session(session, registry).with_audio_factory(|| {
            Ok(playback::audio_engine(
                Arc::new(playback::Device::default()),
            ))
        });
        client
            .history_sync_preference(
                &epoch,
                HistorySyncAction::Set {
                    enabled: false,
                    confirmed_first_disable: true,
                },
            )
            .unwrap();
        client.refresh_pc_presentation().await.unwrap();
        for (id, at) in [(51, 200), (52, 100)] {
            history
                .record_for(
                    &token,
                    HistoryKind::Voice,
                    HistoryRecord::local(HistoryKind::Voice, voice(id), at).unwrap(),
                )
                .unwrap();
        }
        store
            .put(
                owner,
                "podcast-latest-voice-v1",
                "800",
                &json!({
                    "id":"51", "mainTrackId":media, "name":"合成节目 51", "time":41, "duration":120,
                }),
            )
            .unwrap();
        let snapshot = client
            .history_page(&epoch, HistoryKind::Voice, false)
            .await
            .unwrap()
            .value;
        assert_eq!(
            snapshot
                .entries
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            ["51", "52"]
        );
        let queue = client
            .player_action(
                &epoch,
                PlayerAction::Mode {
                    mode: PlaybackMode::Random,
                },
            )
            .await
            .unwrap();
        assert_eq!(queue.queue_len, 0);
        let ticket = client
            .prepare_play_selection(&epoch, &queue.queue_revision, queue.play_id.as_deref())
            .unwrap();
        let result = client
            .commit_prepared_selection(
                &epoch,
                &ticket,
                PlaySelection::HistoryAdd {
                    history_kind: HistoryKind::Voice,
                    revision: snapshot.revision,
                    ids: vec!["51".into(), "52".into()],
                    quality: Quality::Standard,
                },
            )
            .await;
        if case == "retired-during-read" {
            assert!(matches!(
                result,
                Err(Error::QueueChanged | Error::StaleResponse)
            ));
            assert_eq!(client.player_queue(&epoch).await.unwrap().tracks.len(), 0);
            assert!(
                http.calls()
                    .iter()
                    .all(|call| matches!(call.path.as_str(), ASSIGNMENT_PATH | RESUME))
            );
            continue;
        }
        let result = result.unwrap();
        assert!(result.error.is_none(), "{case}");
        assert_eq!(result.mode, PlaybackMode::Random, "{case}");
        let selected = result
            .track
            .as_ref()
            .and_then(|row| row.voice_id.as_deref())
            .unwrap();
        if anchored {
            assert_eq!(selected, "51", "{case}");
            assert_eq!(result.position, Some(41.0), "{case}");
        } else {
            assert!(["51", "52"].contains(&selected), "{case}");
            assert_eq!(result.position, Some(0.0), "{case}");
        }
        let queue = client.player_queue(&epoch).await.unwrap();
        assert_eq!(
            queue
                .tracks
                .iter()
                .map(|row| row.track.voice_id.as_deref().unwrap())
                .collect::<Vec<_>>(),
            ["51", "52"]
        );
        let calls = http.calls();
        let rights = calls.iter().position(|call| call.path == RIGHTS).unwrap();
        let settings = calls
            .iter()
            .enumerate()
            .filter(|(_, call)| call.path == RESUME)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        match case {
            "matched" => assert!(matches!(settings.as_slice(), [only] if *only < rights)),
            "known-control" => assert!(settings.is_empty()),
            "setting-off" => assert!(
                matches!(settings.as_slice(), [preflight, playback] if *preflight < rights && *playback > rights)
            ),
            _ => assert!(matches!(settings.as_slice(), [playback] if *playback > rights)),
        }
        for call in calls.iter().filter(|call| call.path == RESUME) {
            assert!(
                call.cookie
                    .as_deref()
                    .is_some_and(|value| value.contains("MUSIC_U=synthetic-history-shuffle"))
            );
        }
    }
}
