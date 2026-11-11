mod liked;
mod operation;
mod reconcile;
use super::*;
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    protocol::api::{Api, GuardedPlaylistAdd, PlaylistAddPermission, identifier},
    storage::{OperationLock, StateStore, StateTransaction},
};
pub(crate) use liked::Origin as GroupDownloadLikedOrigin;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) const JOURNAL: &str = "playlist-mutations";

pub(crate) struct GroupDownloadPlaylistProof {
    pub operation: String,
    pub playlist: String,
    pub songs: Arc<[String]>,
    pub creation: Option<String>,
    pub liked_origin: Option<GroupDownloadLikedOrigin>,
}

impl GroupDownloadPlaylistProof {
    pub(crate) fn outcome(&self, record: &PlaylistMutation, owner: &str) -> Result<(bool, bool)> {
        if record.operation_id != self.operation
            || record.local_batch_favorite
            || record.system_liked_group != self.liked_origin.is_some()
            || record.playlist_id.as_deref() != Some(self.playlist.as_str())
            || !matches!(&record.command, PlaylistCommand::Tracks { id, change: TrackChange::Add, ids, .. }
                if id == &self.playlist && ids.as_slice() == self.songs.as_ref())
        {
            return Err(Error::StaleResponse);
        }
        let outcome = record.download_picker_outcome(self.creation.is_none());
        if outcome.0
            && let Some(origin) = self.liked_origin
        {
            liked::verify_download_readback(record, owner, &self.playlist, &self.songs, origin)?;
        }
        Ok(outcome)
    }

    pub(crate) fn verify_in(&self, tx: &StateTransaction<'_>, owner: &str) -> Result<()> {
        let read = |id: &str| {
            PlaylistService::parse_record(
                id,
                tx.get(owner, JOURNAL, id)?.ok_or(Error::StaleResponse)?,
            )
        };
        if !self.outcome(&read(&self.operation)?, owner)?.0 {
            return Err(Error::StaleResponse);
        }
        if let Some(id) = &self.creation
            && read(id)?.confirmed_created_target().map(|(id, _, _)| id) != Some(&self.playlist)
        {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
}

enum GuardedReadback<'a> {
    SingleAdd,
    SelectionAdd {
        playlist: &'a str,
        songs: &'a [String],
        system_liked: bool,
    },
    LocalAdd {
        playlist: &'a str,
        songs: &'a [String],
    },
    Create,
}

enum AddDestination<'a> {
    Owned(&'a str),
    DownloadPicker {
        playlist: &'a str,
        regular_only: bool,
    },
    LocalPicker(&'a str),
    DownloadCreated(&'a str),
    Created(&'a str),
    SystemLiked(&'a str),
    DownloadSystemLiked(&'a str),
    History(HistoryCollectDestination<'a>),
}

pub(crate) enum HistoryCollectDestination<'a> {
    Existing(&'a str),
    ExistingRegular(&'a str),
    Created(&'a str),
    SystemLiked(&'a str),
}

struct WriteGuard<'a> {
    permission: &'a PlaylistAddPermission<'a>,
    credentials: &'a PlaylistAddPermission<'a>,
    system_liked_group: bool,
    local_picker: bool,
    history_picker: bool,
}
pub struct PlaylistService {
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
}

struct Unsent<'a> {
    service: &'a PlaylistService,
    uid: &'a str,
    record: &'a PlaylistMutation,
    dispatched: AtomicBool,
    settled: bool,
}

impl Unsent<'_> {
    fn reject(&mut self) -> Result<()> {
        self.settled = true;
        let mut record = self.record.clone();
        record.phase = PlaylistPhase::Rejected;
        record.error_class = Some("not_sent".into());
        self.service.persist(self.uid, &record)
    }
}

impl Drop for Unsent<'_> {
    fn drop(&mut self) {
        if !self.settled && !self.dispatched.load(Ordering::Relaxed) {
            let _ = self.reject();
        }
    }
}
fn now() -> Result<f64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .map_err(|_| Error::State("system clock precedes UNIX epoch"))
}
impl PlaylistService {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Ok(Self {
            store: session.persistent_store()?,
            session,
        })
    }
    fn owner(&self, token: &SessionToken) -> Result<String> {
        self.session.with_token(token, |uid| Ok(uid.into()))
    }
    fn records(&self, uid: &str) -> Result<Vec<PlaylistMutation>> {
        self.store
            .values(uid, JOURNAL)?
            .into_iter()
            .map(|(key, v)| Self::parse_record(&key, v))
            .collect()
    }

    fn parse_record(key: &str, value: Value) -> Result<PlaylistMutation> {
        let mut record: PlaylistMutation = serde_json::from_value(value)?;
        if record.operation_id != key
            || key.len() != 32
            || !key.bytes().all(|b| b.is_ascii_hexdigit())
            || !record.created_at.is_finite()
        {
            return Err(Error::Response("invalid playlist operation record"));
        }
        record.command.validate()?;
        if record.local_batch_favorite
            && (record.system_liked_group
                || !matches!(
                    &record.command,
                    PlaylistCommand::Tracks {
                        change: TrackChange::Add,
                        ..
                    }
                ))
        {
            return Err(Error::Response("invalid local batch favorite record"));
        }
        Ok(record)
    }
    fn persist(&self, uid: &str, record: &PlaylistMutation) -> Result<()> {
        self.store.put(
            uid,
            JOURNAL,
            &record.operation_id,
            &serde_json::to_value(record)?,
        )
    }
    pub fn operations(&self, token: &SessionToken) -> Result<Vec<PlaylistMutation>> {
        self.session.with_token(token, |uid| self.records(uid))
    }

    /// Caller already holds the common playlist writer lease.
    pub(super) fn require_no_pending_playlist(&self, uid: &str, id: &str) -> Result<()> {
        if self.records(uid)?.iter().any(|record| {
            record.phase.pending()
                && (record.playlist_id.as_deref() == Some(id)
                    || record.command.playlist_id() == Some(id))
        }) {
            return Err(Error::MutationUncertain);
        }
        Ok(())
    }
    pub async fn list(&self, token: &SessionToken) -> Result<PlaylistList> {
        let value = self.read_list(token).await?;
        self.session.save_result(
            token,
            "playlist-cache",
            "list",
            &serde_json::to_value(&value)?,
        )?;
        Ok(value)
    }

    async fn read_list(&self, token: &SessionToken) -> Result<PlaylistList> {
        self.session
            .run_for(token, |api, uid| async move {
                PlaylistList::read(&PlaylistApi::new(api), &uid).await
            })
            .await
    }
    pub async fn detail(&self, token: &SessionToken, id: &str) -> Result<PlaylistSnapshot> {
        let value = self.read_detail(token, id).await?;
        self.session
            .save_result(token, "playlist-cache", id, &serde_json::to_value(&value)?)?;
        Ok(value)
    }

    async fn read_detail(&self, token: &SessionToken, id: &str) -> Result<PlaylistSnapshot> {
        identifier(&json!(id))?;
        self.session
            .run_for(token, |api, _| async move {
                PlaylistSnapshot::read(&PlaylistApi::new(api), id).await
            })
            .await
    }
    pub fn cached_detail(
        &self,
        token: &SessionToken,
        id: &str,
    ) -> Result<Option<PlaylistSnapshot>> {
        crate::protocol::api::identifier(&serde_json::json!(id))?;
        self.session.with_token(token, |uid| {
            let Some(value) = self.store.get(uid, "playlist-cache", id)? else {
                return Ok(None);
            };
            let snapshot: PlaylistSnapshot = serde_json::from_value(value)?;
            if snapshot.summary.id != id {
                return Err(Error::State("cached playlist identity differs"));
            }
            Ok(Some(snapshot))
        })
    }
    /// At most one local writer per account. Only unresolved overlapping
    /// resources block subsequent commands; unrelated metadata stays editable.
    pub async fn execute(
        &self,
        token: &SessionToken,
        command: PlaylistCommand,
    ) -> Result<PlaylistMutation> {
        self.execute_identified(token, command, &uuid::Uuid::new_v4().simple().to_string())
            .await
    }
    /// A durable parent records this ID before invoking its child. An existing
    /// child is only read back, even after a crash between sending and reply.
    pub(super) async fn execute_identified(
        &self,
        token: &SessionToken,
        mut command: PlaylistCommand,
        operation_id: &str,
    ) -> Result<PlaylistMutation> {
        let uid = self.owner(token)?;
        command.validate()?;
        if operation_id.len() != 32 || !operation_id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::InvalidInput("invalid playlist operation identifier"));
        }
        if matches!(
            &command,
            PlaylistCommand::Tracks {
                change: TrackChange::LegacyReplace,
                ..
            }
        ) {
            return Err(Error::InvalidInput(
                "update is reorder, not replacement; use clear to remove all tracks",
            ));
        }
        if matches!(&command,PlaylistCommand::Tracks{change:TrackChange::Clear,ids,..} if !ids.is_empty())
        {
            return Err(Error::InvalidInput("clear takes no caller-supplied ids"));
        }
        let _lock = OperationLock::acquire(self.store.directory(), &uid, JOURNAL)?;
        if let PlaylistCommand::Tracks { id, .. } | PlaylistCommand::Delete { id, .. } = &command {
            super::local_membership::reorder::require_settled(&self.store, &uid, id)?;
        }
        let records = self.records(&uid)?;
        if let Some(record) = records.iter().find(|r| r.operation_id == operation_id) {
            if serde_json::to_value(&record.command)? != serde_json::to_value(&command)? {
                return Err(Error::State(
                    "playlist operation ID belongs to another command",
                ));
            }
            return if record.phase.pending() {
                self.check(token, record.clone()).await
            } else {
                Ok(record.clone())
            };
        }
        if records.iter().any(|r| {
            r.phase.pending()
                && (r.command.conflicts(&command)
                    || r.playlist_id
                        .as_deref()
                        .zip(command.playlist_id())
                        .is_some_and(|(a, b)| a == b))
        }) {
            return Err(Error::MutationUncertain);
        }
        let before = self.prepare(token, &command).await?;
        self.execute_prepared(token, &uid, operation_id, command, before, None)
            .await
    }

    pub(crate) fn operation_for(
        &self,
        token: &SessionToken,
        operation_id: &str,
    ) -> Result<Option<PlaylistMutation>> {
        self.session.with_token(token, |uid| {
            self.store
                .get(uid, JOURNAL, operation_id)?
                .map(|value| Self::parse_record(operation_id, value))
                .transpose()
        })
    }

    /// Scope only the native existing-destination action; callers cannot pass
    /// an arbitrary playlist command or a caller-supplied revision snapshot.
    pub(crate) async fn guarded_add_song(
        &self,
        token: &SessionToken,
        playlist: &str,
        song: &str,
        operation_id: &str,
        permission: &PlaylistAddPermission<'_>,
    ) -> Result<PlaylistMutation> {
        self.guarded_add_songs(
            token,
            AddDestination::Owned(playlist),
            &[song.into()],
            operation_id,
            permission,
            |api| api.song_card_playlist_scope_revision(),
        )
        .await
    }

    /// Current-song download: only the verified creation journal supplies a
    /// writable destination, under the same lock as its detail and add.
    pub(crate) async fn guarded_add_created_song(
        &self,
        token: &SessionToken,
        creation: &str,
        song: &str,
        operation_id: &str,
        permission: &PlaylistAddPermission<'_>,
    ) -> Result<PlaylistMutation> {
        self.guarded_add_songs(
            token,
            AddDestination::Created(creation),
            &[song.into()],
            operation_id,
            permission,
            |api| api.song_card_playlist_scope_revision(),
        )
        .await
    }

    pub(crate) async fn guarded_collect_songs(
        &self,
        token: &SessionToken,
        playlist: &str,
        songs: &[String],
        operation_id: &str,
        permission: &PlaylistAddPermission<'_>,
    ) -> Result<PlaylistMutation> {
        self.guarded_add_songs(
            token,
            AddDestination::Owned(playlist),
            songs,
            operation_id,
            permission,
            |api| api.song_card_collection_scope_revision(),
        )
        .await
    }

    pub(crate) async fn guarded_collect_local(
        &self,
        token: &SessionToken,
        playlist: &str,
        songs: &[String],
        operation_id: &str,
        permission: &PlaylistAddPermission<'_>,
    ) -> Result<PlaylistMutation> {
        self.guarded_add_songs(
            token,
            AddDestination::LocalPicker(playlist),
            songs,
            operation_id,
            permission,
            |api| api.local_collection_scope_revision(),
        )
        .await
    }

    pub(crate) async fn guarded_collect_download(
        &self,
        token: &SessionToken,
        playlist: &str,
        songs: &[String],
        regular_only: bool,
        operation_id: &str,
        permission: &PlaylistAddPermission<'_>,
    ) -> Result<PlaylistMutation> {
        self.guarded_add_songs(
            token,
            AddDestination::DownloadPicker {
                playlist,
                regular_only,
            },
            songs,
            operation_id,
            permission,
            |api| api.download_collection_scope_revision(),
        )
        .await
    }

    pub(crate) async fn guarded_collect_created_download(
        &self,
        token: &SessionToken,
        creation_operation: &str,
        songs: &[String],
        operation_id: &str,
        permission: &PlaylistAddPermission<'_>,
    ) -> Result<PlaylistMutation> {
        self.guarded_add_songs(
            token,
            AddDestination::DownloadCreated(creation_operation),
            songs,
            operation_id,
            permission,
            |api| api.download_collection_scope_revision(),
        )
        .await
    }

    /// Resolve the verified create journal under the same lock as the fresh
    /// detail and ordered add. A list-inferred ID is not write authority.
    pub(crate) async fn guarded_collect_created_songs(
        &self,
        token: &SessionToken,
        creation_operation: &str,
        songs: &[String],
        operation_id: &str,
        permission: &PlaylistAddPermission<'_>,
    ) -> Result<PlaylistMutation> {
        self.guarded_add_songs(
            token,
            AddDestination::Created(creation_operation),
            songs,
            operation_id,
            permission,
            |api| api.song_card_collection_scope_revision(),
        )
        .await
    }

    pub(crate) async fn guarded_collect_system_liked(
        &self,
        token: &SessionToken,
        playlist: &str,
        songs: &[String],
        operation_id: &str,
        permission: &PlaylistAddPermission<'_>,
    ) -> Result<PlaylistMutation> {
        self.guarded_add_songs(
            token,
            AddDestination::SystemLiked(playlist),
            songs,
            operation_id,
            permission,
            |api| api.song_card_system_liked_scope_revision(),
        )
        .await
    }

    pub(crate) async fn guarded_collect_history(
        &self,
        token: &SessionToken,
        destination: HistoryCollectDestination<'_>,
        songs: &[String],
        operation_id: &str,
        permission: &PlaylistAddPermission<'_>,
    ) -> Result<PlaylistMutation> {
        let scope: fn(&dyn Api) -> Result<Option<String>> = match destination {
            HistoryCollectDestination::SystemLiked(_) => {
                |api| api.download_system_liked_scope_revision()
            }
            _ => |api| api.download_collection_scope_revision(),
        };
        self.guarded_add_songs(
            token,
            AddDestination::History(destination),
            songs,
            operation_id,
            permission,
            scope,
        )
        .await
    }

    pub(crate) async fn guarded_collect_download_liked_group(
        &self,
        token: &SessionToken,
        playlist: &str,
        songs: &[String],
        operation_id: &str,
        permission: &PlaylistAddPermission<'_>,
    ) -> Result<PlaylistMutation> {
        self.guarded_add_songs(
            token,
            AddDestination::DownloadSystemLiked(playlist),
            songs,
            operation_id,
            permission,
            |api| api.download_system_liked_scope_revision(),
        )
        .await
    }

    async fn guarded_add_songs(
        &self,
        token: &SessionToken,
        destination: AddDestination<'_>,
        songs: &[String],
        operation_id: &str,
        permission: &PlaylistAddPermission<'_>,
        scope: fn(&dyn Api) -> Result<Option<String>>,
    ) -> Result<PlaylistMutation> {
        let uid = self.owner(token)?;
        let (destination, history_picker) = match destination {
            AddDestination::History(destination) => (
                match destination {
                    HistoryCollectDestination::Existing(playlist) => {
                        AddDestination::DownloadPicker {
                            playlist,
                            regular_only: false,
                        }
                    }
                    HistoryCollectDestination::ExistingRegular(playlist) => {
                        AddDestination::DownloadPicker {
                            playlist,
                            regular_only: true,
                        }
                    }
                    HistoryCollectDestination::Created(operation) => {
                        AddDestination::DownloadCreated(operation)
                    }
                    HistoryCollectDestination::SystemLiked(playlist) => {
                        AddDestination::DownloadSystemLiked(playlist)
                    }
                },
                true,
            ),
            other => (other, false),
        };
        let local_picker = matches!(destination, AddDestination::LocalPicker(_));
        let regular_picker = local_picker
            || matches!(
                destination,
                AddDestination::DownloadPicker {
                    regular_only: true,
                    ..
                }
            );
        let system_liked_group = matches!(
            destination,
            AddDestination::SystemLiked(_) | AddDestination::DownloadSystemLiked(_)
        );
        let max_songs = if history_picker {
            GuardedPlaylistAdd::MAX_HISTORY_SONGS
        } else {
            GuardedPlaylistAdd::MAX_SONGS
        };
        if !(1..=max_songs).contains(&songs.len()) {
            return Err(Error::InvalidInput("invalid guarded add selection"));
        }
        if operation_id.len() != 32 || !operation_id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::InvalidInput("invalid playlist operation identifier"));
        }
        let _lock = OperationLock::acquire(self.store.directory(), &uid, JOURNAL)?;
        let records = self.records(&uid)?;
        if records.iter().any(|r| r.operation_id == operation_id) {
            return Err(Error::State(
                "picker operation already exists; read back instead",
            ));
        }
        let api = self.session.native_api(token)?;
        let revision =
            scope(api.as_ref())?.ok_or(Error::State("guarded playlist add is unavailable"))?;
        let credentials = || {
            self.session.require_api(token, &api)?;
            if scope(api.as_ref())?.as_deref() != Some(revision.as_str()) {
                return Err(Error::CredentialChanged);
            }
            Ok(())
        };
        let check = || {
            credentials()?;
            permission()
        };
        check()?;
        let conflict = |playlist: &str| {
            records.iter().any(|r| {
                r.phase.pending()
                    && (r.command.playlist_id() == Some(playlist)
                        || r.playlist_id.as_deref() == Some(playlist)
                        || system_liked_group && r.system_liked_group)
            })
        };
        let (playlist, revision, before) = match destination {
            owned @ (AddDestination::Owned(_)
            | AddDestination::DownloadPicker { .. }
            | AddDestination::LocalPicker(_)
            | AddDestination::DownloadCreated(_)
            | AddDestination::Created(_)) => {
                let (playlist, creation) = match owned {
                    AddDestination::Owned(playlist)
                    | AddDestination::DownloadPicker { playlist, .. }
                    | AddDestination::LocalPicker(playlist) => {
                        (identifier(&json!(playlist))?, None)
                    }
                    AddDestination::Created(operation)
                    | AddDestination::DownloadCreated(operation) => {
                        let record = records
                            .iter()
                            .find(|record| record.operation_id == operation)
                            .ok_or(Error::StaleResponse)?;
                        let (playlist, name, private) = record
                            .confirmed_created_target()
                            .ok_or(Error::StaleResponse)?;
                        (identifier(&json!(playlist))?, Some((name, private)))
                    }
                    AddDestination::SystemLiked(_)
                    | AddDestination::DownloadSystemLiked(_)
                    | AddDestination::History(_) => {
                        unreachable!()
                    }
                };
                if conflict(&playlist) {
                    return Err(Error::MutationUncertain);
                }
                let detail = if let Some((name, private)) = creation {
                    self.read_created_detail(
                        token,
                        &playlist,
                        &uid,
                        name,
                        private,
                        matches!(owned, AddDestination::DownloadCreated(_)),
                    )
                    .await?
                } else if local_picker || matches!(owned, AddDestination::DownloadPicker { .. }) {
                    self.read_picker_detail(token, &playlist, &uid, regular_picker)
                        .await?
                } else {
                    self.read_detail(token, &playlist).await?
                };
                check()?;
                detail.writable(&uid)?;
                if detail.has_local
                    || super::local_membership::has_entries(&self.store, &uid, &playlist)?
                {
                    return Err(Error::State(
                        "local-only playlist entries require native matching before editing",
                    ));
                }
                (
                    playlist,
                    detail.revision.clone(),
                    serde_json::to_value(detail)?,
                )
            }
            AddDestination::SystemLiked(expected)
            | AddDestination::DownloadSystemLiked(expected) => {
                let origin = if matches!(destination, AddDestination::DownloadSystemLiked(_)) {
                    liked::Origin::Download
                } else {
                    liked::Origin::Voice
                };
                let liked = self
                    .observe_system_liked(token, expected, &check, origin)
                    .await?;
                liked.check_selected_membership(songs)?;
                let playlist = liked.playlist.summary.id.clone();
                if conflict(&playlist) {
                    return Err(Error::MutationUncertain);
                }
                (
                    playlist,
                    liked.playlist.revision.clone(),
                    serde_json::to_value(liked)?,
                )
            }
            AddDestination::History(_) => unreachable!("history destination normalized"),
        };
        check()?;
        let mut command = PlaylistCommand::Tracks {
            id: playlist,
            revision,
            change: TrackChange::Add,
            ids: songs.to_vec(),
        };
        command.validate()?;
        self.execute_prepared(
            token,
            &uid,
            operation_id,
            command,
            before,
            Some(WriteGuard {
                permission: &check,
                credentials: &credentials,
                system_liked_group,
                local_picker,
                history_picker,
            }),
        )
        .await
    }

    async fn read_picker_detail(
        &self,
        token: &SessionToken,
        playlist: &str,
        uid: &str,
        regular_only: bool,
    ) -> Result<PlaylistSnapshot> {
        self.session
            .run_for(token, |api, _| async move {
                let row = PlaylistApi::new(api).detail(&json!(playlist)).await?;
                let detail = PlaylistSnapshot::from_detail(&row, playlist)?;
                detail.writable(uid)?;
                // A conflicting explicit creator cannot be treated as an
                // owned row merely because one field repeats the viewer UID.
                for owner in [&row["userId"], &row["creator"]["userId"]] {
                    if !owner.is_null() && identifier(owner)? != uid {
                        return Err(Error::StaleResponse);
                    }
                }
                if (regular_only && row["specialType"].as_i64() != Some(0))
                    || (!row["specialType"].is_null() && row["specialType"].as_i64().is_none())
                {
                    return Err(Error::StaleResponse);
                }
                Ok(detail)
            })
            .await
    }

    async fn read_created_detail(
        &self,
        token: &SessionToken,
        playlist: &str,
        uid: &str,
        name: &str,
        private: bool,
        require_regular: bool,
    ) -> Result<PlaylistSnapshot> {
        self.session
            .run_for(token, |api, _| async move {
                let row = PlaylistApi::new(api).detail(&json!(playlist)).await?;
                let detail = PlaylistSnapshot::from_detail(&row, playlist)?;
                detail.writable(uid)?;
                // The general projection can fall back to creator.userId.
                // Here every asserted owner must agree with the create account.
                for owner in [&row["userId"], &row["creator"]["userId"]] {
                    if !owner.is_null() && identifier(owner)? != uid {
                        return Err(Error::StaleResponse);
                    }
                }
                let special = &row["specialType"];
                if detail.summary.name != name
                    || detail.summary.private != private
                    || !special.is_null()
                        && (special.as_i64().is_none()
                            || require_regular && special.as_i64() != Some(0))
                {
                    return Err(Error::StaleResponse);
                }
                Ok(detail)
            })
            .await
    }

    /// The explicit name/privacy confirmation is the only producer; the
    /// picker holds its separate add intent until the returned ID is verified.
    pub(crate) async fn guarded_create_song_card(
        &self,
        token: &SessionToken,
        operation_id: &str,
        name: &str,
        private: bool,
        permission: &PlaylistAddPermission<'_>,
    ) -> Result<PlaylistMutation> {
        self.guarded_create_playlist(token, operation_id, name, private, permission, |api| {
            api.song_card_playlist_create_scope_revision()
        })
        .await
    }

    pub(crate) async fn guarded_create_collect_playlist(
        &self,
        token: &SessionToken,
        operation_id: &str,
        name: &str,
        private: bool,
        permission: &PlaylistAddPermission<'_>,
    ) -> Result<PlaylistMutation> {
        self.guarded_create_playlist(token, operation_id, name, private, permission, |api| {
            api.song_card_collection_create_scope_revision()
        })
        .await
    }

    pub(crate) async fn guarded_create_download_collection(
        &self,
        token: &SessionToken,
        operation_id: &str,
        name: &str,
        private: bool,
        permission: &PlaylistAddPermission<'_>,
    ) -> Result<PlaylistMutation> {
        self.guarded_create_playlist(token, operation_id, name, private, permission, |api| {
            api.download_collection_create_scope_revision()
        })
        .await
    }

    async fn guarded_create_playlist(
        &self,
        token: &SessionToken,
        operation_id: &str,
        name: &str,
        private: bool,
        permission: &PlaylistAddPermission<'_>,
        scope: fn(&dyn Api) -> Result<Option<String>>,
    ) -> Result<PlaylistMutation> {
        let uid = self.owner(token)?;
        let mut command = PlaylistCommand::Create {
            name: name.into(),
            private,
        };
        command.validate()?;
        if operation_id.len() != 32 || !operation_id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::InvalidInput("invalid playlist operation identifier"));
        }
        let _lock = OperationLock::acquire(self.store.directory(), &uid, JOURNAL)?;
        let records = self.records(&uid)?;
        if records.iter().any(|r| r.operation_id == operation_id) {
            return Err(Error::State(
                "picker operation already exists; read back instead",
            ));
        }
        if records
            .iter()
            .any(|r| r.phase.pending() && r.command.conflicts(&command))
        {
            return Err(Error::MutationUncertain);
        }
        let api = self.session.native_api(token)?;
        let revision =
            scope(api.as_ref())?.ok_or(Error::State("guarded playlist create is unavailable"))?;
        let credentials = || {
            self.session.require_api(token, &api)?;
            if scope(api.as_ref())?.as_deref() != Some(revision.as_str()) {
                return Err(Error::CredentialChanged);
            }
            Ok(())
        };
        let check = || {
            credentials()?;
            permission()
        };
        check()?;
        let before = self.read_list(token).await?;
        check()?;
        self.execute_prepared(
            token,
            &uid,
            operation_id,
            command,
            serde_json::to_value(before)?,
            Some(WriteGuard {
                permission: &check,
                credentials: &credentials,
                system_liked_group: false,
                local_picker: false,
                history_picker: false,
            }),
        )
        .await
    }

    async fn execute_prepared(
        &self,
        token: &SessionToken,
        uid: &str,
        operation_id: &str,
        command: PlaylistCommand,
        before: Value,
        guard: Option<WriteGuard<'_>>,
    ) -> Result<PlaylistMutation> {
        let local_picker = guard.as_ref().is_some_and(|guard| guard.local_picker);
        let history_picker = guard.as_ref().is_some_and(|guard| guard.history_picker);
        let mut record = PlaylistMutation {
            operation_id: operation_id.into(),
            playlist_id: command.playlist_id().map(str::to_owned),
            command,
            phase: PlaylistPhase::Sending,
            before: before.clone(),
            observed: None,
            created_at: now()?,
            checked_at: None,
            codes: Default::default(),
            error_class: None,
            system_liked_group: guard.as_ref().is_some_and(|guard| guard.system_liked_group),
            local_batch_favorite: local_picker,
        };
        // M3160 sends the selected IDs even when already present; its 502
        // duplicate branch is visible failure, not an optimistic success.
        let unchanged = if local_picker || history_picker {
            false
        } else if record.system_liked_group {
            liked::matches(&record, &before, uid)?
        } else {
            reconcile::matches(&mut record, &before, uid)?
        };
        if unchanged {
            record.phase = PlaylistPhase::Unchanged;
            record.observed = Some(before);
        }
        self.session.save_result(
            token,
            JOURNAL,
            &record.operation_id,
            &serde_json::to_value(&record)?,
        )?;
        if record.phase == PlaylistPhase::Unchanged {
            return Ok(record);
        }
        let mut unsent = guard.as_ref().map(|_| Unsent {
            service: self,
            uid,
            record: &record,
            dispatched: AtomicBool::new(false),
            settled: false,
        });
        let dispatch = guard.as_ref().zip(unsent.as_ref());
        let command = record.command.clone();
        let before = record.before.clone();
        let reply = self
            .session
            .run_for(token, |api, uid| async move {
                if let Some((guard, marker)) = dispatch {
                    return operation::send_guarded(
                        api,
                        uid,
                        command,
                        guard.permission,
                        &marker.dispatched,
                        history_picker,
                    )
                    .await;
                }
                operation::send(PlaylistApi::new(api), &uid, command, before).await
            })
            .await;
        if let Some(marker) = &mut unsent
            && !marker.dispatched.load(Ordering::Relaxed)
        {
            marker.reject()?;
            return reply.and(Err(Error::State("guarded playlist add did not dispatch")));
        }
        drop(unsent);
        match reply {
            Ok(ack) => {
                record.phase = PlaylistPhase::Verifying;
                record.codes = ack.codes;
                if ack.created.is_some() {
                    record.playlist_id = ack.created;
                }
            }
            Err(error) => {
                if (local_picker || history_picker) && error.is_service_code(502) {
                    record.phase = PlaylistPhase::Rejected;
                    record.codes = operation::error_codes(&error);
                    record.error_class = Some("service".into());
                    self.persist(uid, &record)?;
                    return Ok(record);
                }
                if guard.is_none() && matches!(error.root(), Error::RequestNotSent(_)) {
                    record.phase = PlaylistPhase::Rejected;
                    record.error_class = Some("not_sent".into());
                    self.persist(uid, &record)?;
                    return Err(error);
                }
                record.phase = PlaylistPhase::Uncertain;
                record.codes = operation::error_codes(&error);
                record.error_class = Some(
                    match error.root() {
                        Error::Transport(_) => "transport",
                        Error::Batch { .. } => "batch",
                        Error::Service { .. } | Error::HttpStatus { .. } => "service",
                        Error::StaleResponse => "stale",
                        _ => "state_or_response",
                    }
                    .into(),
                );
                // Old-account responsibility survives a late reply, but never
                // gets published into the newly selected account's cache.
                self.persist(uid, &record)?;
                if error.is_service_code(301) || self.owner(token).is_err() {
                    return Err(error);
                }
            }
        }
        self.persist(uid, &record)?;
        self.check_with(token, record, guard.map(|guard| guard.credentials))
            .await
    }
    pub async fn reconcile(
        &self,
        token: &SessionToken,
        operation_id: &str,
    ) -> Result<PlaylistMutation> {
        let uid = self.owner(token)?;
        let _lock = OperationLock::acquire(self.store.directory(), &uid, JOURNAL)?;
        let record = self
            .records(&uid)?
            .into_iter()
            .find(|r| r.operation_id == operation_id)
            .ok_or(Error::InvalidInput("unknown playlist operation"))?;
        if !record.phase.pending() {
            return Ok(record);
        }
        if record.local_batch_favorite {
            let api = self.session.native_api(token)?;
            let revision = api
                .local_collection_scope_revision()?
                .ok_or(Error::CredentialChanged)?;
            let check = || {
                self.session.require_api(token, &api)?;
                if api.local_collection_scope_revision()?.as_deref() != Some(revision.as_str()) {
                    return Err(Error::CredentialChanged);
                }
                Ok(())
            };
            self.check_with(token, record, Some(&check)).await
        } else {
            self.check(token, record).await
        }
    }

    pub(crate) async fn reconcile_guarded_add(
        &self,
        token: &SessionToken,
        operation_id: &str,
        check: &PlaylistAddPermission<'_>,
    ) -> Result<PlaylistMutation> {
        self.reconcile_guarded_write(token, operation_id, check, GuardedReadback::SingleAdd)
            .await
    }

    pub(crate) async fn reconcile_guarded_create(
        &self,
        token: &SessionToken,
        operation_id: &str,
        check: &PlaylistAddPermission<'_>,
    ) -> Result<PlaylistMutation> {
        self.reconcile_guarded_write(token, operation_id, check, GuardedReadback::Create)
            .await
    }

    pub(crate) async fn reconcile_guarded_collect(
        &self,
        token: &SessionToken,
        operation_id: &str,
        playlist: &str,
        songs: &[String],
        system_liked: bool,
        check: &PlaylistAddPermission<'_>,
    ) -> Result<PlaylistMutation> {
        self.reconcile_guarded_write(
            token,
            operation_id,
            check,
            GuardedReadback::SelectionAdd {
                playlist,
                songs,
                system_liked,
            },
        )
        .await
    }

    pub(crate) async fn reconcile_local_collect(
        &self,
        token: &SessionToken,
        operation_id: &str,
        playlist: &str,
        songs: &[String],
        check: &PlaylistAddPermission<'_>,
    ) -> Result<PlaylistMutation> {
        self.reconcile_guarded_write(
            token,
            operation_id,
            check,
            GuardedReadback::LocalAdd { playlist, songs },
        )
        .await
    }

    async fn reconcile_guarded_write(
        &self,
        token: &SessionToken,
        operation_id: &str,
        check: &PlaylistAddPermission<'_>,
        expected: GuardedReadback<'_>,
    ) -> Result<PlaylistMutation> {
        let uid = self.owner(token)?;
        let _lock = OperationLock::acquire(self.store.directory(), &uid, JOURNAL)?;
        let record = self
            .operation_for(token, operation_id)?
            .ok_or(Error::State("picker playlist operation is missing"))?;
        let valid = match (&record.command, expected) {
            (PlaylistCommand::Create { .. }, GuardedReadback::Create) => true,
            (
                PlaylistCommand::Tracks {
                    change: TrackChange::Add,
                    ids,
                    ..
                },
                GuardedReadback::SingleAdd,
            ) => ids.len() == 1 && !record.system_liked_group && !record.local_batch_favorite,
            (
                PlaylistCommand::Tracks {
                    id,
                    change: TrackChange::Add,
                    ids,
                    ..
                },
                GuardedReadback::SelectionAdd {
                    playlist,
                    songs,
                    system_liked,
                },
            ) => {
                !record.local_batch_favorite
                    && record.system_liked_group == system_liked
                    && id == playlist
                    && ids == songs
            }
            (
                PlaylistCommand::Tracks {
                    id,
                    change: TrackChange::Add,
                    ids,
                    ..
                },
                GuardedReadback::LocalAdd { playlist, songs },
            ) => {
                record.local_batch_favorite
                    && !record.system_liked_group
                    && id == playlist
                    && ids == songs
            }
            _ => false,
        };
        if !valid {
            return Err(Error::State("picker playlist operation is invalid"));
        }
        check()?;
        if record.phase.pending() {
            self.check_with(token, record, Some(check)).await
        } else {
            Ok(record)
        }
    }
    async fn check(
        &self,
        token: &SessionToken,
        record: PlaylistMutation,
    ) -> Result<PlaylistMutation> {
        self.check_with(token, record, None).await
    }

    async fn check_with(
        &self,
        token: &SessionToken,
        mut record: PlaylistMutation,
        guard: Option<&PlaylistAddPermission<'_>>,
    ) -> Result<PlaylistMutation> {
        if record.system_liked_group {
            return self.check_system_liked(token, record, guard).await;
        }
        let uid = self.owner(token)?;
        let observed = match record.command {
            PlaylistCommand::Create { .. }
            | PlaylistCommand::Delete { .. }
            | PlaylistCommand::Subscribe { .. }
            | PlaylistCommand::Order { .. } => {
                if let Some(guard) = guard {
                    guard()?;
                    let list = serde_json::to_value(self.read_list(token).await?)?;
                    guard()?;
                    self.session
                        .save_result(token, "playlist-cache", "list", &list)?;
                    list
                } else {
                    serde_json::to_value(self.list(token).await?)?
                }
            }
            _ => {
                let id = record.command.playlist_id().expect("detail command");
                if let Some(guard) = guard {
                    guard()?;
                    let snapshot = if record.local_batch_favorite {
                        self.read_picker_detail(token, id, &uid, true).await?
                    } else {
                        self.read_detail(token, id).await?
                    };
                    let detail = serde_json::to_value(snapshot)?;
                    guard()?;
                    self.session
                        .save_result(token, "playlist-cache", id, &detail)?;
                    detail
                } else {
                    serde_json::to_value(self.detail(token, id).await?)?
                }
            }
        };
        record.phase = if reconcile::matches(&mut record, &observed, &uid)? {
            PlaylistPhase::Confirmed
        } else {
            // The first live audit proved acknowledged update([]) is a sort
            // no-op. Preserve its failure instead of indefinitely blocking
            // cleanup, or retroactively claiming the requested clear worked.
            if matches!(&record.command,PlaylistCommand::Tracks{change:TrackChange::LegacyReplace,ids,..} if ids.is_empty())
                && record.codes.get("main") == Some(&200)
                && observed["tracks"] == record.before["tracks"]
            {
                record.error_class = Some("unsupported_replace_contract".into());
                PlaylistPhase::Rejected
            } else {
                operation::unmatched_phase(&record)
            }
        };
        record.observed = Some(observed);
        record.checked_at = Some(now()?);
        let persisted = serde_json::to_value(&record)?;
        if record.phase == PlaylistPhase::Confirmed
            && let PlaylistCommand::Delete { id, .. } = &record.command
        {
            self.session.with_token(token, |owner| {
                self.store.transaction(|tx| {
                    tx.delete(owner, OFFLINE_SNAPSHOT_KIND, id)?;
                    tx.put(owner, OFFLINE_DELETED_KIND, id, &Value::Bool(true))?;
                    tx.put(owner, JOURNAL, &record.operation_id, &persisted)
                })
            })?;
        } else {
            self.session
                .save_result(token, JOURNAL, &record.operation_id, &persisted)?;
        }
        // Return the actual readback even for a partial/rejected/uncertain
        // result, so hosts cannot accidentally paint an optimistic success.
        Ok(record)
    }
    async fn prepare(&self, token: &SessionToken, command: &PlaylistCommand) -> Result<Value> {
        let uid = self.owner(token)?;
        match command {
            PlaylistCommand::Create { .. }
            | PlaylistCommand::Order { .. }
            | PlaylistCommand::Subscribe { .. } => {
                let list = self.list(token).await?;
                if let PlaylistCommand::Order {
                    group,
                    revision,
                    ids,
                } = command
                {
                    if revision != &list.revision {
                        return Err(Error::QueueChanged);
                    }
                    operation::permutation(&list.group(&uid, *group), ids)?;
                    for p in list
                        .playlists
                        .iter()
                        .filter(|p| p.favorite && p.owner == uid)
                    {
                        let original = list.group(&uid, *group);
                        if original.iter().position(|id| id == &p.id)
                            != ids.iter().position(|id| id == &p.id)
                        {
                            return Err(Error::InvalidInput("favorite playlist position is fixed"));
                        }
                    }
                }
                if let PlaylistCommand::Subscribe { id, subscribed } = command {
                    let owner = if let Some(p) = list.playlists.iter().find(|p| p.id == *id) {
                        Some(p.owner.clone())
                    } else if *subscribed {
                        Some(self.detail(token, id).await?.summary.owner)
                    } else {
                        None
                    };
                    if owner.as_deref() == Some(&uid) {
                        return Err(Error::InvalidInput("cannot subscribe to own playlist"));
                    }
                }
                Ok(serde_json::to_value(list)?)
            }
            _ => {
                let detail = self
                    .detail(token, command.playlist_id().expect("detail command"))
                    .await?;
                detail.writable(&uid)?;
                if command.revision() != Some(&detail.revision) {
                    return Err(Error::QueueChanged);
                }
                if let PlaylistCommand::Tracks { change, ids, .. } = command {
                    if detail.has_local
                        || super::local_membership::has_entries(
                            &self.store,
                            &uid,
                            &detail.summary.id,
                        )?
                    {
                        return Err(Error::State(
                            "local-only playlist entries require native matching before editing",
                        ));
                    }
                    if *change == TrackChange::Reorder {
                        operation::permutation(&detail.tracks, ids)?;
                    }
                }
                Ok(serde_json::to_value(detail)?)
            }
        }
    }
}
