//! M622 looks up the clicked voice's completed task, independent of the group.
use super::{HistoryRowMenuTarget, HistoryVoiceFileMenu, MusicClient, row_menu};
use crate::{
    Error, Result,
    account::SessionToken,
    download::completed::{CompletedDownloads, DownloadFileCandidate, DownloadFileReveal},
    history::{HistoryKind, HistoryRecord, HistoryService, HistorySnapshot},
    podcast::Episode,
    protocol::wire::serialization::js_truthy,
    storage::PlaybackDownloadKind,
};
use std::{
    path::Path,
    sync::Mutex,
    time::{Duration, Instant},
};

const LIFETIME: Duration = Duration::from_secs(180);

#[derive(Default)]
pub(in crate::client) struct HistoryVoiceMenuHost(Mutex<(u64, Option<Offer>)>);

struct Offer {
    receipt: String,
    issued: Instant,
    token: SessionToken,
    target: HistoryRowMenuTarget,
    file: DownloadFileCandidate,
}

pub(super) enum VoiceFileObservation {
    Download,
    Reveal(Box<DownloadFileCandidate>),
    Unavailable,
}

impl HistoryVoiceMenuHost {
    pub(super) fn begin(&self) -> Result<u64> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        state.0 = state
            .0
            .checked_add(1)
            .ok_or(Error::State("history row menu generation exhausted"))?;
        state.1 = None;
        Ok(state.0)
    }

    pub(super) fn publish(
        &self,
        sequence: u64,
        token: SessionToken,
        target: HistoryRowMenuTarget,
        observation: Option<VoiceFileObservation>,
    ) -> Result<Option<HistoryVoiceFileMenu>> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if state.0 != sequence {
            return Err(Error::StaleResponse);
        }
        Ok(observation.map(|observation| match observation {
            VoiceFileObservation::Download => HistoryVoiceFileMenu::Download,
            VoiceFileObservation::Unavailable => HistoryVoiceFileMenu::Unavailable,
            VoiceFileObservation::Reveal(file) => {
                let receipt = uuid::Uuid::new_v4().simple().to_string();
                state.1 = Some(Offer {
                    receipt: receipt.clone(),
                    issued: Instant::now(),
                    token,
                    target,
                    file: *file,
                });
                HistoryVoiceFileMenu::Reveal { receipt }
            }
        }))
    }

    fn take(
        &self,
        token: &SessionToken,
        target: &HistoryRowMenuTarget,
        receipt: &str,
    ) -> Result<Offer> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if !state.1.as_ref().is_some_and(|offer| {
            &offer.token == token
                && &offer.target == target
                && offer.receipt == receipt
                && offer.issued.elapsed() < LIFETIME
        }) {
            return Err(Error::QueueChanged);
        }
        state.1.take().ok_or(Error::QueueChanged)
    }
}

pub(in crate::client) fn episode(row: &HistoryRecord) -> Result<Episode> {
    let item = Episode::from_official(&row.resource)?;
    if item.id != row.id {
        return Err(Error::QueueChanged);
    }
    if ["offlineId", "local_id", "fileId", "cloud_song_id"]
        .iter()
        .any(|field| js_truthy(&row.resource[*field]))
    {
        return Err(Error::InvalidInput(
            "history row is not an ordinary program",
        ));
    }
    Ok(item)
}

pub(in crate::client) fn selection<'a>(
    snapshot: &'a HistorySnapshot,
    target: &HistoryRowMenuTarget,
) -> Result<row_menu::ResolvedRows<'a>> {
    if target.history_kind != HistoryKind::Voice {
        return Err(Error::InvalidInput("history menu requires a program"));
    }
    if snapshot.kind != target.history_kind || snapshot.revision != target.revision {
        return Err(Error::QueueChanged);
    }
    row_menu::resolve(snapshot, &target.clicked_id, &target.selected_ids)
}

impl MusicClient {
    pub(super) async fn observe_history_voice_file(
        &self,
        token: &SessionToken,
        clicked: Option<Episode>,
    ) -> Result<VoiceFileObservation> {
        let Some(clicked) = clicked else {
            return Ok(VoiceFileObservation::Unavailable);
        };
        if token.uid.is_none() {
            return Ok(VoiceFileObservation::Download);
        }
        let session = self.session.clone();
        let token = token.clone();
        tokio::task::spawn_blocking(move || {
            let store = session.persistent_store()?;
            let complete = session.with_token(&token, |owner| {
                Ok(store
                    .playback_download_rows(owner, &clicked.id, PlaybackDownloadKind::Voice)?
                    .into_iter()
                    .filter(|row| row.status == "complete")
                    .collect::<Vec<_>>())
            })?;
            let observation = match complete.as_slice() {
                [] => VoiceFileObservation::Download,
                [row] if !row.moving => {
                    // The history row can lack a main-song ID. The native
                    // task still proves the program; a present media ID
                    // must match the completed publication too.
                    let files = CompletedDownloads::new(session.clone())?;
                    match files.program_reveal_candidate(
                        &row.id,
                        &clicked.id,
                        clicked.song_id.as_deref(),
                    ) {
                        Ok(file) => VoiceFileObservation::Reveal(Box::new(file)),
                        Err(Error::StaleResponse) => return Err(Error::StaleResponse),
                        Err(_) => VoiceFileObservation::Unavailable,
                    }
                }
                _ => VoiceFileObservation::Unavailable,
            };
            session.check_generation(&token)?;
            Ok(observation)
        })
        .await
        .map_err(|_| Error::State("history program file discovery stopped"))?
    }

    /// The exact menu-time publication is consumed once; no later file or
    /// different task can replace it even if it belongs to the same program.
    pub async fn history_row_voice_file_reveal(
        &self,
        epoch: &str,
        target: HistoryRowMenuTarget,
        receipt: &str,
    ) -> Result<HistoryVoiceFileReveal> {
        let token = self.player_token(epoch)?;
        let history = HistoryService::new(self.session.clone())?;
        let snapshot = history.snapshot_for(&token, HistoryKind::Voice)?;
        episode(selection(&snapshot, &target)?.clicked)?;
        let offer = self.history_voice_menu.take(&token, &target, receipt)?;
        let file = tokio::task::spawn_blocking(move || offer.file.verify().map_err(file_error))
            .await
            .map_err(|_| Error::State("history program file validation stopped"))??;
        let reveal = HistoryVoiceFileReveal {
            token,
            revision: target.revision,
            file,
        };
        reveal.check_current(self)?;
        Ok(reveal)
    }
}

fn file_error(error: Error) -> Error {
    match error {
        Error::Media(_) | Error::Io(_) | Error::State(_) | Error::MutationBusy => {
            Error::DownloadedFileMissing
        }
        other => other,
    }
}

/// Native file-manager handoff. This is deliberately not a serializable DTO.
pub struct HistoryVoiceFileReveal {
    token: SessionToken,
    revision: String,
    file: DownloadFileReveal,
}

impl HistoryVoiceFileReveal {
    pub fn path(&self) -> &Path {
        self.file.path()
    }

    pub fn check_current(&self, client: &MusicClient) -> Result<()> {
        client.session.check_generation(&self.token)?;
        HistoryService::new(client.session.clone())?.with_matching_revision(
            &self.token,
            HistoryKind::Voice,
            &self.revision,
            |_| Ok(()),
        )?;
        // Leave the pathname and publication check nearest the OS dispatch.
        self.file.check_current().map_err(file_error)
    }
}
