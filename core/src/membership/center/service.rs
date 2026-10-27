use super::loaders;
use crate::{
    Error, Result,
    account::{AccountSession, SessionStatus, SessionToken},
    storage::{OperationLock, StateStore},
};
use serde_json::{Value, json};
use std::sync::Arc;

pub struct MemberCenter {
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
    owner: String,
}
impl MemberCenter {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        let snapshot = session.snapshot()?;
        if snapshot.status != SessionStatus::Authenticated {
            return Err(Error::StaleResponse);
        }
        Ok(Self {
            store: session.persistent_store()?,
            owner: snapshot.token.uid.ok_or(Error::StaleResponse)?,
            session,
        })
    }
    fn token(&self) -> Result<SessionToken> {
        let snapshot = self.session.snapshot()?;
        if snapshot.status != SessionStatus::Authenticated
            || snapshot.token.uid.as_deref() != Some(&self.owner)
        {
            return Err(Error::StaleResponse);
        }
        Ok(snapshot.token)
    }
    fn lock(&self) -> Result<OperationLock> {
        OperationLock::acquire(self.store.directory(), &self.owner, "member-center")
    }
    fn stored(&self, token: &SessionToken) -> Result<Value> {
        self.session.with_token(token, |uid| {
            Ok(self
                .store
                .get(uid, "member_center", "state")?
                .unwrap_or_else(|| json!({"status":"idle","results":[]})))
        })
    }
    pub fn snapshot(&self) -> Result<Value> {
        let token = self.token()?;
        self.snapshot_for(&token)
    }
    pub(crate) fn snapshot_for(&self, token: &SessionToken) -> Result<Value> {
        self.session.with_token(token, |uid| {
            if uid != self.owner {
                return Err(Error::StaleResponse);
            }
            Ok(())
        })?;
        let mut value = self.stored(token)?;
        if value["status"] == "loading" {
            // A dropped Future/process releases the file lock. Normalize its
            // persisted loading state while retaining the previous content.
            match self.lock() {
                Ok(_lock) => {
                    value["status"] = json!("error");
                    value["error"] = json!({"class":"interrupted"});
                    self.session
                        .save_result(token, "member_center", "state", &value)?;
                }
                Err(Error::MutationBusy) => (),
                Err(error) => return Err(error),
            }
        }
        Ok(value)
    }
    pub async fn refresh(&self) -> Result<Value> {
        let token = self.token()?;
        self.refresh_for(&token).await
    }
    pub(crate) async fn refresh_for(&self, token: &SessionToken) -> Result<Value> {
        self.refresh_checked_for(token, |_| Ok(())).await
    }
    pub(crate) async fn refresh_checked_for(
        &self,
        token: &SessionToken,
        validate: fn(&Value) -> Result<()>,
    ) -> Result<Value> {
        self.session.with_token(token, |uid| {
            if uid != self.owner {
                return Err(Error::StaleResponse);
            }
            Ok(())
        })?;
        let _lock = self.lock()?;
        let previous = self.stored(token)?;
        let mut loading = previous.clone();
        loading["status"] = json!("loading");
        loading["error"] = Value::Null;
        self.session
            .save_result(token, "member_center", "state", &loading)?;
        let result = self
            .session
            .run_for(token, |api, _| async move { loaders::load(api).await })
            .await
            .and_then(|value| {
                validate(&value)?;
                Ok(value)
            });
        match result {
            Ok(value) => {
                self.session
                    .save_result(token, "member_center", "state", &value)?;
                Ok(value)
            }
            Err(error) => {
                let mut failed = previous;
                failed["status"] = json!("error");
                let code = match &error {
                    Error::Service { code, .. } => code.clone(),
                    Error::HttpStatus {
                        body: Some(body), ..
                    } => body["code"].clone(),
                    _ => Value::Null,
                };
                failed["error"] = json!({"class":if matches!(&error,Error::Service{..}|Error::HttpStatus{..}) {"service"} else {"transport_or_state"},"code":code});
                // A stale/expired identity cannot update any account's UI
                // cache. Last saved contents remain available after relogin.
                match self
                    .session
                    .save_result(token, "member_center", "state", &failed)
                {
                    Ok(()) | Err(Error::StaleResponse) => (),
                    Err(storage) => return Err(storage),
                };
                Err(error)
            }
        }
    }
    pub async fn history(&self, date: Option<Value>) -> Result<Vec<Value>> {
        let token = self.token()?;
        self.history_for(&token, date).await
    }
    fn history_key(date: &Option<Value>) -> Result<String> {
        Ok(match date {
            None => "dates".into(),
            Some(Value::String(text))
                if !text.is_empty() && text != "dates" && text.chars().count() <= 32 =>
            {
                text.clone()
            }
            Some(Value::Number(number)) if number.is_i64() || number.is_u64() => number.to_string(),
            _ => return Err(Error::InvalidInput("invalid recommendation date")),
        })
    }
    pub(crate) fn history_cached_for(
        &self,
        token: &SessionToken,
        date: Option<Value>,
    ) -> Result<(Vec<Value>, f64)> {
        let key = Self::history_key(&date)?;
        self.session.with_token(token, |uid| {
            if uid != self.owner {
                return Err(Error::StaleResponse);
            }
            let value = self
                .store
                .get(uid, "recommendation_history", &key)?
                .ok_or(Error::State("recommendation history is not cached"))?;
            let fetched = self
                .store
                .get(uid, "recommendation_history_meta", &key)?
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            Ok((
                value
                    .as_array()
                    .ok_or(Error::Response("invalid stored recommendation history"))?
                    .clone(),
                fetched,
            ))
        })
    }
    pub(crate) async fn history_for(
        &self,
        token: &SessionToken,
        date: Option<Value>,
    ) -> Result<Vec<Value>> {
        self.history_checked_for(token, date, |_| Ok(())).await
    }
    pub(crate) async fn history_checked_for(
        &self,
        token: &SessionToken,
        date: Option<Value>,
        validate: fn(&[Value]) -> Result<()>,
    ) -> Result<Vec<Value>> {
        self.session.with_token(token, |uid| {
            if uid != self.owner {
                return Err(Error::StaleResponse);
            }
            Ok(())
        })?;
        let key = Self::history_key(&date)?;
        let detail = date.is_some();
        let path = if detail {
            "/api/discovery/recommend/songs/history/detail"
        } else {
            "/api/discovery/recommend/songs/history/recent"
        };
        let value = self
            .session
            .run_for(token, |api, _| async move {
                api.call(
                    path,
                    match date {
                        Some(date) => json!({"date":date}),
                        None => json!({}),
                    },
                    false,
                    &[200],
                )
                .await
            })
            .await?;
        let rows = value["data"][if detail { "songs" } else { "dates" }]
            .as_array()
            .ok_or(Error::Response("missing recommendation history items"))?
            .clone();
        let fetched = time::OffsetDateTime::now_utc().unix_timestamp_nanos() as f64 / 1e9;
        validate(&rows)?;
        self.session.with_token(token, |uid| {
            self.store.transaction(|tx| {
                tx.put(uid, "recommendation_history", &key, &json!(rows))?;
                tx.put(uid, "recommendation_history_meta", &key, &json!(fetched))
            })
        })?;
        Ok(rows)
    }
}
