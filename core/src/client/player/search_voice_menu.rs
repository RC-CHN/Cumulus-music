//! Ordinary search voice menu Play, separate from the row transport toggle.
use super::{
    MusicClient, PlaybackCommand, PlaybackStatus, PlayerResourceKind, PlayerView, identity,
    prepared_selection::PreparedSelection,
    project,
    queue_playlist::{QUEUE_LIMIT, insert_collection_bound},
};
use crate::{
    Error, Result,
    account::SessionToken,
    client::dto::SearchVoiceMenuTarget,
    playback::{
        controller::{QueueTransport, QueueUpdate},
        engine::Cancellation,
    },
    podcast::PodcastApi,
};
use serde_json::{Value, json};

fn matches_voice(resource: &Value, id: &str) -> bool {
    identity::kind(resource) == PlayerResourceKind::Voice && resource["voice_id"] == id
}

impl MusicClient {
    pub(super) async fn play_search_voice_menu(
        &self,
        token: &SessionToken,
        target: &SearchVoiceMenuTarget,
        prepared: &PreparedSelection,
    ) -> Result<PlayerView> {
        let id = target.voice_id.as_str();
        let (api, row) = self.search_voice_menu_playback_source(token, target)?;
        let room = self.together_detached_guard(token);
        let authority = Cancellation::new(move || !row.is_cancelled() && !room.is_cancelled());
        authority.check()?;
        let controller = self.player.controller().await?;
        let (before, queue) = controller.queue_with_state(token).await?;
        prepared.check(self, &before)?;
        authority.check()?;
        if before.fm || queue.intelligence.is_some() {
            return Err(Error::State("search voice menu requires an ordinary queue"));
        }
        let same = before.play_id.is_some()
            && before
                .selected
                .as_ref()
                .is_some_and(|entry| matches_voice(&entry.resource, id));
        let command = if same
            && matches!(
                before.state,
                PlaybackStatus::Playing | PlaybackStatus::Buffering
            ) {
            return project(&before, token);
        } else if same && before.state == PlaybackStatus::Paused {
            PlaybackCommand::Pause(false)
        } else {
            if matches!(
                before.state,
                PlaybackStatus::Preparing | PlaybackStatus::Loading
            ) {
                return Err(Error::State("search voice queue is not settled"));
            }
            if queue.items.len() >= QUEUE_LIMIT
                && !queue
                    .items
                    .iter()
                    .any(|entry| matches_voice(&entry.resource, id))
            {
                return Err(Error::InvalidInput("playback queue is full"));
            }
            let preparing = {
                let player = self.pending_preparation_guard(prepared, controller.clone());
                let authority = authority.clone();
                Cancellation::new(move || !player.is_cancelled() && !authority.is_cancelled())
            };
            let requested = json!(id);
            let episode = self
                .session
                .run_for(token, |api, _| async move {
                    PodcastApi::new(api).episode(&requested).await
                })
                .await?;
            preparing.check()?;
            if episode.raw["grayDisplay"] == true {
                return Err(Error::State("selected podcast episode is unavailable"));
            }
            let mut resource = episode.playback_resource()?;
            resource["song"] = episode.download_metadata()?;
            resource["level"] = json!("standard");
            let first_source = controller
                .source_resolver()
                .prepare_scoped_episode(&resource, token, &api, &preparing, Some(&episode))
                .await?;
            preparing.check()?;
            let source = json!({"kind":"voice","id":id,"scene":"search","pldScene":"audio"});
            let mut queue =
                insert_collection_bound(queue, vec![resource], source, false, true, true);
            queue.current = Some(
                queue
                    .items
                    .iter()
                    .find(|entry| matches_voice(&entry.resource, id))
                    .ok_or(Error::QueueChanged)?
                    .entry_id
                    .clone(),
            );
            PlaybackCommand::PlayPodcastBrowse {
                update: QueueUpdate {
                    expected: before.fence(),
                    queue,
                    transport: Some(QueueTransport {
                        position: None,
                        paused: false,
                    }),
                },
                first_source,
            }
        };
        let state = self
            .submit_selection_with_source(
                token,
                command,
                prepared.observed,
                None,
                Some(prepared),
                Some(authority),
            )
            .await?;
        self.session.check_generation(token)?;
        project(&state, token)
    }
}
