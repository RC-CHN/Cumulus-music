//! M556 history location: indexed original, finished Song, then local mapping.
use super::{HistoryRowMenuTarget, MusicClient, file_menu::Candidate, row_menu, song_download};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    download::{
        completed::{CompletedDownloads, DownloadFileCandidate, DownloadFileReveal, fs},
        file_identity::{Identity, identity},
    },
    history::{HistoryKind, HistoryRecord, HistoryService, RecordOrigin},
    library::{LibraryItem, LocalLibrary, LocalRevision},
    storage::PlaybackDownloadKind,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub(super) enum SongFileTarget {
    Indexed { id: String, revision: LocalRevision },
    Catalog { song: String, task: Option<String> },
    Unavailable,
}

impl SongFileTarget {
    pub(super) fn from_record(row: &HistoryRecord) -> Self {
        let raw = &row.resource;
        if let Some(id) = raw["local_id"].as_str()
            && row.origin == RecordOrigin::Local
            && row.id.strip_prefix("local:") == Some(id)
            && id.len() == 64
            && id.bytes().all(|byte| byte.is_ascii_hexdigit())
            && let Ok(revision) = serde_json::from_value(raw["local_revision"].clone())
        {
            return Self::Indexed {
                id: id.into(),
                revision,
            };
        }
        // M556's explicit local branch outranks the download or local mapping.
        // Its original filename/fileId cannot establish a Cumulus index lease.
        if raw.get("fileId").is_some() && raw.get("filename").is_some() {
            return Self::Unavailable;
        }
        use song_download::HistorySongDownloadSource::{Catalog, DownloadTask};
        let task = match song_download::row_source(row) {
            Catalog => None,
            DownloadTask(task) => Some(task.to_owned()),
            _ => return Self::Unavailable,
        };
        Self::Catalog {
            song: row.id.clone(),
            task,
        }
    }
}

pub(super) struct SongFileObservation {
    pub(super) hides_download: bool,
    pub(super) candidate: Option<SongFileCandidate>,
}

pub(super) enum SongFileCandidate {
    Download(Box<Publication<DownloadFileCandidate>>),
    Local(Box<LocalFilePin>),
}

enum SongFile {
    Download(Box<Publication<DownloadFileReveal>>),
    Local(Box<LocalFilePin>),
}

pub(super) struct Publication<F> {
    song: String,
    task: String,
    file: F,
}

enum FinishedSong {
    None,
    One {
        id: String,
        path: Option<PathBuf>,
        moving: bool,
    },
    Ambiguous,
}

fn finished_song(
    session: &AccountSession,
    token: &SessionToken,
    song: &str,
) -> Result<FinishedSong> {
    let store = session.persistent_store()?;
    session.with_token(token, |owner| {
        let mut completed = store
            .playback_download_rows(owner, song, PlaybackDownloadKind::Song)?
            .into_iter()
            .filter(|row| row.status == "complete");
        let Some(row) = completed.next() else {
            return Ok(FinishedSong::None);
        };
        if completed.next().is_some() {
            Ok(FinishedSong::Ambiguous)
        } else {
            Ok(FinishedSong::One {
                id: row.id,
                path: row.path,
                moving: row.moving,
            })
        }
    })
}

impl<F> Publication<F> {
    fn check_selected_task(&self, session: &AccountSession, token: &SessionToken) -> Result<()> {
        match finished_song(session, token, &self.song)? {
            FinishedSong::One {
                id, moving: false, ..
            } if id == self.task => Ok(()),
            _ => Err(Error::QueueChanged),
        }
    }
}

impl Publication<DownloadFileCandidate> {
    fn verify(
        self,
        session: &AccountSession,
        token: &SessionToken,
    ) -> Result<Publication<DownloadFileReveal>> {
        self.check_selected_task(session, token)?;
        Ok(Publication {
            song: self.song,
            task: self.task,
            file: self.file.verify().map_err(download_error)?,
        })
    }
}

pub(super) struct LocalFilePin {
    library: LocalLibrary,
    item: LibraryItem,
    mapped_song: Option<String>,
    identity: Identity,
    parent: Identity,
}

fn canonical_regular(path: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || path.canonicalize()? != path {
        return Err(Error::QueueChanged);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(Error::QueueChanged);
        }
    }
    Ok(())
}

impl LocalFilePin {
    fn new(library: LocalLibrary, item: LibraryItem, mapped_song: Option<String>) -> Result<Self> {
        let path = &item.path;
        canonical_regular(path)?;
        Ok(Self {
            identity: identity(path)?,
            parent: identity(path.parent().ok_or(Error::QueueChanged)?)?,
            library,
            item,
            mapped_song,
        })
    }

    fn check(&self, session: &AccountSession, token: &SessionToken) -> Result<()> {
        if let Some(song) = &self.mapped_song {
            self.library
                .check_unique_online_file(session, token, song, &self.item)?;
        } else {
            self.library.check_indexed_file(&self.item)?;
        }
        let path = &self.item.path;
        fs::parent(path, &self.parent)?;
        canonical_regular(path)?;
        if identity(path)? != self.identity {
            return Err(Error::QueueChanged);
        }
        if let LocalRevision::Cue { sheet, .. } = &self.item.revision {
            canonical_regular(sheet)?;
        }
        if let Some(song) = &self.mapped_song
            && !matches!(finished_song(session, token, song)?, FinishedSong::None)
        {
            return Err(Error::QueueChanged);
        }
        session.check_generation(token)
    }
}

fn discover(
    session: &Arc<AccountSession>,
    token: &SessionToken,
    target: SongFileTarget,
) -> Result<SongFileObservation> {
    let store = session.persistent_store()?;
    let observation = match target {
        SongFileTarget::Unavailable => SongFileObservation {
            hides_download: true,
            candidate: None,
        },
        SongFileTarget::Indexed { id, revision } => {
            let candidate = LocalLibrary::new(store)
                .and_then(|library| {
                    let item = library.item(&id)?;
                    if item.revision != revision {
                        return Err(Error::QueueChanged);
                    }
                    let pin = LocalFilePin::new(library, item, None)?;
                    pin.check(session, token)?;
                    Ok(SongFileCandidate::Local(Box::new(pin)))
                })
                .ok();
            SongFileObservation {
                hides_download: true,
                candidate,
            }
        }
        SongFileTarget::Catalog { song, task } if token.uid.is_some() => {
            let finished = finished_song(session, token, &song)?;
            if matches!(finished, FinishedSong::None) {
                let library = LocalLibrary::new(store)?;
                let matches = library.files_for_online_song(session, token, &song)?;
                let hides_download = !matches.is_empty();
                // M171 only orders localMap by song ID. No ordering among
                // distinct files for the same song is stable across hosts.
                let candidate = if let [item] = matches.as_slice() {
                    LocalFilePin::new(library, item.clone(), Some(song))
                        .and_then(|pin| {
                            pin.check(session, token)?;
                            Ok(SongFileCandidate::Local(Box::new(pin)))
                        })
                        .ok()
                } else {
                    None
                };
                SongFileObservation {
                    hides_download,
                    candidate,
                }
            } else {
                // A finished but unavailable/moving publication still owns
                // this branch; never substitute another local file for it.
                let candidate = match finished {
                    FinishedSong::One {
                        id,
                        path: Some(path),
                        moving: false,
                    } if task.as_ref().is_none_or(|task| task == &id) => {
                        CompletedDownloads::new(session.clone())
                            .and_then(|files| {
                                files.catalog_song_reveal_candidate(&id, &song, &path)
                            })
                            .ok()
                            .map(|file| {
                                SongFileCandidate::Download(Box::new(Publication {
                                    song,
                                    task: id,
                                    file,
                                }))
                            })
                    }
                    _ => None,
                };
                SongFileObservation {
                    hides_download: true,
                    candidate,
                }
            }
        }
        SongFileTarget::Catalog { .. } => SongFileObservation {
            hides_download: false,
            candidate: None,
        },
    };
    session.check_generation(token)?;
    Ok(observation)
}

impl MusicClient {
    pub(super) async fn observe_history_song_file(
        &self,
        token: &SessionToken,
        target: SongFileTarget,
    ) -> Result<SongFileObservation> {
        let session = self.session.clone();
        let token = token.clone();
        tokio::task::spawn_blocking(move || discover(&session, &token, target))
            .await
            .map_err(|_| Error::State("history song file discovery stopped"))?
    }

    pub async fn history_row_song_file_reveal(
        &self,
        epoch: &str,
        target: HistoryRowMenuTarget,
        receipt: &str,
    ) -> Result<HistorySongFileReveal> {
        if target.history_kind != HistoryKind::Song {
            return Err(Error::InvalidInput("history menu requires a song"));
        }
        let token = self.player_token(epoch)?;
        let history = HistoryService::new(self.session.clone())?;
        let snapshot = history.snapshot_for(&token, HistoryKind::Song)?;
        if snapshot.revision != target.revision {
            return Err(Error::QueueChanged);
        }
        let row = row_menu::resolve(&snapshot, &target.clicked_id, &target.selected_ids)?;
        if matches!(
            SongFileTarget::from_record(row.clicked),
            SongFileTarget::Unavailable
        ) {
            return Err(Error::QueueChanged);
        }
        let Candidate::Song(candidate) =
            self.history_row_file_menu.take(&token, &target, receipt)?
        else {
            return Err(Error::QueueChanged);
        };
        let session = self.session.clone();
        let owner = token.clone();
        let file = tokio::task::spawn_blocking(move || match candidate {
            SongFileCandidate::Download(file) => file
                .verify(&session, &owner)
                .map(Box::new)
                .map(SongFile::Download),
            SongFileCandidate::Local(file) => {
                file.check(&session, &owner).map_err(local_error)?;
                Ok(SongFile::Local(file))
            }
        })
        .await
        .map_err(|_| Error::State("history song file validation stopped"))??;
        let reveal = HistorySongFileReveal {
            token,
            revision: target.revision,
            file,
        };
        reveal.check_current(self)?;
        Ok(reveal)
    }
}

fn download_error(error: Error) -> Error {
    match error {
        Error::Media(_) | Error::Io(_) | Error::State(_) | Error::MutationBusy => {
            Error::DownloadedFileMissing
        }
        other => other,
    }
}

fn local_error(error: Error) -> Error {
    match error {
        Error::Media(_) | Error::Io(_) | Error::State(_) | Error::MutationBusy => {
            Error::QueueChanged
        }
        other => other,
    }
}

/// Native handoff. Neither the path nor a reusable reveal capability enters IPC.
pub struct HistorySongFileReveal {
    token: SessionToken,
    revision: String,
    file: SongFile,
}

impl HistorySongFileReveal {
    pub fn path(&self) -> &Path {
        match &self.file {
            SongFile::Download(file) => file.file.path(),
            SongFile::Local(file) => &file.item.path,
        }
    }

    pub fn check_current(&self, client: &MusicClient) -> Result<()> {
        HistoryService::new(client.session.clone())?.with_matching_revision(
            &self.token,
            HistoryKind::Song,
            &self.revision,
            |_| Ok(()),
        )?;
        match &self.file {
            SongFile::Download(file) => {
                file.file.check_current().map_err(download_error)?;
                file.check_selected_task(&client.session, &self.token)
            }
            SongFile::Local(file) => file
                .check(&client.session, &self.token)
                .map_err(local_error),
        }
    }
}
