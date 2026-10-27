//! Normalize official reason choices and bind them to the native current entry.
use super::*;
use crate::{Error, Result, account::SessionToken, protocol::api::identifier};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
const REASONS: &str = "fm-reasons-v1";
const NAMES: &str = "fm-artist-names-v1";
impl FmService {
    pub async fn reason_choices_for(
        &self,
        token: &SessionToken,
        source: &FmSource,
        entry_id: &str,
    ) -> Result<FmReasons> {
        let queue = self
            .queue_for(token, source)?
            .ok_or(Error::State("FM has not been loaded"))?;
        let track = queue
            .current()
            .filter(|t| t.entry_id == entry_id)
            .ok_or(Error::QueueChanged)?;
        let value = self.reasons_for(token, &track.id).await?;
        let rows = value["data"]
            .as_array()
            .ok_or(Error::Response("missing FM reasons"))?;
        let mut choices = FmReasons {
            entry_id: entry_id.into(),
            song_id: track.id.clone(),
            options: vec![],
        };
        for row in rows {
            let label = row["reason"]
                .as_str()
                .filter(|s| !s.is_empty() && s.chars().count() <= 200)
                .ok_or(Error::Response("invalid FM reason label"))?;
            let artist_id = match row["type"].as_str() {
                Some("fixed") => None,
                Some("artist") => Some(identifier(&row["id"])?),
                _ => continue,
            };
            let key = hex::encode(Sha256::digest(serde_json::to_vec(&json!([
                label, artist_id
            ]))?));
            if !choices.options.iter().any(|r| r.key == key) {
                choices.options.push(FmReason {
                    key,
                    label: label.into(),
                    artist_id,
                });
            }
        }
        // Do not publish a dialog for a different song after a slow request.
        let current = self.queue_for(token, source)?.ok_or(Error::QueueChanged)?;
        if current.current().is_none_or(|t| t.entry_id != entry_id) {
            return Err(Error::QueueChanged);
        }
        self.session.with_token(token, |owner| {
            self.store.transaction(|tx| {
                tx.put(owner, REASONS, entry_id, &serde_json::to_value(&choices)?)?;
                for reason in &choices.options {
                    if let Some(id) = &reason.artist_id {
                        let name = reason
                            .label
                            .strip_prefix("屏蔽歌手：")
                            .or_else(|| reason.label.strip_prefix("屏蔽歌手:"));
                        if let Some(name) = name {
                            tx.put(owner, NAMES, id, &json!({"name":name}))?;
                        }
                    }
                }
                Ok(())
            })
        })?;
        Ok(choices)
    }
    pub(super) fn reason_for(
        &self,
        token: &SessionToken,
        track: &FmTrack,
        key: &str,
    ) -> Result<FmReason> {
        self.session.with_token(token, |owner| {
            let choices: FmReasons = serde_json::from_value(
                self.store
                    .get(owner, REASONS, &track.entry_id)?
                    .ok_or(Error::State("reload FM reasons"))?,
            )?;
            if choices.song_id != track.id || choices.entry_id != track.entry_id {
                return Err(Error::QueueChanged);
            }
            choices
                .options
                .into_iter()
                .find(|r| r.key == key)
                .ok_or(Error::InvalidInput("unknown FM reason"))
        })
    }
    pub fn artist_name_for(&self, token: &SessionToken, id: &str) -> Result<Option<String>> {
        self.session.with_token(token, |owner| {
            Ok(self
                .store
                .get(owner, NAMES, id)?
                .and_then(|v| v.get("name").and_then(Value::as_str).map(str::to_owned)))
        })
    }
}
