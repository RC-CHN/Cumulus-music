//! The official new-release action disables notifications, not artist following.
use super::MusicClient;
use crate::{Error, Result, client::dto::AccountData, protocol::api::identifier};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const SWITCH: &str = "/api/sub/artist/publish/switch";
const JOURNAL: &str = "followed-artist-release-notification-v1";

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseNotificationAction {
    Inspect,
    Disable,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseNotificationState {
    Ready,
    Confirmed,
    Uncertain,
}

fn read(record: Option<Value>) -> Result<ReleaseNotificationState> {
    let Some(record) = record else {
        return Ok(ReleaseNotificationState::Ready);
    };
    match serde_json::from_value(record)? {
        ReleaseNotificationState::Ready => Err(Error::State("invalid release notification record")),
        state => Ok(state),
    }
}

fn rejected(error: &Error) -> bool {
    matches!(error.root(), Error::RequestNotSent(_))
        || matches!(error.root(), Error::Service { code, .. }
            if code.as_i64().is_some_and(|n| (400..500).contains(&n) || n == 301))
}

impl MusicClient {
    pub async fn release_notification(
        &self,
        epoch: &str,
        artist: &str,
        action: ReleaseNotificationAction,
    ) -> Result<AccountData<ReleaseNotificationState>> {
        let artist = identifier(&json!(artist))?;
        if artist.len() > 128 {
            return Err(Error::InvalidInput(
                "oversized release notification artist ID",
            ));
        }
        // Official M/15 Ng forwards the artist ID unchanged. Keep it as
        // decimal text so an IPC client cannot round a large identifier.
        let token = self.playlist_token(epoch)?;
        let store = self.session.persistent_store()?;
        let account_data = |value| AccountData {
            epoch: epoch.into(),
            value,
        };
        let state = self.session.with_token(&token, |uid| match action {
            ReleaseNotificationAction::Inspect => read(store.get(uid, JOURNAL, &artist)?),
            ReleaseNotificationAction::Disable => store.transaction(|tx| {
                let state = read(tx.get(uid, JOURNAL, &artist)?)?;
                if state == ReleaseNotificationState::Ready {
                    // Store the conservative state before transport, so a
                    // different process, a crash, or a late reply cannot replay.
                    tx.put(
                        uid,
                        JOURNAL,
                        &artist,
                        &json!(ReleaseNotificationState::Uncertain),
                    )?;
                }
                Ok(state)
            }),
        })?;
        if matches!(action, ReleaseNotificationAction::Inspect)
            || state != ReleaseNotificationState::Ready
        {
            return Ok(account_data(state));
        }

        let request_artist = artist.clone();
        let response = self
            .session
            .run_for(&token, |api, _| async move {
                api.call(
                    SWITCH,
                    json!({"artistId": request_artist, "switchStatus": 0}),
                    false,
                    &[200],
                )
                .await
            })
            .await;
        self.session.with_token(&token, |uid| match response {
            Ok(_) => {
                store.put(
                    uid,
                    JOURNAL,
                    &artist,
                    &json!(ReleaseNotificationState::Confirmed),
                )?;
                Ok(account_data(ReleaseNotificationState::Confirmed))
            }
            Err(error) if rejected(&error) => {
                store.delete(uid, JOURNAL, &artist)?;
                Err(error)
            }
            Err(_) => Ok(account_data(ReleaseNotificationState::Uncertain)),
        })
    }
}
