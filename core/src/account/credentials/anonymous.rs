//! M115 anonymous token and bounded one-send registration receipt.
use super::*;
use crate::protocol::{api::identifier, pc_experiments::PcAnonymousState};
use std::sync::atomic::{AtomicBool, Ordering};

const ANONYMOUS: &str = "anonymous-session-v1";

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case", deny_unknown_fields)]
enum Phase {
    Prepared,
    Sending { attempt: String },
    Registered { uid: String },
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Anonymous {
    device_id: String,
    username: String,
    state: Phase,
}

fn record(value: Value, device: &str) -> Result<Option<Anonymous>> {
    let record: Anonymous = serde_json::from_value(value)?;
    if record.device_id != device {
        return Ok(None);
    }
    if record.username.is_empty()
        || record.username.len() > 16_384
        || !record
            .username
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-=".contains(&c))
    {
        return Err(Error::State("invalid cached PC anonymous token"));
    }
    if let Phase::Registered { uid } = &record.state
        && (identifier(&serde_json::json!(uid)).is_err() || uid.len() > 128)
    {
        return Err(Error::State("invalid cached PC anonymous identity"));
    }
    Ok(Some(record))
}

impl Credentials {
    /// Same installation/native device, but never carry account Cookies into
    /// the independent anonymous jar. A changed device retires only guest data.
    pub(crate) fn seed_anonymous_device(&self, device_id: &str) -> Result<()> {
        self.change(|record| {
            if jar(record)?
                .iter_unexpired()
                .any(|cookie| cookie.name() == "MUSIC_U" && !cookie.value().is_empty())
            {
                return Err(Error::State("anonymous store contains an account Cookie"));
            }
            if record.device_id != device_id {
                record.device_id = device_id.into();
                record.cookies.clear();
                record.refresh_token = None;
                record.login_rotation = None;
                record.last_login_at = None;
            }
            Ok(())
        })
    }
    pub(crate) fn anonymous_state(&self) -> Result<PcAnonymousState> {
        let state = self.lock()?;
        self.require_current(&state)?;
        let cookies = jar(&state.record)?;
        if cookies
            .iter_unexpired()
            .any(|cookie| cookie.name() == "MUSIC_U" && !cookie.value().is_empty())
        {
            return Ok(PcAnonymousState::Unsupported);
        }
        let value = self
            .store
            .get(OWNER, ANONYMOUS, KEY)?
            .map(|value| record(value, &state.record.device_id))
            .transpose()?
            .flatten();
        Ok(match value.map(|value| value.state) {
            Some(Phase::Registered { uid }) if cookies.iter_unexpired().next().is_some() => {
                PcAnonymousState::Registered {
                    uid,
                    revision: state.record.revision.clone(),
                }
            }
            Some(Phase::Sending { .. }) => PcAnonymousState::Uncertain,
            _ => PcAnonymousState::Unregistered,
        })
    }

    pub(crate) fn begin_anonymous(self: &Arc<Self>) -> Result<AnonymousIntent> {
        if self.origin != ApiOrigin::Pc {
            return Err(Error::State("anonymous registration requires PC scope"));
        }
        if matches!(self.anonymous_state()?, PcAnonymousState::Unsupported) {
            return Err(Error::State(
                "anonymous registration cannot replace a logged-in Cookie",
            ));
        }
        let target = self.origin.request_url(
            crate::protocol::http::WireRoute::Eapi,
            crate::protocol::pc_experiments::ANONYMOUS_PATH,
        )?;
        let before = self.request_credentials(&target)?;
        let device = before.device_id;
        let id = Uuid::new_v4().simple().to_string();
        let username = self.store.transaction(|tx| {
            let old = tx
                .get(OWNER, ANONYMOUS, KEY)?
                .map(|value| record(value, &device))
                .transpose()?
                .flatten();
            let mut value = match old {
                Some(value) => value,
                None => Anonymous {
                    username: crate::account::anonymous_identity::encode_anonymous_id(&device)?,
                    device_id: device.clone(),
                    state: Phase::Prepared,
                },
            };
            if matches!(value.state, Phase::Sending { .. }) {
                return Err(Error::State(
                    "PC anonymous registration acknowledgement is unknown",
                ));
            }
            value.state = Phase::Sending {
                attempt: id.clone(),
            };
            tx.put(OWNER, ANONYMOUS, KEY, &serde_json::to_value(&value)?)?;
            Ok(value.username)
        })?;
        Ok(AnonymousIntent {
            credentials: self.clone(),
            attempt: id,
            username,
            revision: before.revision,
            device_id: device,
            dispatched: AtomicBool::new(false),
        })
    }
}

pub(crate) struct AnonymousIntent {
    credentials: Arc<Credentials>,
    attempt: String,
    pub(crate) username: String,
    pub(crate) revision: String,
    device_id: String,
    pub(crate) dispatched: AtomicBool,
}

impl AnonymousIntent {
    fn finish(&self, uid: Option<String>) -> Result<()> {
        self.credentials.store.transaction(|tx| {
            let mut value = tx
                .get(OWNER, ANONYMOUS, KEY)?
                .map(|value| record(value, &self.device_id))
                .transpose()?
                .flatten()
                .ok_or(Error::StaleResponse)?;
            if !matches!(&value.state, Phase::Sending { attempt } if attempt == &self.attempt) {
                return Err(Error::StaleResponse);
            }
            value.state = match uid {
                Some(uid) => Phase::Registered { uid },
                None => Phase::Prepared,
            };
            tx.put(OWNER, ANONYMOUS, KEY, &serde_json::to_value(value)?)
        })
    }

    pub(crate) fn rejected(&self) -> Result<()> {
        self.finish(None)
    }

    pub(crate) fn acknowledged(&self, uid: String) -> Result<PcAnonymousState> {
        if identifier(&serde_json::json!(&uid)).is_err() || uid.len() > 128 {
            return Err(Error::Response("invalid PC anonymous user identity"));
        }
        // M198 stores the anonymous jar without any MUSIC_U. A registration
        // response cannot turn this private guest identity into account login.
        self.credentials.change(|record| {
            record.cookies.retain(|cookie| cookie.name() != "MUSIC_U");
            Ok(())
        })?;
        let state = self.credentials.lock()?;
        let any_cookie = jar(&state.record)?.iter_unexpired().next().is_some();
        drop(state);
        if !any_cookie {
            return Err(Error::Response(
                "PC anonymous registration returned no Cookie",
            ));
        }
        self.finish(Some(uid))?;
        self.credentials.anonymous_state()
    }
}

impl Drop for AnonymousIntent {
    fn drop(&mut self) {
        if !self.dispatched.load(Ordering::Relaxed) {
            let _ = self.finish(None);
        }
    }
}
