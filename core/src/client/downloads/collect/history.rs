//! Recently played uses the same picker but freezes a separate source policy.
use super::{
    AccountData, Context, HistoryCollectTarget, HistoryKind, HistoryService, MusicClient,
    Selection, Source,
};
use crate::{Error, Result, catalog::CatalogApi, protocol::api::identifier};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, atomic::Ordering},
    time::Instant,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryCollectPlan {
    pub receipt: Option<String>,
    pub can_create: bool,
    pub selected_count: usize,
    pub song_ids: Vec<String>,
}

pub(in crate::client) struct OfflineSources {
    context: Context,
    tasks: BTreeMap<String, String>,
}

impl OfflineSources {
    fn matches(context: &Context, song: &str, task: &str) -> Result<bool> {
        let item = match context.service.get_for(&context.token, task) {
            Ok(item) => item,
            Err(Error::State("download does not belong to this account")) => return Ok(false),
            Err(error) => return Err(error),
        };
        Ok(item.matches_completed_history_song(song))
    }

    pub(in crate::client) fn require(&self) -> Result<()> {
        for (song, task) in &self.tasks {
            if !Self::matches(&self.context, song, task)? {
                return Err(Error::QueueChanged);
            }
        }
        Ok(())
    }

    pub(in crate::client) fn task_ids(&self) -> &BTreeMap<String, String> {
        &self.tasks
    }
}

impl MusicClient {
    pub(in crate::client) fn resolve_history_catalogue_downloads(
        &self,
        epoch: &str,
        snapshot: &mut crate::history::HistorySnapshot,
        candidates: Vec<(String, String)>,
    ) -> Result<Option<OfflineSources>> {
        if candidates.is_empty() {
            return Ok(None);
        }
        let context = self.downloads.inner.context(epoch)?;
        let mut tasks = BTreeMap::new();
        for (song, task) in candidates {
            if OfflineSources::matches(&context, &song, &task)? {
                tasks.insert(song, task);
            }
        }
        for row in &mut snapshot.entries {
            if tasks.contains_key(&row.id) {
                row.resource
                    .as_object_mut()
                    .expect("verified history song")
                    .remove("download_task_id");
            }
        }
        Ok((!tasks.is_empty()).then_some(OfflineSources { context, tasks }))
    }

    pub async fn history_collect_prepare(
        &self,
        epoch: &str,
        target: HistoryCollectTarget,
    ) -> Result<AccountData<HistoryCollectPlan>> {
        let token = self.player_token(epoch)?;
        let api = self.session.native_api(&token)?;
        let credentials = api
            .download_collection_scope_revision()?
            .ok_or(Error::State("guarded history collection is unavailable"))?;
        let ticket = self.begin_download_collection()?;
        let history = HistoryService::new(self.session.clone())?;
        let mut snapshot = history.snapshot_for(&token, HistoryKind::Song)?;
        let candidates = target.download_candidates(&snapshot)?;
        let mut offline =
            self.resolve_history_catalogue_downloads(epoch, &mut snapshot, candidates)?;
        let missing = target.unobserved_privilege_ids(&snapshot)?;
        if !missing.is_empty() {
            // The local playback mapper does not retain all privilege fields.
            // Use the same ordinary rights read as remote history, on a copy;
            // absent results remain unknown and cannot produce a write.
            let requested = missing.iter().collect::<BTreeSet<_>>();
            let mut observed = BTreeMap::new();
            for privilege in CatalogApi::new(api.clone())
                .privileges(&missing, None)
                .await?
            {
                let id = identifier(&privilege["id"])?;
                if !requested.contains(&id) || observed.insert(id, privilege).is_some() {
                    return Err(Error::Response("unexpected history collection privilege"));
                }
            }
            for row in &mut snapshot.entries {
                if let Some(privilege) = observed.remove(&row.id) {
                    row.resource["privilege"] = privilege;
                }
            }
        }
        self.session.require_api(&token, &api)?;
        if self.downloads.inner.collect_prepare.load(Ordering::Acquire) != ticket
            || api.download_collection_scope_revision()?.as_deref() != Some(&credentials)
        {
            return Err(Error::StaleResponse);
        }
        if history.snapshot_for(&token, HistoryKind::Song)?.revision != target.revision() {
            return Err(Error::QueueChanged);
        }
        let (selected_count, song_ids) = target.project(&snapshot)?;
        if let Some(sources) = &mut offline {
            let eligible = song_ids.iter().collect::<BTreeSet<_>>();
            sources.tasks.retain(|song, _| eligible.contains(song));
        }
        let mut value = HistoryCollectPlan {
            receipt: None,
            can_create: false,
            selected_count,
            song_ids,
        };
        if !value.song_ids.is_empty() {
            let receipt = uuid::Uuid::new_v4().simple().to_string();
            value.can_create = api
                .download_collection_create_scope_revision()
                .ok()
                .flatten()
                .as_deref()
                == Some(&credentials);
            self.install_download_collection(
                Arc::new(Source {
                    session: self.session.clone(),
                    token,
                    api,
                    credentials,
                    receipt: receipt.clone(),
                    selection: Selection::History {
                        target,
                        songs: value.song_ids.clone(),
                        offline: offline
                            .filter(|sources| !sources.tasks.is_empty())
                            .map(Box::new),
                    },
                    created: Instant::now(),
                }),
                ticket,
            )?;
            value.receipt = Some(receipt);
        }
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
}
