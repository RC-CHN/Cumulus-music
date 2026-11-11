//! History AudioTitleCell -> M622 audio menu -> M3085 playback.
use super::{
    HistoryRadioMenuAction, MusicClient, SearchPodcastRowGesture,
    prepared_selection::PreparedSelection,
    search_podcast::{PodcastRowPlan, PodcastRowTarget},
};
use crate::{
    Error, Result,
    account::SessionToken,
    history::{HistoryKind, HistoryService},
    playback::engine::Cancellation,
    podcast::Radio,
};
use serde_json::json;

impl MusicClient {
    pub(super) async fn history_radio_menu_selection(
        &self,
        token: &SessionToken,
        kind: HistoryKind,
        revision: &str,
        id: &str,
        action: HistoryRadioMenuAction,
        prepared: &PreparedSelection,
    ) -> Result<(PodcastRowPlan, Cancellation)> {
        let pld_scene = match kind {
            HistoryKind::Podcast => "audio",
            HistoryKind::Audiobook => "audioBook",
            _ => return Err(Error::InvalidInput("not a radio history kind")),
        };
        if self.login_pending()? {
            return Err(Error::StaleResponse);
        }
        self.validate_queue_clear_room(token, None)?;
        let api = self.session.native_api(token)?;
        let credentials = api.search_privilege_credential_revision()?;
        let history = HistoryService::new(self.session.clone())?;
        let snapshot = history.snapshot_for(token, kind)?;
        if snapshot.revision != revision {
            return Err(Error::QueueChanged);
        }
        let row = snapshot
            .entries
            .into_iter()
            .find(|row| row.id == id)
            .ok_or(Error::QueueChanged)?;
        let radio = Radio::from_official(&row.resource)?;
        if radio.id != id
            || id.bytes().all(|digit| digit == b'0')
            || radio.raw["grayDisplay"] == true
            || radio.raw["underShelf"] == true
        {
            return Err(Error::State("history radio is unavailable"));
        }
        let session = self.session.clone();
        let owner = token.clone();
        let native = api.clone();
        let room = self.together_detached_guard(token);
        let pending = self.auth.pending_guard();
        let guard = Cancellation::new(move || {
            !room.is_cancelled()
                && pending().is_ok()
                && session.require_api(&owner, &native).is_ok()
                && native
                    .search_privilege_credential_revision()
                    .is_ok_and(|current| current == credentials)
        });
        let source = json!({
            "kind":"podcast","scope":{"kind":"radio","id":id},
            "scene":"history","pldScene":pld_scene,
        });
        let gesture = match action {
            HistoryRadioMenuAction::Play => SearchPodcastRowGesture::Menu,
            HistoryRadioMenuAction::Next => SearchPodcastRowGesture::Next,
        };
        let (plan, transport) = self
            .prepare_podcast_row_selection(
                token,
                PodcastRowTarget {
                    radio,
                    api,
                    guard,
                    source,
                },
                gesture,
                prepared,
            )
            .await?;
        // The history hash is only needed at authority boundaries, not once
        // per episode while expanding the radio list.
        let owner = token.clone();
        let revision = revision.to_owned();
        let current = Cancellation::new(move || {
            !transport.is_cancelled()
                && history
                    .snapshot_for(&owner, kind)
                    .is_ok_and(|snapshot| snapshot.revision == revision)
        });
        current.check()?;
        Ok((plan, current))
    }
}
