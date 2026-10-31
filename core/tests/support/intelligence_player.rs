#[path = "playback/mod.rs"]
mod native;
pub use native::*;
#[path = "intelligence_input.rs"]
mod input;
use cumulus_core::{
    Result,
    playback::{
        controller::{IntelligenceUpdate, PlaybackCommand, PlaybackSnapshot},
        intelligence::IntelligenceAction,
    },
};
pub use input::*;

pub async fn act(f: &Fixture, action: IntelligenceAction) -> Result<PlaybackSnapshot> {
    let before = f.command(PlaybackCommand::Snapshot).await?;
    f.command(PlaybackCommand::Intelligence(IntelligenceUpdate {
        expected: before.fence(),
        action,
    }))
    .await
}
