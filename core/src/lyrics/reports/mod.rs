//! Official M109/M3085 correction form, with explicit one-send intents.
mod model;
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    storage::{OperationLock, StateStore},
};
pub use model::*;
use serde_json::json;
use std::sync::Arc;
const JOURNAL: &str = "lyric-reports-v1";

pub struct Reports {
    session: Arc<AccountSession>,
    token: SessionToken,
    store: Arc<StateStore>,
}
impl Reports {
    pub fn new(session: Arc<AccountSession>, token: SessionToken) -> Result<Self> {
        session.with_token(&token, |_| Ok(()))?;
        let store = session.persistent_store()?;
        Ok(Self {
            session,
            token,
            store,
        })
    }
    fn gate(&self, song: &str) -> Result<OperationLock> {
        let owner = self.session.with_token(&self.token, |v| Ok(v.to_owned()))?;
        OperationLock::acquire(
            self.store.directory(),
            "lyric-report",
            &serde_json::to_string(&(owner, song))?,
        )
    }
    fn stored(&self, owner: &str, id: &str) -> Result<Record> {
        uuid::Uuid::parse_str(id).map_err(|_| Error::InvalidInput("invalid lyric report ID"))?;
        let value: Record = serde_json::from_value(
            self.store
                .get(owner, JOURNAL, id)?
                .ok_or(Error::StaleResponse)?,
        )?;
        value.validate()?;
        if value.id != id {
            return Err(Error::State("lyric report identity mismatch"));
        }
        Ok(value)
    }
    fn save(&self, record: &mut Record) -> Result<()> {
        record.revision = uuid::Uuid::new_v4().to_string();
        record.validate()?;
        self.session.with_token(&self.token, |owner| {
            self.store
                .put(owner, JOURNAL, &record.id, &serde_json::to_value(&record)?)
        })
    }
    pub fn read(&self, id: &str) -> Result<Record> {
        self.session
            .with_token(&self.token, |owner| self.stored(owner, id))
    }
    pub fn open(&self, target: Target) -> Result<Record> {
        target.validate()?;
        let _gate = self.gate(&target.song_id)?;
        let existing = self.session.with_token(&self.token, |owner| {
            self.store
                .values(owner, JOURNAL)?
                .keys()
                .map(|id| self.stored(owner, id))
                .collect::<Result<Vec<_>>>()
        })?;
        // An unknown response blocks a new report for the song, even after a
        // lyric version changed. Reopening is never permission to send again.
        if let Some(record) = existing.iter().find(|r| {
            r.target.song_id == target.song_id
                && matches!(r.phase, Phase::Sending | Phase::Uncertain)
        }) {
            return Ok(record.clone());
        }
        if let Some(record) = existing
            .into_iter()
            .find(|r| r.target == target && matches!(r.phase, Phase::Draft | Phase::Rejected))
        {
            return Ok(record);
        }
        let mut record = Record {
            id: uuid::Uuid::new_v4().to_string(),
            revision: uuid::Uuid::new_v4().to_string(),
            target,
            options: Vec::new(),
            selected: None,
            content: String::new(),
            phase: Phase::Draft,
            issue: None,
        };
        self.save(&mut record)?;
        Ok(record)
    }
    pub async fn types(&self, id: &str, revision: &str) -> Result<Record> {
        let record = self.read(id)?;
        let _gate = self.gate(&record.target.song_id)?;
        let mut record = self.read(id)?;
        if record.revision != revision || !matches!(record.phase, Phase::Draft | Phase::Rejected) {
            return Err(Error::StaleResponse);
        }
        let options = self
            .session
            .run_for(&self.token, |api, _| async move {
                parse_types(&api.call(TYPES_PATH, json!({}), false, &[200]).await?)
            })
            .await?;
        record.options = options;
        if record
            .selected
            .as_ref()
            .is_some_and(|v| !record.options.iter().any(|o| &o.key == v))
        {
            record.selected = None;
        }
        self.save(&mut record)?;
        Ok(record)
    }
    pub async fn submit(
        &self,
        id: &str,
        revision: &str,
        key: &str,
        content: &str,
    ) -> Result<Record> {
        validate_content(content)?;
        let record = self.read(id)?;
        let _gate = self.gate(&record.target.song_id)?;
        let mut record = self.read(id)?;
        if record.revision != revision {
            return Err(Error::StaleResponse);
        }
        if !matches!(record.phase, Phase::Draft | Phase::Rejected) {
            return Err(Error::MutationUncertain);
        }
        let choice = record
            .options
            .iter()
            .find(|o| o.key == key)
            .ok_or(Error::InvalidInput("choose a loaded lyric report type"))?
            .value;
        record.selected = Some(key.into());
        record.content = content.into();
        record.phase = Phase::Sending;
        record.issue = None;
        let mut payload =
            json!({"songId":record.target.song_id,"content":content,"correctType":choice});
        if let Some(version) = &record.target.lyric_version {
            payload["lyricVersion"] = version.clone();
        }
        let owner = self.token.uid.clone().ok_or(Error::StaleResponse)?;
        let result = self
            .session
            .run_for(&self.token, |api, _| async move {
                // Only mark Sending after obtaining this account's API, and
                // durably before the first permitted transport attempt.
                self.save(&mut record)?;
                let result = api.call(SUBMIT_PATH, payload, false, &[200]).await;
                match &result {
                    Ok(_) => {
                        record.phase = Phase::Sent;
                        record.issue = None;
                    }
                    Err(error) => {
                        let not_sent = matches!(error.root(), Error::RequestNotSent(_));
                        let rejected = [250, 301, 403, 404, 405, 480, 505, 506, 511, 512]
                            .into_iter()
                            .any(|c| error.is_service_code(c));
                        record.phase = if not_sent || rejected {
                            Phase::Rejected
                        } else {
                            Phase::Uncertain
                        };
                        record.issue = Some(if not_sent {
                            Issue::NotSent
                        } else if rejected {
                            Issue::Rejected
                        } else {
                            Issue::Unknown
                        });
                    }
                }
                record.revision = uuid::Uuid::new_v4().to_string();
                // Keep a late receipt only under its original account, before the
                // session layer rejects delivery to a replacement generation.
                self.store
                    .put(&owner, JOURNAL, id, &serde_json::to_value(&record)?)?;
                result.map(|_| record)
            })
            .await;
        self.session.check_generation(&self.token)?;
        match result {
            Ok(record) => Ok(record),
            Err(error) => {
                let stored = self.read(id)?;
                if stored.revision == revision {
                    Err(error)
                } else {
                    Ok(stored)
                }
            }
        }
    }
    /// Stops tracking locally; never retracts, resends, or asserts failure.
    pub fn abandon(&self, id: &str, revision: &str) -> Result<Record> {
        let record = self.read(id)?;
        let _gate = self.gate(&record.target.song_id)?;
        let mut record = self.read(id)?;
        if record.revision != revision {
            return Err(Error::StaleResponse);
        }
        record.phase = Phase::Abandoned;
        self.save(&mut record)?;
        Ok(record)
    }
}
