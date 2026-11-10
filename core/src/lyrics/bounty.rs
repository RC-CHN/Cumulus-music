//! M/185 lyric bounty: distinct from a lyric error report or browser upload.
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    protocol::api::{LyricBountyPermission, identifier},
    storage::{OperationLock, StateStore},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

const JOURNAL: &str = "lyric-bounty-v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LyricBountyKind {
    MissingLyrics,
    Translation,
    Scrolling,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorDetails {
    txt: &'static str,
    err_type: u8,
    err_title: &'static str,
}

impl LyricBountyKind {
    fn details(self) -> ErrorDetails {
        let (txt, err_type, err_title) = match self {
            Self::MissingLyrics => ("补歌词", 6, "求歌词"),
            Self::Translation => ("翻译歌词", 7, "求翻译"),
            Self::Scrolling => ("补歌词", 8, "求滚动"),
        };
        ErrorDetails {
            txt,
            err_type,
            err_title,
        }
    }
}

/// Only the native lyric flow can construct a fixed write. The API transport
/// has no caller-provided URL, currency, amount, version, or arbitrary xInfo.
pub struct LyricBountyWrite {
    song_id: String,
    kind: LyricBountyKind,
}

impl LyricBountyWrite {
    pub const PATH: &str = "/api/v1/feedback/lyric";
    pub(crate) const SONG_LYRIC_PATH: &str = "/api/song/lyric/v1";

    pub(crate) fn new(song_id: &str, kind: LyricBountyKind) -> Result<Self> {
        Ok(Self {
            song_id: identifier(&json!(song_id))?,
            kind,
        })
    }

    fn key(&self) -> String {
        format!(
            "{}:{}",
            self.song_id.trim_start_matches('0'),
            self.kind.details().err_type
        )
    }

    pub(crate) fn into_payload(self) -> Result<Value> {
        #[derive(Serialize)]
        struct Info {
            lyric_error: ErrorDetails,
            detail: &'static str,
        }
        let lyric_error = self.kind.details();
        let error_type = lyric_error.err_type;
        // Match M/185's JSON.stringify order inside the string value.
        let x_info = serde_json::to_string(&Info {
            lyric_error,
            detail: "",
        })?;
        Ok(json!({
            "songId": self.song_id, "errorType": error_type, "lyricType": "lrc",
            "lyricVersion": 0, "xInfo": x_info
        }))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LyricBountyStatus {
    /// A structured successful response, not proof of balance or refund.
    Accepted,
    /// Provably never entered the business request backend.
    NotSent,
    /// A response or local write was lost. No reissue or refund is inferred.
    Unknown,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Record {
    Sending,
    Accepted,
    NotSent,
    Unknown,
}

impl From<Record> for LyricBountyStatus {
    fn from(value: Record) -> Self {
        match value {
            Record::Accepted => Self::Accepted,
            Record::NotSent => Self::NotSent,
            Record::Sending | Record::Unknown => Self::Unknown,
        }
    }
}

pub(crate) struct Bounties {
    session: Arc<AccountSession>,
    token: SessionToken,
    store: Arc<StateStore>,
}

impl Bounties {
    pub(crate) fn new(session: Arc<AccountSession>, token: SessionToken) -> Result<Self> {
        session.with_token(&token, |_| Ok(()))?;
        let store = session.persistent_store()?;
        Ok(Self {
            session,
            token,
            store,
        })
    }

    fn stored(&self, owner: &str, key: &str) -> Result<Option<Record>> {
        self.store
            .get(owner, JOURNAL, key)?
            .map(serde_json::from_value)
            .transpose()
            .map_err(Into::into)
    }

    pub(crate) fn status(&self, write: &LyricBountyWrite) -> Result<Option<LyricBountyStatus>> {
        self.session.with_token(&self.token, |owner| {
            Ok(self.stored(owner, &write.key())?.map(Into::into))
        })
    }

    pub(crate) async fn submit(
        &self,
        write: LyricBountyWrite,
        expected_revision: &str,
        authorize: &LyricBountyPermission<'_>,
    ) -> Result<LyricBountyStatus> {
        let key = write.key();
        let owner = self
            .session
            .with_token(&self.token, |uid| Ok(uid.to_owned()))?;
        let _operation = OperationLock::acquire(
            self.store.directory(),
            &owner,
            &format!("lyric-bounty:{key}"),
        )?;
        if let Some(existing) = self.status(&write)?
            && existing != LyricBountyStatus::NotSent
        {
            return Ok(existing);
        }
        self.session
            .run_for(&self.token, |api, original_owner| async move {
                self.session.require_api(&self.token, &api)?;
                if api.lyric_bounty_credential_revision()?.as_deref() != Some(expected_revision) {
                    return Err(Error::CredentialChanged);
                }
                // Once marked Sending, dropping this future or crashing can never
                // authorize another backend attempt for this account/song/kind.
                self.session.with_token(&self.token, |active| {
                    self.store
                        .put(active, JOURNAL, &key, &json!(Record::Sending))
                })?;
                let permission = || {
                    self.session.require_api(&self.token, &api)?;
                    authorize()
                };
                let result = api
                    .request_lyric_bounty(write, expected_revision, &permission)
                    .await;
                let record = match result {
                    Ok(value) if value["code"] == 200 => Record::Accepted,
                    Err(error) if matches!(error.root(), Error::RequestNotSent(_)) => {
                        Record::NotSent
                    }
                    // The recovered endpoint does not prove no-debit semantics
                    // for any refusal, non-200 response or malformed reply.
                    Ok(_) | Err(_) => Record::Unknown,
                };
                // A late receipt stays with the original account. If this write
                // fails, the durable Sending record still reads as Unknown.
                self.store
                    .put(&original_owner, JOURNAL, &key, &json!(record))?;
                Ok(record.into())
            })
            .await
    }
}
