//! M/3085 playback from one verified returning-song notification receipt.
use super::{
    MusicClient, PlaybackCommand, identity,
    queue_playlist::{insert_collection_bound, queue_resource_for_track},
};
use crate::{
    Error, Result,
    account::SessionToken,
    client::{dto::TrackView, focus::Owner},
    playback::{
        controller::{QueueTransport, QueueUpdate},
        engine::Cancellation,
    },
};
use serde_json::json;
use std::sync::atomic::Ordering;

impl MusicClient {
    pub(in crate::client) async fn play_copyright_notification(
        &self,
        owner: &SessionToken,
        rows: Vec<TrackView>,
        authorized: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Result<()> {
        let first = rows
            .first()
            .ok_or(Error::State("no playable returning songs"))?
            .id
            .clone();
        if !authorized() {
            return Err(Error::StaleResponse);
        }
        let focus = self.focus.snapshot()?;
        let selection = self.player.selection.clone();
        let previous = selection.load(Ordering::Acquire);
        let controller = self.player.controller().await?;
        let (before, queue) = controller.queue_with_state(owner).await?;
        if queue.fm_source.is_some()
            || queue
                .intelligence
                .as_ref()
                .is_some_and(|context| context.active())
        {
            return Err(Error::State(
                "returning songs require an ordinary playback queue",
            ));
        }
        let quality = before.quality_preferences.preferences.quality;
        let mut queue = insert_collection_bound(
            queue,
            rows.iter()
                .map(|row| queue_resource_for_track(row, quality))
                .collect(),
            json!({"kind":"copyright_reversion","scene":"copyrightReversionPush","resourceType":"track","originalResourceType":"trackList"}),
            false,
            true,
            true,
        );
        queue.current = Some(
            queue
                .items
                .iter()
                .find(|row| identity::online_song(&row.resource, &first))
                .ok_or(Error::State("returning song was not inserted"))?
                .entry_id
                .clone(),
        );
        let _room = self
            .together
            .gate
            .try_lock()
            .map_err(|_| Error::MutationBusy)?;
        if self.together_attached(owner, None)? {
            return Err(Error::State(
                "returning songs require an ordinary playback queue",
            ));
        }
        if !authorized() {
            return Err(Error::StaleResponse);
        }
        let attempt = previous
            .checked_add(1)
            .ok_or(Error::State("playback selection exhausted"))?;
        selection
            .compare_exchange(previous, attempt, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| Error::StaleResponse)?;
        let lease = if focus.owner == Some(Owner::Audio) {
            focus.generation
        } else {
            self.claim_native_media(owner, Owner::Audio, focus).await?
        };
        let focus = self.focus.clone();
        let expected = before.fence();
        controller
            .command_guarded(
                owner,
                PlaybackCommand::UpdateQueue(QueueUpdate {
                    expected: expected.clone(),
                    queue,
                    transport: Some(QueueTransport {
                        position: None,
                        paused: false,
                    }),
                }),
                expected,
                Cancellation::new(move || {
                    selection.load(Ordering::Acquire) == attempt
                        && focus.check(lease, Owner::Audio).is_ok()
                        && authorized()
                }),
            )
            .await?;
        self.session.check_generation(owner)
    }
}
