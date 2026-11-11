//! Scene.newMiniBar Song sharing. Neither an existing menu link nor a row
//! receipt from a different feature can authorize this player operation.
use super::{MusicClient, PlayerResourceKind, identity, vinyl_similar::stale_credentials};
use crate::{
    Error, Result,
    account::SessionToken,
    catalog::song_share_allowed,
    client::{
        dto::{
            AccountData, HistoryRowShareChoice, HistoryRowShareOutcome, HistoryRowSharePreparation,
        },
        focus::{FocusSnapshot, Owner},
        share::qr_modules,
    },
    playback::{
        TimelineMode,
        controller::{PlaybackController, PlaybackSnapshot, PlaybackStatus},
        engine::Cancellation,
    },
    protocol::{
        api::{Api, identifier},
        wire::serialization::js_truthy,
    },
};
use serde::Deserialize;
use serde_json::Value;
use std::{
    sync::{Arc, Mutex, Weak, atomic::Ordering},
    time::{Duration, Instant},
};

const METADATA: &str = "_current_catalog_share_album_required";
const LIFETIME: Duration = Duration::from_secs(180);
const READ_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerSongShareTarget {
    play_id: String,
    entry_id: String,
}

fn canonical_id(value: &Value) -> Option<String> {
    let id = identifier(value).ok()?;
    (!id.starts_with('0') && id.len() <= 128).then_some(id)
}

fn private_song(song: &Value) -> bool {
    [
        "fileId",
        "path",
        "localId",
        "local_id",
        "cloudSongId",
        "cloud_song_id",
        "programId",
        "voiceId",
        "voice_id",
        "download_task_id",
    ]
    .into_iter()
    .any(|key| js_truthy(&song[key]))
        || ["t", "songType"]
            .into_iter()
            .any(|key| song[key].as_i64() == Some(1) || song[key].as_str() == Some("1"))
}

fn private_privilege(privilege: &Value) -> bool {
    js_truthy(&privilege["cs"]) || js_truthy(&privilege["cloudSong"])
}

fn status(privilege: &Value, key: &str) -> Option<i64> {
    privilege[key]
        .as_i64()
        .or_else(|| privilege[key].as_str()?.parse().ok())
}

/// Called only while compacting native V3 detail and its matching privilege.
/// The stored boolean is a presentation hint, never a permission or a cookie.
pub(in crate::client) fn mark_metadata(
    output: &mut Value,
    song: &Value,
    privilege: Option<&Value>,
) {
    let Some(privilege) = privilege else { return };
    let Some(id) = canonical_id(&song["id"]) else {
        return;
    };
    let embedded = &song["privilege"];
    let status_known = status(privilege, "st").or_else(|| status(privilege, "status"));
    if canonical_id(&privilege["id"]).as_deref() != Some(id.as_str())
        || private_song(song)
        || private_privilege(privilege)
        || private_privilege(embedded)
        || status_known.is_none_or(|value| value < 0)
        || [privilege, embedded].into_iter().any(|row| {
            ["st", "status"]
                .into_iter()
                .any(|key| status(row, key).is_some_and(|value| value < 0))
        })
        || (js_truthy(&song["onlineTrackId"])
            && canonical_id(&song["onlineTrackId"]).as_deref() != Some(id.as_str()))
        || (!song["noCopyrightRcmd"].is_null()
            && (song["noCopyrightRcmd"]["type"].as_i64() != Some(0)
                || !song["noCopyrightRcmd"]["mvInfo"].is_null()))
    {
        return;
    }
    output[METADATA] = Value::Bool(privilege["fee"].as_u64().is_some_and(|fee| fee & 4 != 0));
}

#[derive(Clone, PartialEq, Eq)]
struct Song {
    id: String,
    album_required: bool,
}

fn current_song(snapshot: &PlaybackSnapshot) -> Option<Song> {
    let entry = snapshot.selected.as_ref()?;
    let resource = &entry.resource;
    let song = &resource["song"];
    if snapshot.play_id.is_none()
        || !matches!(
            snapshot.state,
            PlaybackStatus::Playing | PlaybackStatus::Paused | PlaybackStatus::Buffering
        )
        || snapshot.fm
        || snapshot.intelligence.is_some()
        || snapshot.catalog_file_source
        || snapshot.timeline.as_ref()?.mode != TimelineMode::Full
        || !entry.visible_in_queue()
        || entry.source["kind"] == "listen_together"
        || !resource["shared_room"].is_null()
        || !resource["playback_constraint"].is_null()
        || identity::kind(resource) != PlayerResourceKind::Catalog
        || resource["missing"] == true
        || private_song(resource)
        || private_song(song)
    {
        return None;
    }
    let id = canonical_id(&resource["id"])?;
    if canonical_id(&song["id"]).as_deref() != Some(id.as_str()) {
        return None;
    }
    Some(Song {
        id,
        album_required: song[METADATA].as_bool()?,
    })
}

pub(super) fn available(snapshot: &PlaybackSnapshot) -> bool {
    current_song(snapshot).is_some()
}

#[derive(Clone, PartialEq, Eq)]
struct Playback {
    song: Song,
    focus: FocusSnapshot,
    selection: u64,
}

fn target_song(
    view: &PlaybackSnapshot,
    owner: &SessionToken,
    target: &PlayerSongShareTarget,
) -> Result<Option<Song>> {
    if &view.account != owner
        || view.play_id.as_deref() != Some(target.play_id.as_str())
        || view.selected.as_ref().map(|entry| entry.entry_id.as_str())
            != Some(target.entry_id.as_str())
    {
        return Err(Error::StaleResponse);
    }
    Ok(current_song(view))
}

impl Playback {
    fn capture(
        client: &MusicClient,
        controller: &PlaybackController,
        owner: &SessionToken,
        target: &PlayerSongShareTarget,
    ) -> Result<Option<Self>> {
        let selection = client.player.selection.load(Ordering::Acquire);
        let Some(song) = target_song(&controller.snapshot(), owner, target)? else {
            return Ok(None);
        };
        let focus = client.focus.snapshot()?;
        if focus.owner != Some(Owner::Audio)
            || client.together_attached(owner, None)?
            || client.player.selection.load(Ordering::Acquire) != selection
        {
            return Err(Error::StaleResponse);
        }
        Ok(Some(Self {
            song,
            focus,
            selection,
        }))
    }

    fn check_selected(&self, client: &MusicClient, auth: &Authority<'_>) -> Result<()> {
        let revision = &client.player.selection;
        if revision.load(Ordering::Acquire) != self.selection
            || target_song(&auth.controller.snapshot(), auth.owner, auth.target)?.as_ref()
                != Some(&self.song)
            || revision.load(Ordering::Acquire) != self.selection
        {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
}

struct Authority<'a> {
    owner: &'a SessionToken,
    controller: &'a PlaybackController,
    target: &'a PlayerSongShareTarget,
    api: Arc<dyn Api>,
    revision: String,
    playback: Playback,
}

struct Offer {
    owner: SessionToken,
    target: PlayerSongShareTarget,
    api: Weak<dyn Api>,
    revision: String,
    playback: Playback,
    original_host: Cancellation,
    receipt: String,
    issued: Instant,
    choosing: bool,
}

#[derive(Default)]
struct State {
    generation: u64,
    offer: Option<Offer>,
}
impl State {
    fn advance(&mut self) -> Result<u64> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(Error::State("player sharing generation exhausted"))?;
        self.offer = None;
        Ok(self.generation)
    }
    fn check(&self, generation: u64) -> Result<()> {
        if self.generation != generation
            || self.offer.as_ref().is_some_and(|offer| {
                offer.issued.elapsed() >= LIFETIME || offer.original_host.is_cancelled()
            })
        {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct Host(Mutex<State>);
impl Host {
    fn begin(&self) -> Result<u64> {
        self.0.lock().map_err(|_| Error::LockPoisoned)?.advance()
    }
    fn check(&self, generation: u64) -> Result<()> {
        self.0
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .check(generation)
    }
    fn publish(
        &self,
        auth: &Authority<'_>,
        generation: u64,
        original_host: Cancellation,
    ) -> Result<String> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        state.check(generation)?;
        require_host(&original_host)?;
        let receipt = uuid::Uuid::new_v4().simple().to_string();
        state.offer = Some(Offer {
            owner: auth.owner.clone(),
            target: auth.target.clone(),
            api: Arc::downgrade(&auth.api),
            revision: auth.revision.clone(),
            playback: auth.playback.clone(),
            original_host,
            receipt: receipt.clone(),
            issued: Instant::now(),
            choosing: false,
        });
        Ok(receipt)
    }
    fn choose(&self, auth: &Authority<'_>, receipt: &str) -> Result<u64> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let generation = state.generation;
        state.check(generation)?;
        let offer = state.offer.as_mut().ok_or(Error::StaleResponse)?;
        if offer.choosing
            || &offer.owner != auth.owner
            || &offer.target != auth.target
            || !offer.api.ptr_eq(&Arc::downgrade(&auth.api))
            || offer.revision != auth.revision
            || offer.playback != auth.playback
            || offer.receipt != receipt
        {
            return Err(Error::StaleResponse);
        }
        offer.choosing = true;
        Ok(generation)
    }
    fn cancel(
        &self,
        owner: &SessionToken,
        target: &PlayerSongShareTarget,
        receipt: &str,
    ) -> Result<()> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if state.offer.as_ref().is_some_and(|offer| {
            &offer.owner == owner && &offer.target == target && offer.receipt == receipt
        }) {
            state.advance()?;
        }
        Ok(())
    }
    fn finish(&self, generation: u64) {
        if let Ok(mut state) = self.0.lock()
            && state.generation == generation
        {
            state.offer = None;
        }
    }

    fn complete<T>(&self, generation: u64, finish: impl FnOnce() -> Result<T>) -> Result<T> {
        let state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        state.check(generation)?;
        if !state.offer.as_ref().is_some_and(|offer| offer.choosing) {
            return Err(Error::StaleResponse);
        }
        finish()
    }
}

struct FinishChoice<'a>(&'a Host, u64);
impl Drop for FinishChoice<'_> {
    fn drop(&mut self) {
        self.0.finish(self.1);
    }
}

enum Access {
    Allowed,
    Unavailable,
    AlbumRequired,
}

fn require_host(authority: &Cancellation) -> Result<()> {
    if authority.is_cancelled() {
        Err(Error::StaleResponse)
    } else {
        Ok(())
    }
}

impl MusicClient {
    fn share_authority<'a>(
        &self,
        owner: &'a SessionToken,
        controller: &'a PlaybackController,
        target: &'a PlayerSongShareTarget,
        playback: Playback,
    ) -> Result<Authority<'a>> {
        let api = self.session.native_api(owner)?;
        let revision = api
            .player_song_share_credential_revision()
            .map_err(stale_credentials)?
            .ok_or(Error::State(
                "player sharing requires current PC credentials",
            ))?;
        Ok(Authority {
            owner,
            controller,
            target,
            api,
            revision,
            playback,
        })
    }

    fn with_current_share<T>(
        &self,
        auth: &Authority<'_>,
        op: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        self.auth.pending_guard()()?;
        if Playback::capture(self, auth.controller, auth.owner, auth.target)?.as_ref()
            != Some(&auth.playback)
        {
            return Err(Error::StaleResponse);
        }
        self.session
            .with_verified_profile(auth.owner, &auth.api, |_, _| {
                if auth
                    .api
                    .player_song_share_credential_revision()
                    .map_err(stale_credentials)?
                    .as_deref()
                    != Some(auth.revision.as_str())
                {
                    return Err(Error::StaleResponse);
                }
                op()
            })
    }

    async fn player_song_share_access(
        &self,
        auth: &Authority<'_>,
        generation: u64,
        preparing_host: Option<&Cancellation>,
    ) -> Result<Access> {
        let permission = || {
            if let Some(host) = preparing_host {
                require_host(host)?;
            }
            self.with_current_share(auth, || self.player.song_share.check(generation))
        };
        permission()?;
        let check = &permission;
        let response = tokio::time::timeout(
            READ_TIMEOUT,
            self.session.run_for(auth.owner, |api, _| async move {
                if !Arc::ptr_eq(&api, &auth.api) {
                    return Err(Error::StaleResponse);
                }
                api.request_player_song_share_privilege(
                    &auth.playback.song.id,
                    &auth.revision,
                    check,
                )
                .await
            }),
        )
        .await
        .map_err(|_| Error::Response("song sharing check timed out"))?;
        permission()?;
        let response = response.map_err(stale_credentials)?;
        let rows = response["data"]
            .as_array()
            .ok_or(Error::Response("song privileges missing"))?;
        let allowed = song_share_allowed(rows, &auth.playback.song.id)?;
        // The read may reveal a private or substituted identity after the
        // earlier public metadata check; no channel can use that row.
        if rows.iter().any(|row| {
            private_privilege(row)
                || private_song(row)
                || (js_truthy(&row["onlineTrackId"])
                    && canonical_id(&row["onlineTrackId"]).as_deref()
                        != Some(auth.playback.song.id.as_str()))
        }) {
            return Ok(Access::Unavailable);
        }
        Ok(if allowed {
            Access::Allowed
        } else if auth.playback.song.album_required {
            Access::AlbumRequired
        } else {
            Access::Unavailable
        })
    }

    pub async fn player_song_share_prepare(
        &self,
        epoch: &str,
        target: PlayerSongShareTarget,
    ) -> Result<AccountData<HistoryRowSharePreparation>> {
        self.player_song_share_prepare_with_guard(epoch, target, Cancellation::default())
            .await
    }

    /// Capture the exact native host lifetime before preparing this offer.
    /// The predicate must be cheap and must not acquire a client/account lock.
    pub async fn player_song_share_prepare_with_guard(
        &self,
        epoch: &str,
        target: PlayerSongShareTarget,
        original_host: Cancellation,
    ) -> Result<AccountData<HistoryRowSharePreparation>> {
        use HistoryRowSharePreparation as Prepare;
        require_host(&original_host)?;
        let owner = self.player_token(epoch)?;
        let controller = self.player.controller.get().ok_or(Error::StaleResponse)?;
        let envelope = |value| AccountData {
            epoch: epoch.into(),
            value,
        };
        let Some(playback) = Playback::capture(self, controller, &owner, &target)? else {
            return Ok(envelope(Prepare::Unavailable));
        };
        self.auth.pending_guard()()?;
        if owner.uid.is_none() {
            // The recovered PC reads guest rights before login. Cumulus has
            // no verified guest privilege scope; only return the login intent.
            self.session.check_generation(&owner)?;
            return Ok(envelope(Prepare::LoginRequired));
        }
        let auth = self.share_authority(&owner, controller, &target, playback)?;
        let generation = self.with_current_share(&auth, || self.player.song_share.begin())?;
        let access = self
            .player_song_share_access(&auth, generation, Some(&original_host))
            .await?;
        self.with_current_share(&auth, || {
            self.player.song_share.check(generation)?;
            require_host(&original_host)?;
            Ok(envelope(match access {
                Access::Allowed => Prepare::Ready {
                    receipt: self
                        .player
                        .song_share
                        .publish(&auth, generation, original_host)?,
                },
                Access::Unavailable => Prepare::Unavailable,
                Access::AlbumRequired => Prepare::AlbumRequired,
            }))
        })
    }

    pub async fn player_song_share_choose(
        &self,
        epoch: &str,
        target: PlayerSongShareTarget,
        receipt: &str,
        choice: HistoryRowShareChoice,
    ) -> Result<AccountData<HistoryRowShareOutcome>> {
        self.player_song_share_choose_with(epoch, target, receipt, choice, std::convert::identity)
            .await
    }

    /// Trusted host completion: keep the final account and offer check in
    /// the same short synchronous step as handing the result to the host.
    /// The callback must not call back into the client or start network I/O.
    pub async fn player_song_share_choose_with<T>(
        &self,
        epoch: &str,
        target: PlayerSongShareTarget,
        receipt: &str,
        choice: HistoryRowShareChoice,
        finish: impl FnOnce(HistoryRowShareOutcome) -> T,
    ) -> Result<AccountData<T>> {
        use HistoryRowShareOutcome as Outcome;
        let owner = self.player_token(epoch)?;
        if owner.uid.is_none() {
            return Err(Error::StaleResponse);
        }
        let controller = self.player.controller.get().ok_or(Error::StaleResponse)?;
        let playback =
            Playback::capture(self, controller, &owner, &target)?.ok_or(Error::StaleResponse)?;
        let auth = self.share_authority(&owner, controller, &target, playback)?;
        let generation =
            self.with_current_share(&auth, || self.player.song_share.choose(&auth, receipt))?;
        let _finish = FinishChoice(&self.player.song_share, generation);
        let access = self
            .player_song_share_access(&auth, generation, None)
            .await?;
        let value = match access {
            Access::Allowed => {
                let url = format!("https://music.163.com/song?id={}", auth.playback.song.id);
                match choice {
                    HistoryRowShareChoice::Copy => Outcome::Copy { url },
                    HistoryRowShareChoice::WechatQr => Outcome::WechatQr {
                        modules: qr_modules(&url)?,
                    },
                }
            }
            Access::Unavailable => Outcome::Unavailable,
            Access::AlbumRequired => Outcome::AlbumRequired,
        };
        self.with_current_share(&auth, || {
            self.player.song_share.complete(generation, || {
                // A same-account selection may change while acquiring the
                // verified account and offer locks. Read the latest actor
                // identity again at the point of native publication.
                auth.playback.check_selected(self, &auth)?;
                Ok(AccountData {
                    epoch: epoch.into(),
                    value: finish(value),
                })
            })
        })
    }

    pub fn player_song_share_cancel(
        &self,
        epoch: &str,
        target: PlayerSongShareTarget,
        receipt: &str,
    ) -> Result<()> {
        let owner = self.player_token(epoch)?;
        self.player.song_share.cancel(&owner, &target, receipt)
    }
}

#[cfg(test)]
mod tests;
