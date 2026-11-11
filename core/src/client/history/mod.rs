mod batch_scope;
mod song_collection;
pub(super) mod song_download;
pub use song_collection::HistoryCollectTarget;
mod dto;
mod file_menu;
pub(super) use file_menu::HistoryRowFileMenuHost;
mod radio_chapter;
mod radio_collection;
pub(super) mod row_menu;
mod row_menu_info;
mod share;
pub(super) use share::HistoryRowShareHost;
mod song_file_menu;
pub use song_file_menu::HistorySongFileReveal;
pub(super) mod voice_file_menu;
pub(super) use radio_chapter::RadioChaptersHost;
pub use voice_file_menu::HistoryVoiceFileReveal;
mod listened_books;
pub(super) use listened_books::ListenedBooksHost;
pub use listened_books::{ListenedBooksPage, ListenedBooksQuery};
mod sync_observer;
pub(super) use sync_observer::HistorySyncHost;
mod selection;
pub(in crate::client) use selection::{Selection, SelectionRows};
mod view;
use super::{MusicClient, dto::AccountData};
use crate::{
    Error, Result,
    history::{HistoryKind, HistoryService, HistorySyncAction, HistorySyncState},
};
pub use dto::*;
use std::collections::{HashMap, HashSet};

impl MusicClient {
    pub fn history_sync_preference(
        &self,
        epoch: &str,
        action: HistorySyncAction,
    ) -> Result<AccountData<HistorySyncState>> {
        let token = self.player_token(epoch)?;
        let value = HistoryService::new(self.session.clone())?.sync_setting_for(&token, action)?;
        self.session.with_token(&token, |_| {
            if matches!(action, HistorySyncAction::Set { .. }) {
                self.history_sync.publish(token.generation, value);
            }
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }

    pub async fn history_page(
        &self,
        epoch: &str,
        kind: HistoryKind,
        refresh: bool,
    ) -> Result<AccountData<HistoryPage>> {
        let token = self.player_token(epoch)?;
        let service = HistoryService::new(self.session.clone())?;
        let mut snapshot = service.snapshot_for(&token, kind)?;
        if refresh && token.uid.is_some() && snapshot.sync_enabled {
            snapshot = service.refresh_for(&token, kind).await?;
        }
        let local = snapshot
            .entries
            .iter()
            .filter(|r| r.resource["local_id"].is_string())
            .map(|r| (r.id.clone(), r.resource.clone()))
            .collect::<Vec<_>>();
        let available = if local.is_empty() {
            HashSet::new()
        } else {
            let context = self.local.context().await?;
            tokio::task::spawn_blocking(move || {
                local
                    .into_iter()
                    .filter_map(|(id, raw)| {
                        let resource = context.library.resource(raw["local_id"].as_str()?).ok()?;
                        (serde_json::to_value(resource.local_revision).ok()?
                            == raw["local_revision"])
                            .then_some(id)
                    })
                    .collect::<HashSet<_>>()
            })
            .await
            .map_err(|_| Error::State("history local inspection stopped"))?
        };
        let mut operations = service
            .mutations_for(&token)?
            .into_iter()
            .filter(|op| op.kind == kind)
            .collect::<Vec<_>>();
        operations.sort_by_key(|op| {
            (
                std::cmp::Reverse(op.phase.unresolved()),
                std::cmp::Reverse(op.created_at),
            )
        });
        operations.truncate(20);
        let chapters = radio_chapter::supported(kind);
        let hidden = if chapters && token.uid.is_some() {
            let api = self.session.native_api(&token)?;
            self.hidden_radio_progress(&token, &api)
        } else {
            HashSet::new()
        };
        self.session.with_local_owner(&token, |owner| {
            let viewer = token.uid.as_deref();
            let (entries, chapter_revision) = if chapters {
                let store = self.session.persistent_store()?;
                let (entries, selectable) = store.read_snapshot(|tx| {
                    let mut entries = Vec::with_capacity(snapshot.entries.len());
                    let mut selectable = HashMap::new();
                    for row in snapshot.entries {
                        let (chapter, selected) =
                            radio_chapter::view(tx, owner, &row, token.uid.is_some(), &hidden)?;
                        if let Some(selected) = selected {
                            selectable.insert(row.id.clone(), selected);
                        }
                        entries.push(view::entry(kind, row, false, chapter, viewer)?);
                    }
                    Ok((entries, selectable))
                })?;
                let receipt = self.history_radio_chapters.publish(
                    &token,
                    kind,
                    &snapshot.revision,
                    selectable,
                )?;
                (entries, Some(receipt))
            } else {
                let entries = snapshot
                    .entries
                    .into_iter()
                    .map(|row| {
                        let usable = available.contains(&row.id);
                        view::entry(kind, row, usable, None, viewer)
                    })
                    .collect::<Result<_>>()?;
                (entries, None)
            };
            Ok(AccountData {
                epoch: epoch.into(),
                value: HistoryPage {
                    kind,
                    revision: snapshot.revision,
                    chapter_revision,
                    sync_enabled: snapshot.sync_enabled,
                    can_sync: token.uid.is_some(),
                    remote_included: snapshot.sync_enabled && snapshot.remote.is_some(),
                    remote_fetched_at: snapshot.remote.map(|r| r.fetched_at),
                    local_count: snapshot.local.len(),
                    entries,
                    operations: operations.into_iter().map(view::operation).collect(),
                },
            })
        })
    }
    pub async fn history_command(
        &self,
        epoch: &str,
        kind: HistoryKind,
        command: HistoryCommand,
    ) -> Result<AccountData<HistoryPage>> {
        let token = self.player_token(epoch)?;
        let service = HistoryService::new(self.session.clone())?;
        match command {
            HistoryCommand::CancelUnseen {
                id,
                expected_revision,
                action,
            } => {
                service.cancel_unseen_for(&token, &id, kind, &expected_revision, action)?;
            }
            HistoryCommand::SetSync { enabled } => {
                service.set_sync_for(&token, enabled)?;
                if token.uid.is_some()
                    && let Ok(value) = service.sync_setting_for(&token, HistorySyncAction::Read)
                {
                    self.session.with_token(&token, |_| {
                        self.history_sync.publish(token.generation, value);
                        Ok(())
                    })?;
                }
            }
            HistoryCommand::Mutate {
                id,
                expected_revision,
                action,
            } => {
                service
                    .mutate_for(&token, &id, kind, &expected_revision, action)
                    .await?;
            }
            HistoryCommand::Reconcile { id } => {
                require_kind(&service, &token, &id, kind)?;
                service.reconcile_for(&token, &id).await?;
            }
            HistoryCommand::AcceptCurrent {
                id,
                expected_revision,
            } => {
                require_kind(&service, &token, &id, kind)?;
                service.accept_current_for(&token, &id, &expected_revision)?;
            }
        }
        self.history_page(epoch, kind, false).await
    }
}
fn require_kind(
    service: &HistoryService,
    token: &crate::account::SessionToken,
    id: &str,
    kind: HistoryKind,
) -> Result<()> {
    if !service
        .mutations_for(token)?
        .iter()
        .any(|op| op.id == id && op.kind == kind)
    {
        return Err(Error::InvalidInput("history operation category mismatch"));
    }
    Ok(())
}
