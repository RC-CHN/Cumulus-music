use crate::{
    Error, Result,
    account::SessionToken,
    client::{MusicClient, dto::AccountData},
    playback::controller::{PlaybackSnapshot, PlaybackStatus},
    podcast::{AudiobookSkip, PodcastApi, PodcastLibrary, PodcastScope, is_audiobook},
    protocol::api::identifier,
    storage::OperationLock,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Mutex;
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AudiobookSkipContext {
    Detail {
        scope: PodcastScope,
        revision: String,
    },
    Player {
        play_id: String,
        entry_id: String,
    },
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AudiobookSkipAction {
    Open,
    Smart {
        edit_id: String,
    },
    Save {
        edit_id: String,
        setting: AudiobookSkip,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudiobookSkipView {
    pub radio_id: String,
    pub edit_id: String,
    pub setting: AudiobookSkip,
    pub smart_available: bool,
}

struct Edit {
    id: String,
    owner: SessionToken,
    context: AudiobookSkipContext,
    radio_id: String,
    previous: AudiobookSkip,
    smart: Smart,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Smart {
    Unrequested,
    Checking,
    Ready(bool),
}

#[derive(Default)]
pub(in crate::client) struct Host(Mutex<Option<Edit>>);

fn matching<'a>(
    edit: Option<&'a mut Edit>,
    id: &str,
    owner: &SessionToken,
    context: &AudiobookSkipContext,
    radio: &str,
) -> Result<&'a mut Edit> {
    edit.filter(|edit| {
        edit.id == id && edit.owner == *owner && edit.context == *context && edit.radio_id == radio
    })
    .ok_or(Error::StaleResponse)
}

/// M/622 uses this narrower gate on a radio detail page only.
pub(super) fn detail_category(radio: &Value) -> bool {
    matches!(
        radio["categoryId"].as_f64(),
        Some(10001.0 | 3088097.0 | 3148096.0)
    )
}

pub(in crate::client) fn active_radio(snapshot: &PlaybackSnapshot) -> Option<String> {
    if !matches!(
        snapshot.state,
        PlaybackStatus::Playing | PlaybackStatus::Paused | PlaybackStatus::Buffering
    ) || snapshot.play_id.is_none()
    {
        return None;
    }
    let resource = &snapshot.selected.as_ref()?.resource;
    let voice = resource
        .get("voice")
        .filter(|voice| voice.is_object())
        .or_else(|| resource["song"].get("voice"))?;
    let declared_id = resource
        .get("voice_id")
        .filter(|id| !id.is_null())
        .unwrap_or(&resource["song"]["voice_id"]);
    if identifier(declared_id).ok()? != identifier(&voice["id"]).ok()? {
        return None;
    }
    let radio = &voice["audio"];
    // M/622’s footer calls the broader M/144 classifier on voice.audio.
    is_audiobook(radio)
        .then(|| identifier(&radio["id"]).ok())
        .flatten()
}

impl MusicClient {
    fn audiobook_skip_radio(
        &self,
        token: &SessionToken,
        context: &AudiobookSkipContext,
    ) -> Result<String> {
        self.session.check_generation(token)?;
        match context {
            AudiobookSkipContext::Detail { scope, revision } => {
                let PodcastScope::Radio { id, .. } = scope else {
                    return Err(Error::InvalidInput("audiobook detail requires a radio"));
                };
                let snapshot = PodcastLibrary::new(self.session.clone())?
                    .cached_for(token, scope)?
                    .filter(|snapshot| snapshot.revision == *revision)
                    .ok_or(Error::StaleResponse)?;
                let radio = snapshot
                    .radios
                    .first()
                    .filter(|radio| radio.id == *id && detail_category(&radio.raw))
                    .ok_or(Error::InvalidInput("not an audiobook detail menu"))?;
                Ok(radio.id.clone())
            }
            AudiobookSkipContext::Player { play_id, entry_id } => {
                let current = self
                    .player
                    .controller
                    .get()
                    .ok_or(Error::StaleResponse)?
                    .snapshot();
                if current.account != *token
                    || current.play_id.as_deref() != Some(play_id)
                    || current.selected.as_ref().map(|item| &item.entry_id) != Some(entry_id)
                {
                    return Err(Error::StaleResponse);
                }
                active_radio(&current).ok_or(Error::InvalidInput("not an active audiobook"))
            }
        }
    }

    pub async fn audiobook_skip_settings(
        &self,
        epoch: &str,
        context: AudiobookSkipContext,
        action: AudiobookSkipAction,
    ) -> Result<AccountData<AudiobookSkipView>> {
        let token = self.podcast_token(epoch)?;
        let radio_id = self.audiobook_skip_radio(&token, &context)?;
        let store = self.session.persistent_store()?;
        let (edit_id, setting, smart_available) = match action {
            AudiobookSkipAction::Open => {
                let edit_id = Uuid::new_v4().simple().to_string();
                let mut slot = self
                    .audiobook_skip
                    .0
                    .lock()
                    .map_err(|_| Error::LockPoisoned)?;
                if self.audiobook_skip_radio(&token, &context)? != radio_id {
                    return Err(Error::StaleResponse);
                }
                let setting = self.session.with_token(&token, |owner| {
                    AudiobookSkip::read(&store, owner, &radio_id)
                })?;
                *slot = Some(Edit {
                    id: edit_id.clone(),
                    owner: token.clone(),
                    context,
                    radio_id: radio_id.clone(),
                    previous: setting,
                    smart: Smart::Unrequested,
                });
                (edit_id, setting, false)
            }
            AudiobookSkipAction::Smart { edit_id } => {
                let (setting, existing) = {
                    let mut slot = self
                        .audiobook_skip
                        .0
                        .lock()
                        .map_err(|_| Error::LockPoisoned)?;
                    let edit = matching(slot.as_mut(), &edit_id, &token, &context, &radio_id)?;
                    let existing = match edit.smart {
                        Smart::Unrequested => {
                            edit.smart = Smart::Checking;
                            None
                        }
                        Smart::Checking => {
                            return Err(Error::State("smart availability is already loading"));
                        }
                        Smart::Ready(available) => Some(available),
                    };
                    (edit.previous, existing)
                };
                let smart_available = if let Some(available) = existing {
                    available
                } else {
                    let available = self
                        .session
                        .run_for(&token, |api, _| async {
                            PodcastApi::new(api).skip_enabled(&json!(radio_id)).await
                        })
                        .await
                        .unwrap_or(false);
                    let mut slot = self
                        .audiobook_skip
                        .0
                        .lock()
                        .map_err(|_| Error::LockPoisoned)?;
                    let edit = matching(slot.as_mut(), &edit_id, &token, &context, &radio_id)?;
                    if self.audiobook_skip_radio(&token, &context)? != radio_id {
                        return Err(Error::StaleResponse);
                    }
                    edit.smart = Smart::Ready(available);
                    available
                };
                (edit_id, setting, smart_available)
            }
            AudiobookSkipAction::Save { edit_id, setting } => {
                let mut slot = self
                    .audiobook_skip
                    .0
                    .lock()
                    .map_err(|_| Error::LockPoisoned)?;
                let edit = matching(slot.as_mut(), &edit_id, &token, &context, &radio_id)?;
                if self.audiobook_skip_radio(&token, &context)? != radio_id {
                    return Err(Error::StaleResponse);
                }
                let smart_available = edit.smart == Smart::Ready(true);
                self.session.with_token(&token, |owner| {
                    let _guard = OperationLock::acquire(
                        store.directory(),
                        owner,
                        &format!("audiobook-skip:{radio_id}"),
                    )?;
                    let current = AudiobookSkip::read(&store, owner, &radio_id)?;
                    if current != edit.previous {
                        return Err(Error::StaleResponse);
                    }
                    if setting == AudiobookSkip::Smart && !smart_available {
                        // An existing, now hidden Smart preference can be left
                        // as-is, but no new Smart preference is granted offline.
                        if current == setting {
                            return Ok(());
                        }
                        return Err(Error::InvalidInput("smart audiobook skip is unavailable"));
                    }
                    setting.save(&store, owner, &radio_id)
                })?;
                *slot = None;
                if (setting != AudiobookSkip::Smart || smart_available)
                    && let Some(player) = self.player.controller.get()
                {
                    // Saving is already complete. The actor separately fences
                    // its active radio and never promises a retroactive seek.
                    let _ = player.refresh_audiobook_skip(&token, &radio_id);
                }
                (edit_id, setting, smart_available)
            }
        };
        self.session.check_generation(&token)?;
        Ok(AccountData {
            epoch: epoch.to_owned(),
            value: AudiobookSkipView {
                radio_id,
                edit_id,
                setting,
                smart_available,
            },
        })
    }
}
