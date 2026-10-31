use super::*;
use crate::{
    catalog::CatalogApi,
    download::offline::OfflineLibrary,
    fm::{FmService, FmTrack},
    playback::intelligence::eligibility::{identical_recording, online_candidate},
};
use std::collections::{BTreeMap, HashSet};

impl MusicClient {
    pub(super) async fn intelligence_current_seed(
        &self,
        flight: &Flight,
        current: &Value,
    ) -> Result<Value> {
        let id = crate::playback::intelligence::resource_id(current)?;
        let mut resource = current.clone();
        if !resource["song"].is_object() {
            let batch = flight
                .wait(
                    &self.session,
                    self.session.run_for(&flight.token, |api, _| async {
                        CatalogApi::new(api).songs(&[json!(id)], 1).await
                    }),
                )
                .await?;
            resource["song"] = batch
                .songs
                .into_iter()
                .next()
                .flatten()
                .ok_or(Error::State("intelligence seed is unavailable"))?;
        }
        if identifier(&resource["song"]["id"])? != id {
            return Err(Error::State("intelligence seed is unavailable"));
        }
        let blacklist = FmService::new(self.session.clone())?.blacklist_for(&flight.token)?;
        if blacklist.blocks(&FmTrack {
            entry_id: String::new(),
            id,
            algorithm: String::new(),
            song: resource["song"].clone(),
        }) {
            return Err(Error::State("intelligence seed is blocked"));
        }
        flight.check(&self.session)?;
        Ok(resource)
    }
    pub(super) async fn intelligence_filter(
        &self,
        flight: &Flight,
        songs: Vec<Option<Value>>,
    ) -> Result<Vec<Value>> {
        let token = &flight.token;
        let offline = OfflineLibrary::new(self.session.clone())?;
        let records = offline.records(token)?;
        if records.iter().any(|r| r.encrypted) {
            // Failed renewal cannot grant offline access; online authorization
            // is still available. Account/cancel errors are never swallowed.
            let _ = flight
                .wait(&self.session, offline.refresh_membership(token))
                .await;
            flight.check(&self.session)?;
        }
        let blacklist = flight
            .wait(
                &self.session,
                FmService::new(self.session.clone())?.refresh_blacklist_for(token),
            )
            .await?;
        let mut resources = vec![];
        let mut replacements = vec![];
        for song in songs {
            let Some(song) = song else { break };
            flight.check(&self.session)?;
            let id = identifier(&song["id"])?;
            // Community policy: remove blocked candidates before queuing,
            // rather than reproduce the official AB-gated playback skip UI.
            if blacklist.blocks(&FmTrack {
                entry_id: String::new(),
                id: id.clone(),
                algorithm: String::new(),
                song: song.clone(),
            }) {
                continue;
            }
            let mut resource = json!({"id":id,"dt":song["dt"],"song":song});
            let mut downloaded = false;
            for record in records.iter().filter(|r| r.song_id == id) {
                let checked = flight
                    .wait(&self.session, offline.check(token, &record.path, false))
                    .await;
                flight.check(&self.session)?;
                if let Ok(checked) = checked
                    && checked.access.can_play
                    && checked.integrity_verified
                {
                    resource["path"] = json!(checked.record.path);
                    resource["level"] = json!(checked.record.quality);
                    resource["download_ledger_owner"] = json!(token.uid);
                    resource["download_ledger"] = serde_json::to_value(checked.record)?;
                    downloaded = true;
                    break;
                }
            }
            if downloaded || online_candidate(&resource["song"])? {
                resources.push(resource);
            } else if let Some(backup) = identical_recording(&resource["song"]) {
                replacements.push((resources.len(), backup));
                resources.push(resource);
            }
        }
        let ids: Vec<_> = replacements.iter().map(|(_, id)| json!(id)).collect();
        if !ids.is_empty() {
            let batch = flight
                .wait(
                    &self.session,
                    self.session.run_for(token, |api, _| async move {
                        CatalogApi::new(api).songs(&ids, 500).await
                    }),
                )
                .await?;
            let found: BTreeMap<_, _> = batch
                .songs
                .into_iter()
                .flatten()
                .map(|s| Ok((identifier(&s["id"])?, s)))
                .collect::<Result<_>>()?;
            for (index, id) in replacements {
                if let Some(song) = found.get(&id) {
                    resources[index] = json!({"id":id,"dt":song["dt"],"song":song});
                }
            }
        }
        // M/59 first-ID dedup runs after substitutions. A blocked replacement
        // never slips through merely because its original ID was allowed.
        let mut seen = HashSet::new();
        resources.retain(|r| {
            seen.insert(r["id"].clone().to_string())
                && !blacklist.blocks(&FmTrack {
                    entry_id: String::new(),
                    id: r["id"].as_str().unwrap_or_default().into(),
                    algorithm: String::new(),
                    song: r["song"].clone(),
                })
        });
        Ok(resources)
    }
}
