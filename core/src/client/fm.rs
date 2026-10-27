use super::{
    MusicClient,
    dto::{AccountData, TrackView},
    mapping,
};
use crate::{
    Error, Result,
    fm::{FmRights, FmSource, SkipPhase},
    membership::{MembershipManager, Quality},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::atomic::Ordering;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FmItemView {
    pub entry_id: String,
    pub track: TrackView,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FmFeedbackView {
    pub id: String,
    pub kind: &'static str,
    pub phase: String,
    pub desired: Option<bool>,
    pub can_apply: bool,
    pub artist_id: Option<String>,
}
#[derive(Serialize)]
pub struct FmArtistView {
    pub id: String,
    pub name: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FmView {
    pub source: FmSource,
    pub active_source: Option<FmSource>,
    pub revision: Option<String>,
    pub items: Vec<FmItemView>,
    pub current: Option<String>,
    pub active: bool,
    pub pending: bool,
    pub notice: Option<&'static str>,
    pub operations: Vec<FmFeedbackView>,
    pub blocked_artists: Vec<FmArtistView>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FmTrashView {
    pub page: usize,
    pub reported_total: usize,
    pub songs: Vec<TrackView>,
    pub more: bool,
    pub cursor: Value,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FmCommand {
    Start {
        quality: Quality,
    },
    Resume,
    Leave,
    Trash {
        entry_id: String,
    },
    Feedback {
        entry_id: String,
        reason_key: String,
    },
    Reconcile {
        id: String,
    },
    ReconcileArtist {
        id: String,
    },
    Artist {
        id: String,
        blocked: bool,
    },
    RestoreTrash {
        ids: Vec<String>,
    },
    DispatchSkip {
        id: String,
    },
    AbandonSkip {
        id: String,
    },
}
impl MusicClient {
    pub async fn fm_view(
        &self,
        epoch: &str,
        source: FmSource,
        refresh: bool,
    ) -> Result<AccountData<FmView>> {
        let token = self.player_token(epoch)?;
        self.session.with_token(&token, |_| Ok(()))?;
        source.validate()?;
        let fm = self.player.fm().await?;
        let service = fm.service();
        if refresh {
            service.refresh_blacklist_for(&token).await?;
            service.refresh_trash_for(&token).await?;
            if service.queue_for(&token, &source)?.is_none() {
                let rights = MembershipManager::new(self.session.clone())?
                    .refresh_for(token.clone())
                    .await?;
                service
                    .preload_for(&token, source.clone(), FmRights::from(&rights))
                    .await?;
            }
        }
        let (_, actual) = self
            .player
            .controller()
            .await?
            .queue_with_state(&token)
            .await?;
        let active_source = fm.active_source_for(&token, &actual)?;
        let active = active_source.as_ref() == Some(&source);
        let desired = service.queue_for(&token, &source)?;
        let mut operations = Vec::new();
        for r in service.skip_operations_for(&token)? {
            if matches!(
                r.phase,
                SkipPhase::Prepared | SkipPhase::Sending | SkipPhase::Uncertain
            ) {
                operations.push(FmFeedbackView {
                    id: r.id,
                    kind: "skip",
                    phase: serde_json::to_value(r.phase)?
                        .as_str()
                        .unwrap_or_default()
                        .into(),
                    desired: None,
                    can_apply: false,
                    artist_id: None,
                });
            }
        }
        for r in service.trash_operations_for(&token)? {
            let missing_artist =
                r.artist_id.is_some() && service.artist_operation_for(&token, &r.id)?.is_none();
            let can_apply = r.desired
                && matches!(
                    r.phase,
                    crate::account::resources::MutationPhase::Confirmed
                        | crate::account::resources::MutationPhase::Unchanged
                )
                && (missing_artist
                    || desired
                        .as_ref()
                        .and_then(|q| q.current())
                        .is_some_and(|t| r.song_ids.contains(&t.id)));
            if r.phase.pending() || can_apply {
                operations.push(FmFeedbackView {
                    id: r.id,
                    kind: "trash",
                    phase: serde_json::to_value(r.phase)?
                        .as_str()
                        .unwrap_or_default()
                        .into(),
                    desired: Some(r.desired),
                    can_apply,
                    artist_id: r.artist_id,
                });
            }
        }
        for r in service.artist_operations_for(&token)? {
            if r.phase.pending()
                || (r.phase == crate::account::resources::MutationPhase::Rejected
                    && service.artist_latest_for(&token, &r.artist_id)?.as_deref() == Some(&r.id))
            {
                operations.push(FmFeedbackView {
                    id: r.id,
                    kind: "artist",
                    phase: serde_json::to_value(r.phase)?
                        .as_str()
                        .unwrap_or_default()
                        .into(),
                    desired: Some(r.desired),
                    can_apply: false,
                    artist_id: Some(r.artist_id),
                });
            }
        }
        let blocked_artists = service
            .blacklist_for(&token)?
            .artists
            .into_iter()
            .map(|id| {
                Ok(FmArtistView {
                    name: service
                        .artist_name_for(&token, &id)?
                        .unwrap_or_else(|| format!("歌手 {id}")),
                    id,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let items = desired
            .as_ref()
            .map(|q| {
                q.tracks
                    .iter()
                    .map(|t| {
                        Ok(FmItemView {
                            entry_id: t.entry_id.clone(),
                            track: mapping::track(&t.song)?,
                        })
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?
            .unwrap_or_default();
        let failure = fm.failure_for(&token)?;
        let view = FmView {
            source,
            active_source,
            revision: desired.as_ref().map(|q| q.revision.to_string()),
            items,
            current: desired
                .as_ref()
                .and_then(|q| q.current())
                .map(|t| t.entry_id.clone()),
            active,
            pending: active && fm.pending_for(&token, &actual)?,
            notice: if active {
                failure.map(|code| {
                    if code == "refill_failed" {
                        "推荐暂时加载失败，正在使用已有歌曲"
                    } else {
                        "FM播放尚未完成，请重试继续播放"
                    }
                })
            } else {
                None
            },
            operations,
            blocked_artists,
        };
        self.session.check_generation(&token)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value: view,
        })
    }
    pub async fn fm_command(
        &self,
        epoch: &str,
        source: FmSource,
        command: FmCommand,
    ) -> Result<AccountData<FmView>> {
        let token = self.player_token(epoch)?;
        self.session.with_token(&token, |_| Ok(()))?;
        let fm = self.player.fm().await?;
        source.validate()?;
        if matches!(
            command,
            FmCommand::Resume
                | FmCommand::Leave
                | FmCommand::Trash { .. }
                | FmCommand::Feedback { .. }
        ) {
            let actual = self.player.controller().await?.queue(&token).await?;
            if fm.active_source_for(&token, &actual)?.as_ref() != Some(&source) {
                return Err(Error::QueueChanged);
            }
        }
        let transport = matches!(
            command,
            FmCommand::Start { .. }
                | FmCommand::Resume
                | FmCommand::Leave
                | FmCommand::Trash { .. }
                | FmCommand::Feedback { .. }
                | FmCommand::Reconcile { .. }
        );
        if transport && self.has_together_cleanup()? {
            return Err(Error::State("leave together before entering FM"));
        }
        let affects_filter = matches!(
            command,
            FmCommand::Artist { blocked: true, .. } | FmCommand::ReconcileArtist { .. }
        );
        let intent = if transport || affects_filter {
            self.player.selection.fetch_add(1, Ordering::AcqRel) + 1
        } else {
            self.player.selection.load(Ordering::Acquire)
        };
        let lease = if matches!(
            command,
            FmCommand::Start { .. }
                | FmCommand::Resume
                | FmCommand::Trash { .. }
                | FmCommand::Feedback { .. }
                | FmCommand::Reconcile { .. }
        ) {
            Some(self.claim_media(&token, super::focus::Owner::Audio).await?)
        } else {
            None
        };
        if let Some(lease) = lease {
            self.focus.check(lease, super::focus::Owner::Audio)?;
        }
        match command {
            FmCommand::Start { quality } => {
                fm.enter_for(&token, source.clone(), quality, intent)
                    .await?;
            }
            FmCommand::Resume => {
                fm.resume_for(&token, false, intent).await?;
            }
            FmCommand::Leave => {
                fm.leave_for(&token, intent).await?;
            }
            FmCommand::Trash { entry_id } => {
                fm.trash_for(&token, &entry_id, intent).await?;
            }
            FmCommand::Feedback {
                entry_id,
                reason_key,
            } => {
                fm.feedback_for(&token, &entry_id, &reason_key, intent)
                    .await?;
            }
            FmCommand::Reconcile { id } => {
                let record = fm.service().reconcile_trash_for(&token, &id).await?;
                let (view, _) = self
                    .player
                    .controller()
                    .await?
                    .queue_with_state(&token)
                    .await?;
                if view.fm && record.desired {
                    fm.apply_confirmed_trash_for(&token, &id, intent).await?;
                } else if record.desired {
                    fm.service()
                        .finish_trash_artist_for(&token, &record)
                        .await?;
                }
            }
            FmCommand::ReconcileArtist { id } => {
                fm.service().reconcile_artist_for(&token, &id).await?;
            }
            FmCommand::Artist { id, blocked } => {
                fm.service().change_artist_for(&token, &id, blocked).await?;
            }
            FmCommand::RestoreTrash { ids } => {
                fm.service().restore_trash_for(&token, ids).await?;
            }
            FmCommand::DispatchSkip { id } => {
                fm.service().dispatch_skip_for(&token, &id).await?;
            }
            FmCommand::AbandonSkip { id } => {
                fm.service().abandon_skip_for(&token, &id)?;
            }
        }
        if affects_filter {
            let queue = self.player.controller().await?.queue(&token).await?;
            if fm.active_source_for(&token, &queue)?.is_some() {
                fm.filter_for(&token, intent).await?;
            }
        }
        self.fm_view(epoch, source, false).await
    }
    pub async fn fm_reasons(
        &self,
        epoch: &str,
        source: FmSource,
        entry_id: &str,
    ) -> Result<AccountData<crate::fm::FmReasons>> {
        let token = self.player_token(epoch)?;
        let fm = self.player.fm().await?;
        let queue = self.player.controller().await?.queue(&token).await?;
        if fm.active_source_for(&token, &queue)?.as_ref() != Some(&source)
            || queue.current.as_deref() != Some(entry_id)
        {
            return Err(Error::QueueChanged);
        }
        let value = fm
            .service()
            .reason_choices_for(&token, &source, entry_id)
            .await?;
        self.session.check_generation(&token)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
    pub async fn fm_trash(
        &self,
        epoch: &str,
        page: usize,
        cursor: Value,
    ) -> Result<AccountData<FmTrashView>> {
        let token = self.player_token(epoch)?;
        let result = self
            .player
            .fm()
            .await?
            .service()
            .trash_page_for(&token, page, 50, cursor)
            .await?;
        let view = FmTrashView {
            page,
            reported_total: result.total,
            more: result.more || (result.total == 0 && result.tracks.len() == 50),
            cursor: result.add_time,
            songs: result
                .tracks
                .iter()
                .map(mapping::track)
                .collect::<Result<_>>()?,
        };
        self.session.check_generation(&token)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value: view,
        })
    }
}
