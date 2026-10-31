//! Isolated persisted AI states built by the real domain actor. This is not
//! the missing production liked-list/rights/recommendation start coordinator.
use cumulus_core::{
    Result,
    account::AccountSession,
    catalog::intelligence::{IntelligenceRequest, IntelligenceStart},
    client::{MusicClient, dto::PlayerAction},
    playback::{
        PlaybackMode, QueueEntry, QueueSnapshot,
        controller::{IntelligenceUpdate, PlaybackCommand, PlaybackController},
        intelligence::{IntelligenceAction, IntelligenceBegin, IntelligenceFailure},
        source::SourceResolver,
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, sync::Arc};

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_INTELLIGENCE_UI_PROOF").is_some()
}
fn song(id: u64) -> Value {
    json!({"id":id,"dt":120000,"song":{"id":id,"dt":120000,"name":format!("合成心动歌曲 {id} · A quiet afternoon"),"ar":[{"id":20,"name":"Cumulus"}],"al":{"name":"心动恢复验证"}}})
}
pub async fn control(
    client: &MusicClient,
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
    root: &Path,
    kind: &str,
) -> Result<()> {
    assert!(enabled() && ["seeded", "active", "fallback"].contains(&kind));
    let token = session.token()?;
    assert_eq!(token.uid.as_deref(), Some("2"));
    let epoch = token.generation.to_string();
    client.player_action(&epoch, PlayerAction::Stop).await?;
    let controller = PlaybackController::start(
        session.clone(),
        store,
        Arc::new(SourceResolver::new(session, None)),
        Box::new(super::relay_fixture::NativeAudio::open(
            root.join("relay-synthetic.wav"),
        )?),
    )?;
    controller
        .command(
            &token,
            PlaybackCommand::Replace {
                resources: (7..=9).map(song).collect(),
                index: 1,
                source: json!({"kind":"playlist","id":"1"}),
            },
        )
        .await?;
    controller
        .command(&token, PlaybackCommand::Mode(PlaybackMode::Sequential))
        .await?;
    let items: Vec<_> = (7..=9)
        .map(|id| QueueEntry {
            entry_id: format!("liked-{id}"),
            resource: song(id),
            source: json!({"kind":"playlist","id":"1"}),
        })
        .collect();
    let liked = QueueSnapshot {
        order: items.iter().map(|e| e.entry_id.clone()).collect(),
        current: Some("liked-7".into()),
        items,
        mode: PlaybackMode::OrderLoop,
        ..Default::default()
    };
    let before = controller
        .command(&token, PlaybackCommand::Snapshot)
        .await?;
    let seed = if kind == "fallback" { 90 } else { 8 };
    let started = controller
        .command(
            &token,
            PlaybackCommand::Intelligence(IntelligenceUpdate {
                expected: before.fence(),
                action: IntelligenceAction::Begin(Box::new(IntelligenceBegin {
                    request: IntelligenceRequest {
                        playlist_id: "1".into(),
                        song_id: seed.to_string(),
                        start_music_id: "7".into(),
                        count: 3,
                        start: IntelligenceStart::PlayOne,
                    },
                    seed_resource: song(seed),
                    seed_source: json!({"kind":"playlist","id":"1"}),
                    liked,
                    preserve_current_queue: true,
                    liked_metadata: vec![],
                })),
            }),
        )
        .await?;
    let id = started.intelligence.as_ref().unwrap().id.clone();
    if kind != "seeded" {
        let action = if kind == "active" {
            IntelligenceAction::Complete {
                session_id: id,
                resources: vec![song(10), song(11)],
                recommendations: BTreeMap::new(),
            }
        } else {
            IntelligenceAction::Fail {
                session_id: id,
                failure: IntelligenceFailure::RequestFailed,
            }
        };
        controller
            .command(
                &token,
                PlaybackCommand::Intelligence(IntelligenceUpdate {
                    expected: started.fence(),
                    action,
                }),
            )
            .await?;
    }
    controller.command(&token, PlaybackCommand::Stop).await?;
    drop(controller);
    client.player_action(&epoch, PlayerAction::Restore).await?;
    Ok(())
}
