//! A catalog queue menu binds the chosen download or current local mapping.
use super::{MusicClient, queue_collection};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    download::{
        completed::{CompletedDownloads, DownloadFileCandidate, DownloadFileReveal, fs},
        file_identity::{Identity, identity},
    },
    library::{LibraryItem, LocalLibrary, LocalRevision},
    playback::QueueEntry,
    storage::PlaybackDownloadKind,
};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use uuid::Uuid;

const LIFETIME: Duration = Duration::from_secs(180);

#[derive(Default)]
pub(super) struct Host(Mutex<(u64, Option<Offer>)>);

impl Host {
    pub(super) fn begin(&self) -> Result<u64> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        state.0 = state
            .0
            .checked_add(1)
            .ok_or(Error::State("queue menu revision exhausted"))?;
        state.1 = None;
        Ok(state.0)
    }

    pub(super) fn offer(
        &self,
        menu: u64,
        token: SessionToken,
        version: u64,
        entry: QueueEntry,
        candidate: Option<Candidate>,
    ) -> Result<Option<String>> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if state.0 != menu {
            return Ok(None);
        }
        let Some(candidate) = candidate else {
            return Ok(None);
        };
        let key = Uuid::new_v4().simple().to_string();
        state.1 = Some(Offer {
            key: key.clone(),
            issued: Instant::now(),
            token,
            version,
            entry,
            candidate,
        });
        Ok(Some(key))
    }

    fn take(&self, token: &SessionToken, version: u64, entry_id: &str, key: &str) -> Result<Offer> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if !state.1.as_ref().is_some_and(|offer| {
            &offer.token == token
                && offer.version == version
                && offer.entry.entry_id == entry_id
                && offer.key == key
                && offer.issued.elapsed() < LIFETIME
        }) {
            return Err(Error::QueueChanged);
        }
        state.1.take().ok_or(Error::QueueChanged)
    }
}

struct Offer {
    key: String,
    issued: Instant,
    token: SessionToken,
    version: u64,
    entry: QueueEntry,
    candidate: Candidate,
}

pub(super) enum Candidate {
    Download(Box<DownloadFileCandidate>),
    Mapped(Box<Mapped>),
}

enum Source {
    Download(Box<DownloadFileReveal>),
    Mapped(Box<Mapped>),
}

pub(super) struct Mapped {
    library: LocalLibrary,
    song: String,
    item: LibraryItem,
    file: Identity,
    parent: Identity,
}

impl Mapped {
    fn select(
        library: LocalLibrary,
        session: &AccountSession,
        token: &SessionToken,
        song: String,
    ) -> Result<Option<Self>> {
        let mut matching = library.files_for_online_song(session, token, &song)?;
        if matching.len() != 1 {
            return Ok(None);
        }
        let item = matching.pop().ok_or(Error::QueueChanged)?;
        let file = identity(&item.path)?;
        let parent = identity(item.path.parent().ok_or(Error::QueueChanged)?)?;
        let mapped = Self {
            library,
            song,
            item,
            file,
            parent,
        };
        mapped.check(session, token)?;
        Ok(Some(mapped))
    }

    fn check(&self, session: &AccountSession, token: &SessionToken) -> Result<()> {
        self.library
            .check_mapped_file(session, token, &self.song, &self.item)?;
        let path = &self.item.path;
        fs::parent(path, &self.parent)?;
        if path.canonicalize()? != *path
            || identity(path)? != self.file
            || matches!(&self.item.revision, LocalRevision::Cue { sheet, .. } if sheet.canonicalize()? != *sheet)
        {
            return Err(Error::QueueChanged);
        }
        session.check_generation(token)
    }
}

pub(super) fn discover(
    session: Arc<AccountSession>,
    token: SessionToken,
    song: String,
) -> Result<(bool, Option<Candidate>)> {
    if token.uid.is_none() {
        return Ok((false, None));
    }
    let store = session.persistent_store()?;
    let owner = session.with_token(&token, |owner| Ok(owner.to_owned()))?;
    let finished = store
        .playback_download_rows(&owner, &song, PlaybackDownloadKind::Song)?
        .into_iter()
        .filter(|row| row.status == "complete")
        .collect::<Vec<_>>();
    let candidate = match finished.as_slice() {
        [] => LocalLibrary::new(store)
            .and_then(|library| Mapped::select(library, &session, &token, song))
            .ok()
            .flatten()
            .map(|mapped| Candidate::Mapped(Box::new(mapped))),
        [row] if !row.moving => row.path.as_deref().and_then(|path| {
            CompletedDownloads::new(session.clone())
                .and_then(|files| files.catalog_song_reveal_candidate(&row.id, &song, path))
                .ok()
                .map(|candidate| Candidate::Download(Box::new(candidate)))
        }),
        _ => None,
    };
    session.check_generation(&token)?;
    Ok((!finished.is_empty(), candidate))
}

impl MusicClient {
    pub async fn player_queue_catalog_file_reveal(
        &self,
        epoch: &str,
        revision: &str,
        entry_id: &str,
        receipt: &str,
    ) -> Result<QueueCatalogFileReveal> {
        let token = self.player_token(epoch)?;
        let version = revision
            .parse()
            .map_err(|_| Error::InvalidInput("invalid queue file revision"))?;
        let offer = self
            .player
            .queue_catalog_reveal
            .take(&token, version, entry_id, receipt)?;
        let live = self
            .ordinary_file_queue_entry(&token, version, entry_id)
            .await?;
        if live != offer.entry || !queue_collection::catalog_row(&live.resource) {
            return Err(Error::QueueChanged);
        }
        let session = self.session.clone();
        let check = token.clone();
        let source = tokio::task::spawn_blocking(move || -> Result<_> {
            match offer.candidate {
                Candidate::Download(candidate) => Ok(Source::Download(Box::new(
                    candidate.verify().map_err(file_error)?,
                ))),
                Candidate::Mapped(mapped) => {
                    mapped.check(&session, &check).map_err(file_error)?;
                    Ok(Source::Mapped(mapped))
                }
            }
        })
        .await
        .map_err(|_| Error::State("queue file validation stopped"))??;
        if self
            .ordinary_file_queue_entry(&token, version, entry_id)
            .await?
            != live
        {
            return Err(Error::QueueChanged);
        }
        Ok(QueueCatalogFileReveal {
            token,
            version,
            entry: live,
            source,
        })
    }
}

fn file_error(error: Error) -> Error {
    match error {
        Error::Media(_) | Error::Io(_) | Error::State(_) | Error::MutationBusy => {
            Error::State("queued catalog file is unavailable")
        }
        other => other,
    }
}

pub struct QueueCatalogFileReveal {
    token: SessionToken,
    version: u64,
    entry: QueueEntry,
    source: Source,
}

impl QueueCatalogFileReveal {
    pub fn path(&self) -> &Path {
        match &self.source {
            Source::Download(file) => file.path(),
            Source::Mapped(file) => &file.item.path,
        }
    }

    pub async fn check_current(&self, client: &MusicClient) -> Result<()> {
        client.session.check_generation(&self.token)?;
        match &self.source {
            Source::Download(file) => file.check_current().map_err(file_error)?,
            Source::Mapped(file) => file
                .check(&client.session, &self.token)
                .map_err(file_error)?,
        }
        if client
            .ordinary_file_queue_entry(&self.token, self.version, &self.entry.entry_id)
            .await?
            != self.entry
        {
            return Err(Error::QueueChanged);
        }
        Ok(())
    }
}
