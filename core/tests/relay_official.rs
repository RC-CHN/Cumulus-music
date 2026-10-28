mod support;
use cumulus_core::{Error, relay::*};
use serde_json::{Value, json};
use std::sync::Arc;
use support::ScriptApi;

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/relay-official.json")).unwrap()
}
fn resource(id: &Value, kind: &str) -> RelayResource {
    RelayResource::new(id, RelayResourceKind::from_official(kind).unwrap()).unwrap()
}
fn machine(input: &Value) -> RelayMachine {
    let source = &input["state"]["playingSource"];
    let data = &source["data"];
    let source = match source["kind"].as_str().unwrap() {
        "playlist" => RelaySource::Collection {
            source_type: RelaySourceKind::Playlist,
            id: data["playlistId"].as_str().unwrap().into(),
        },
        "album" => RelaySource::Collection {
            source_type: RelaySourceKind::Album,
            id: data["albumId"].as_str().unwrap().into(),
        },
        "voicelist" => RelaySource::Collection {
            source_type: RelaySourceKind::Podcast,
            id: data["voicelistId"].as_str().unwrap().into(),
        },
        "tracks" => RelaySource::Resources {
            resources: data["trackIds"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| resource(v, "song"))
                .collect(),
        },
        "voices" => RelaySource::Resources {
            resources: data["voiceIds"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| resource(v, "voice"))
                .collect(),
        },
        "mix" => RelaySource::Resources {
            resources: data["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| resource(&v["id"], v["type"].as_str().unwrap()))
                .collect(),
        },
        other => panic!("unexpected source {other}"),
    };
    let state = &input["state"]["playingState"];
    let current = &state["currentPlayingItem"];
    let mut raw = serde_json::to_value(
        RelayMachine::with_session(
            source,
            resource(&json!("11"), "song"),
            state["sessionId"].as_str().unwrap().into(),
        )
        .unwrap(),
    )
    .unwrap();
    // Snapshot-window fixtures intentionally use queue entry IDs (including
    // zero/missing) rather than resource IDs. The caller supplies that separately.
    if cumulus_core::protocol::api::identifier(&current["id"]).is_ok() {
        raw["current"] = json!(resource(&current["id"], current["type"].as_str().unwrap()));
    }
    raw["progress_ms"] = state["progress"].clone();
    if input["state"]["playingCommands"].is_array() {
        raw["commands"] = input["state"]["playingCommands"].clone();
    }
    let machine: RelayMachine = serde_json::from_value(raw).unwrap();
    machine.validate().unwrap();
    machine
}
fn value(upload: RelayUpload) -> Value {
    match upload {
        RelayUpload::Source { value, .. }
        | RelayUpload::State(value)
        | RelayUpload::Commands(value) => value,
    }
}

#[test]
fn frozen_official_sources_commands_modes_windows_and_errors() {
    let fixture = fixture();
    let mut compared = 0;
    for case in fixture["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        if name.starts_with("wire-") || name.starts_with("pull-") {
            continue;
        }
        let input = &case["input"];
        let expected = &case["output"];
        let mut state = machine(input);
        let mode = RelayMode::from_official(
            input["store"]["playing"]["playingMode"]
                .as_str()
                .unwrap_or("playOrder"),
        );
        let upload = if name.starts_with("source-") {
            state.source_upload(mode, false).unwrap()
        } else if name == "state-default-order" {
            state.state_upload(mode).unwrap()
        } else if name == "commands-full-history" {
            state.commands_upload().unwrap()
        } else if name.starts_with("snapshot-window-") {
            let items = input["store"]["playingList"]["curPlayingList"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| SnapshotItem {
                    entry_id: row["id"].as_str().unwrap().into(),
                    resource: resource(&row["resourceId"], row["resourceType"].as_str().unwrap()),
                })
                .collect::<Vec<_>>();
            let playing = &input["store"]["playing"]["curPlaying"];
            state
                .force_snapshot_with_session(
                    SnapshotFrame {
                        items: &items,
                        selected_entry: input["state"]["playingState"]["currentPlayingItem"]["id"]
                            .as_str(),
                        current: resource(
                            &playing["resourceId"],
                            playing["resourceType"].as_str().unwrap(),
                        ),
                        config: &RelayConfig::from_data(&input["state"]["handoffConfig"]).unwrap(),
                        mode,
                    },
                    "ssssssssssss".into(),
                )
                .unwrap()
        } else if name.starts_with("queue-command-") {
            let event = &input["events"][0];
            let data = &event["data"];
            match event["kind"].as_str().unwrap() {
                "delete" => state
                    .delete(vec![resource(&data["itemId"], "song")])
                    .unwrap(),
                "insertTracks" | "insertVoices" => state
                    .insert(
                        data["ids"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|id| {
                                resource(
                                    id,
                                    if event["kind"] == "insertVoices" {
                                        "voice"
                                    } else {
                                        "song"
                                    },
                                )
                            })
                            .collect(),
                    )
                    .unwrap(),
                "move" => state
                    .command(RelayCommand::Move {
                        resources: vec![resource(&data["id"], data["type"].as_str().unwrap())],
                        pre_id: data["preId"].as_str().map(String::from),
                    })
                    .unwrap(),
                "clearPlaylist" => state.clear().unwrap(),
                other => panic!("unhandled event {other}"),
            }
        } else {
            let effect = input["steps"][0]["effect"].as_str().unwrap();
            let upload = match effect {
                "uploadPlayNewSource" => state
                    .source_upload(mode, input["steps"][0]["payload"]["isRetransmit"] == true)
                    .unwrap(),
                "uploadPlayingState" => state.state_upload(mode).unwrap(),
                "uploadPlayingCommands" => state.commands_upload().unwrap(),
                other => panic!("unhandled effect {other}"),
            };
            let response = &input["responses"]
                .as_object()
                .unwrap()
                .values()
                .next()
                .unwrap()["throw"];
            let error = Error::service(SOURCE_PATH, response["code"].clone(), response.clone());
            let force = expected["dispatch"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["type"] == "forceUploadPlaySource");
            assert_eq!(upload.requires_snapshot(&error), force, "{name}");
            assert!(
                !upload.requires_snapshot(&Error::Transport("lost".into())),
                "{name}"
            );
            compared += 1;
            continue;
        };
        assert_eq!(value(upload), expected["calls"][0]["args"][0], "{name}");
        compared += 1;
    }
    assert_eq!(compared, 28);
}

#[test]
fn frozen_official_pull_expansion_and_music_restart_vs_voice_resume() {
    let fixture = fixture();
    let mut compared = 0;
    for case in fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["name"].as_str().unwrap().starts_with("pull-"))
    {
        let plan = PullPlan::from_data(&case["input"]["responses"]["pull"]["data"]).unwrap();
        let expected = &case["output"];
        let action = &expected["dispatch"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["type"] == "async:action/doAction")
            .unwrap()["payload"]["data"];
        match &plan.selection {
            PullSelection::Collection { source_type, id } => {
                assert_eq!(json!(id), action["resource"]["id"]);
                assert_eq!(
                    action["resourceType"],
                    match source_type {
                        RelaySourceKind::Playlist => "playlist",
                        RelaySourceKind::Album => "album",
                        RelaySourceKind::Podcast => "audio",
                    }
                );
            }
            PullSelection::Resources { resources } => {
                assert_eq!(
                    json!(resources.iter().map(|r| &r.id).collect::<Vec<_>>()),
                    json!(
                        action["from"]["listData"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|r| &r["id"])
                            .collect::<Vec<_>>()
                    )
                );
                assert!(resources.iter().all(|r| r.kind
                    == if action["resourceType"] == "voice" {
                        RelayResourceKind::Voice
                    } else {
                        RelayResourceKind::Song
                    }));
            }
        }
        assert_eq!(
            json!(plan.current.as_ref().unwrap().id),
            action["from"]["assignedPlayId"]
        );
        let seconds = expected["breakpoints"][0]["seconds"]
            .as_f64()
            .unwrap_or(0.0);
        assert_eq!(plan.position_ms as f64 / 1000.0, seconds);
        let mode = expected["dispatch"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["type"] == "playing/switchPlayingMode")
            .and_then(|d| d["payload"]["playingMode"].as_str())
            .and_then(RelayMode::from_official);
        assert_eq!(plan.mode, mode);
        compared += 1;
    }
    assert_eq!(compared, 8);
}

#[tokio::test]
async fn frozen_official_wire_payloads_and_local_vs_remote_device() {
    let fixture = fixture();
    let mut compared = 0;
    for case in fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["name"].as_str().unwrap().starts_with("wire-"))
    {
        let mock = Arc::new(ScriptApi::new([json!({"code":200,"data":{"relayMode":"snapshot","resources":[{"id":"11","type":"song"}]}}).into()]));
        let api = RelayApi::new(mock.clone());
        let input = &case["input"];
        match input["wire"].as_str().unwrap() {
            "No" => {
                api.alert().await.unwrap();
            }
            "To" => {
                api.setting().await.unwrap();
            }
            "Po" => api
                .update_setting("local-fixture-device", input["value"].as_bool().unwrap())
                .await
                .unwrap(),
            "Ao" => {
                api.pull(&RelayAlert::from_official(input["value"].clone()).unwrap())
                    .await
                    .unwrap();
            }
            "Io" => api
                .submit(&RelayUpload::State(input["value"].clone()))
                .await
                .unwrap(),
            "Mo" => api
                .submit(&RelayUpload::Commands(input["value"].clone()))
                .await
                .unwrap(),
            other => panic!("unhandled wrapper {other}"),
        }
        let calls = mock.calls();
        let expected = &case["output"]["wire"][0];
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].path.trim_start_matches('/'),
            expected["path"].as_str().unwrap().trim_start_matches('/')
        );
        assert_eq!(calls[0].payload, expected["payload"], "{}", case["name"]);
        assert!(!calls[0].cache);
        compared += 1;
    }
    assert_eq!(compared, 6);
}
