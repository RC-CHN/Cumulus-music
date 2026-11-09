//! Recovered M/2270 home prefill. Recommending and opening media are separate
//! authority boundaries; this lane only commits queue metadata.
use super::{MusicClient, project::recall_songs, request::HomeFeed};
use crate::{
    Error, Result,
    account::SessionToken,
    client::{
        focus::{FocusSnapshot, Owner},
        player::queue_playlist::queue_resource_for_track,
    },
    playback::{
        RECALL_GUIDE_SHOWN, RECALL_STATE,
        controller::{
            PlaybackCommand, PlaybackController, PlaybackFence, PlaybackStatus, RecallPrefill,
        },
        engine::Cancellation,
    },
    protocol::api::Api,
};
use serde::Serialize;
use serde_json::json;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use tokio::time::Instant;

const RECALL: &str = "PH-PC-recallAndNewPlaylist";
const HOT: &str = "PH-PC-RecallHomeContent";
const ECMP: &str = "PH-PC-newEcpm";
const GUIDE_TIME: Duration = Duration::from_secs(3);
const GUIDE_EXPIRY: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(in crate::client) struct Host {
    home_generation: AtomicU64,
    guide: Mutex<Option<Guide>>,
}
impl Host {
    pub(in crate::client) fn begin(&self) -> Result<u64> {
        self.home_generation
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .map(|old| old + 1)
            .map_err(|_| Error::State("home recall generation exhausted"))
    }

    fn current(&self, generation: u64) -> bool {
        self.home_generation.load(Ordering::Acquire) == generation
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecallGuide {
    receipt: String,
    queue_revision: String,
}
struct Guide {
    token: SessionToken,
    issued: Instant,
    receipt: String,
}

pub(super) struct Assignment {
    revision: String,
    pub(super) hot: bool,
    pub(super) prefill: bool,
}

pub(super) struct Capture {
    controller: Arc<PlaybackController>,
    fence: PlaybackFence,
    focus: FocusSnapshot,
    selection: u64,
    quality: crate::membership::Quality,
}

impl MusicClient {
    /// Only the shared verified native assignment can supply these buckets.
    /// A missing/stale assignment never starts a second experiment request.
    pub(super) fn home_recall_assignment(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
    ) -> Option<Assignment> {
        let revision = api.verified_pc_experiment_revision().ok().flatten()?;
        let groups = self
            .pc_experiments
            .observed_groups(token, api, &revision)
            .ok()
            .flatten()?;
        Some(Assignment {
            revision,
            hot: groups.group(HOT) == Some("t1") && groups.group(ECMP) == Some("c"),
            prefill: groups.group(RECALL) == Some("t1"),
        })
    }

    pub(super) async fn capture_home_recall(
        &self,
        token: &SessionToken,
    ) -> Result<Option<Capture>> {
        let focus = self.focus.snapshot()?;
        if !matches!(focus.owner, None | Some(Owner::Audio))
            || self.login_pending()?
            || self.together_attached(token, None)?
        {
            return Ok(None);
        }
        let selection = self.player.selection.load(Ordering::Acquire);
        // If the main player is concurrently opening, this shares the same
        // OnceCell/factory. It does not acquire playback focus or load a source.
        let controller = self.player.controller().await?.clone();
        let (before, queue) = controller.queue_with_state(token).await?;
        if before.state != PlaybackStatus::Idle
            || before.play_id.is_some()
            || before.selected.is_some()
            || !queue.items.is_empty()
            || queue.fm_source.is_some()
            || queue.intelligence.is_some()
        {
            return Ok(None);
        }
        Ok(Some(Capture {
            controller,
            fence: before.fence(),
            focus,
            selection,
            quality: before.quality_preferences.preferences.quality,
        }))
    }

    pub(super) async fn try_home_recall(
        &self,
        token: &SessionToken,
        api: Arc<dyn Api>,
        generation: u64,
        assignment: Assignment,
        capture: Capture,
        feed: &HomeFeed,
    ) -> Result<Option<RecallGuide>> {
        let resources: Vec<_> = recall_songs(feed, assignment.hot)
            .iter()
            .map(|row| queue_resource_for_track(row, capture.quality))
            .collect();
        if resources.is_empty() || self.login_pending()? {
            return Ok(None);
        }
        let _room = self
            .together
            .gate
            .try_lock()
            .map_err(|_| Error::MutationBusy)?;
        if self.together_attached(token, None)? {
            return Ok(None);
        }
        let focus = self.focus.clone();
        let selection = self.player.selection.clone();
        let host = self.home_recall.clone();
        let experiments = self.pc_experiments.clone();
        let expected_token = token.clone();
        let expected_api = api.clone();
        let authority = Cancellation::new(move || {
            host.current(generation)
                && selection.load(Ordering::Acquire) == capture.selection
                && focus.check_snapshot(capture.focus).is_ok()
                && expected_api
                    .verified_pc_experiment_revision()
                    .ok()
                    .flatten()
                    .as_deref()
                    == Some(&assignment.revision)
                && experiments
                    .observed_groups(&expected_token, &expected_api, &assignment.revision)
                    .is_ok_and(|groups| {
                        groups.is_some_and(|groups| {
                            groups.group(RECALL) == Some("t1")
                                && assignment.hot
                                    == (groups.group(HOT) == Some("t1")
                                        && groups.group(ECMP) == Some("c"))
                        })
                    })
        });
        let state = capture
            .controller
            .command_guarded(
                token,
                PlaybackCommand::RecallPrefill(RecallPrefill {
                    api: api.clone(),
                    resources,
                    login_check: Box::new(self.auth.pending_guard()),
                }),
                capture.fence,
                authority,
            )
            .await?;
        let store = self.session.persistent_store()?;
        self.session.with_verified_profile(token, &api, |owner, _| {
            if !self.home_recall.current(generation)
                || store.get(owner, RECALL_STATE, RECALL_GUIDE_SHOWN)? == Some(json!(true))
            {
                return Ok(None);
            }
            let receipt = uuid::Uuid::new_v4().simple().to_string();
            *self
                .home_recall
                .guide
                .lock()
                .map_err(|_| Error::LockPoisoned)? = Some(Guide {
                token: token.clone(),
                issued: Instant::now(),
                receipt: receipt.clone(),
            });
            Ok(Some(RecallGuide {
                receipt,
                queue_revision: state.queue_revision.to_string(),
            }))
        })
    }

    /// Display-only acknowledgment; there is no way to request a prefill or
    /// mark another account via IPC. No request is sent to a service.
    pub fn acknowledge_home_recall_guide(&self, epoch: &str, receipt: &str) -> Result<()> {
        let token = self.playlist_token(epoch)?;
        let store = self.session.persistent_store()?;
        self.session.with_token(&token, |owner| {
            let mut slot = self
                .home_recall
                .guide
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            slot.as_ref()
                .filter(|guide| {
                    guide.token == token
                        && guide.receipt == receipt
                        && (GUIDE_TIME..GUIDE_EXPIRY).contains(&guide.issued.elapsed())
                })
                .ok_or(Error::StaleResponse)?;
            store.put(owner, RECALL_STATE, RECALL_GUIDE_SHOWN, &json!(true))?;
            *slot = None;
            Ok(())
        })
    }
}
