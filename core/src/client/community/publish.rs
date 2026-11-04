//! Recovered PC events branch: text and one catalog music attachment.
//! The local journal never treats an uncorrelated feed card as a receipt.
use super::{
    AccountData, CardLease, Locator, MusicClient,
    composer::{Attachment, ComposerDraft, ComposerKind, DRAFT_KEY, DRAFT_KIND, validate_text},
    handle, project,
};
use crate::{
    Error, Result,
    account::{SessionToken, profile::AccountProfile},
    protocol::wire::serialization::encode_component,
    storage::{OperationLock, StateStore, StateTransaction},
};
use md5::{Digest, Md5};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const PUBLISH: &str = "/api/share/friends/resource";
const JOURNAL: &str = "community-composer-publication-v1";
const CURRENT: &str = "latest";
const NONCE: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ComposerPublishAction {
    Status,
    CheckOwnNotes,
    Submit { revision: String },
}

#[derive(Serialize)]
#[serde(
    tag = "status",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ComposerPublication {
    Ready,
    PhoneRequired,
    PreflightUnavailable,
    Rejected {
        revision: String,
        reason: PublishRejection,
    },
    Uncertain {
        revision: String,
    },
    Published {
        revision: String,
        event_id: String,
        detail: String,
    },
    Removed {
        revision: String,
    },
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PublishRejection {
    CannotShare,
    PrivateCloud,
    PurchaseRequired,
    NotSent,
}

#[derive(Deserialize, Serialize)]
struct Record {
    revision: String,
    uuid: String,
    phase: Phase,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum Phase {
    Pending,
    Confirmed { event: String, detail: String },
    Rejected { reason: PublishRejection },
}

fn read_record(tx: &StateTransaction<'_>, uid: &str) -> Result<Option<Record>> {
    let record = tx
        .get(uid, JOURNAL, CURRENT)?
        .map(serde_json::from_value::<Record>)
        .transpose()?;
    if let Some(record) = &record {
        handle(&record.revision)?;
        if let Phase::Confirmed { detail, .. } = &record.phase {
            handle(detail)?;
        }
        if record.uuid.len() != 32
            || !record
                .uuid
                .bytes()
                .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
            || matches!(&record.phase, Phase::Confirmed {event, ..}
                if project::id(&json!(event)).as_ref() != Some(event))
        {
            return Err(Error::State("invalid community publication journal"));
        }
    }
    Ok(record)
}

pub(super) fn submission_unresolved(tx: &StateTransaction<'_>, uid: &str) -> Result<bool> {
    Ok(read_record(tx, uid)?.is_some_and(|record| matches!(record.phase, Phase::Pending)))
}

fn read_draft(tx: &StateTransaction<'_>, uid: &str, revision: &str) -> Result<ComposerDraft> {
    let draft = tx
        .get(uid, DRAFT_KIND, DRAFT_KEY)?
        .map(serde_json::from_value::<ComposerDraft>)
        .transpose()?
        .filter(|draft| draft.revision == revision)
        .ok_or(Error::StaleResponse)?;
    validate_text(&draft.text)?;
    if draft.text.trim().is_empty()
        || draft
            .attachment
            .as_ref()
            .is_none_or(|item| project::id(&json!(item.id)).as_ref() != Some(&item.id))
    {
        return Err(Error::InvalidInput(
            "community publication requires text and a catalog attachment",
        ));
    }
    Ok(draft)
}

fn publish_type(kind: ComposerKind) -> &'static str {
    match kind {
        ComposerKind::Song => "song",
        ComposerKind::Artist => "artist",
        ComposerKind::Album => "album",
        ComposerKind::Playlist => "playlist",
        ComposerKind::Podcast => "djradio",
    }
}

fn publication_uuid(nonce: &str, time: u64, uid: &str, device: &str) -> String {
    // M3106.1203–1206 uses M32.u, M18's encoded device ID and M314.a.
    // M314 consumes the low byte of UTF-16; the canonical input here is ASCII.
    let input = format!("{nonce}-share-{time}-{uid}-{}", encode_component(device));
    hex::encode(Md5::digest(input.as_bytes()))
}

fn fresh_uuid(uid: &str, device: &str) -> Result<String> {
    let now = u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::State("community publication clock is invalid"))?
            .as_millis(),
    )
    .map_err(|_| Error::State("community publication clock is out of range"))?;
    let mut rng = rand::rng();
    // Same eight uppercase base-36 characters as M32.u(8); Rust obtains them
    // from its native RNG rather than recreating V8's floating conversion.
    let nonce: String = (0..8)
        .map(|_| char::from(NONCE[rng.random_range(0..NONCE.len())]))
        .collect();
    Ok(publication_uuid(&nonce, now, uid, device))
}

fn request(attachment: &Attachment, text: &str, uuid: &str) -> Value {
    json!({"id":attachment.id,"msg":text,"type":publish_type(attachment.kind),"uuid":uuid,
        "active":0,"pics":"[]"})
}

fn confirmed_event(response: &Value, uid: &str, uuid: &str) -> Option<String> {
    let event = response.get("event")?;
    if !event["anonymityInfo"].is_null()
        || project::id(&event["user"]["userId"]).as_deref() != Some(uid)
        || event["uuid"]
            .as_str()
            .is_some_and(|echo| !echo.is_empty() && echo != uuid)
        || (!event["uuid"].is_null() && !event["uuid"].is_string())
    {
        return None;
    }
    project::id(&event["id"])
}

fn own_note_receipt(response: &Value, uid: &str, uuid: &str) -> Result<Option<String>> {
    let events = response["events"]
        .as_array()
        .filter(|rows| rows.len() <= 20)
        .ok_or(Error::Response("invalid own notes receipt page"))?;
    let mut candidates = events.iter().filter(|event| {
        event["uuid"].as_str() == Some(uuid)
            && event["anonymityInfo"].is_null()
            && !project::tombstone(event)
            && project::id(&event["user"]["userId"]).as_deref() == Some(uid)
    });
    let event = candidates.next();
    // An ambiguous or incomplete response must keep the journal pending.
    if candidates.next().is_some() {
        return Ok(None);
    }
    Ok(event.and_then(|event| project::id(&event["id"])))
}

fn rejection(error: &Error) -> Option<PublishRejection> {
    if matches!(error.root(), Error::RequestNotSent(_)) {
        return Some(PublishRejection::NotSent);
    }
    if error.is_service_code(401) {
        Some(PublishRejection::CannotShare)
    } else if error.is_service_code(510) {
        Some(PublishRejection::PrivateCloud)
    } else if error.is_service_code(512) {
        Some(PublishRejection::PurchaseRequired)
    } else {
        None
    }
}

impl MusicClient {
    fn publication_view(
        &self,
        token: &SessionToken,
        owner: &str,
        record: Record,
    ) -> Result<ComposerPublication> {
        let revision = record.revision;
        Ok(match record.phase {
            Phase::Pending => ComposerPublication::Uncertain { revision },
            Phase::Rejected { reason } => ComposerPublication::Rejected { revision, reason },
            Phase::Confirmed { event, detail } => {
                if self.note_removed(owner, &event)? {
                    return Ok(ComposerPublication::Removed { revision });
                }
                let key = handle(&detail)?;
                let mut state = self.published_notes.state(token)?;
                if !state.cards.iter().any(|record| record.key == key) {
                    state.cards.push_back(CardLease {
                        topic: None,
                        key,
                        event: event.clone(),
                        locator: Some(Locator {
                            event: event.clone(),
                            owner: owner.into(),
                        }),
                        comments_receipt: None,
                        detail_read: None,
                    });
                    if state.cards.len() > 16 {
                        state.cards.pop_front();
                    }
                }
                ComposerPublication::Published {
                    revision,
                    event_id: event,
                    detail,
                }
            }
        })
    }

    fn settle_publication(
        &self,
        token: &SessionToken,
        store: &StateStore,
        record: Record,
    ) -> Result<ComposerPublication> {
        self.session.with_token(token, |uid| {
            if !matches!(record.phase, Phase::Pending) {
                store.transaction(|tx| {
                    let prior = read_record(tx, uid)?.filter(|prior| {
                        prior.uuid == record.uuid
                            && prior.revision == record.revision
                            && matches!(prior.phase, Phase::Pending)
                    });
                    if prior.is_none() {
                        return Err(Error::StaleResponse);
                    }
                    tx.put(uid, JOURNAL, CURRENT, &serde_json::to_value(&record)?)?;
                    if matches!(record.phase, Phase::Confirmed { .. })
                        && tx
                            .get(uid, DRAFT_KIND, DRAFT_KEY)?
                            .as_ref()
                            .and_then(|value| value["revision"].as_str())
                            == Some(record.revision.as_str())
                    {
                        tx.delete(uid, DRAFT_KIND, DRAFT_KEY)?;
                    }
                    Ok(())
                })?;
            }
            self.publication_view(token, uid, record)
        })
    }

    async fn check_own_note_receipt(
        &self,
        token: &SessionToken,
        store: &StateStore,
        owner: &str,
    ) -> Result<ComposerPublication> {
        let _lock = OperationLock::acquire(store.directory(), JOURNAL, owner)?;
        let record = self
            .session
            .with_token(token, |uid| store.read_snapshot(|tx| read_record(tx, uid)))?;
        let Some(mut record) = record else {
            return Ok(ComposerPublication::Ready);
        };
        if !matches!(record.phase, Phase::Pending) {
            return self
                .session
                .with_token(token, |uid| self.publication_view(token, uid, record));
        }
        let path = format!("/api/event/get/{owner}");
        let response = self
            .session
            .run_for(token, |api, _| async move {
                api.call(
                    &path,
                    json!({"offset":0,"limit":20,"getcounts":true,"time":-1}),
                    false,
                    &[200],
                )
                .await
            })
            .await?;
        if let Some(event) = own_note_receipt(&response, owner, &record.uuid)? {
            record.phase = Phase::Confirmed {
                event,
                detail: Uuid::new_v4().to_string(),
            };
        }
        self.settle_publication(token, store, record)
    }

    pub async fn community_composer_publish(
        &self,
        epoch: &str,
        action: ComposerPublishAction,
    ) -> Result<AccountData<ComposerPublication>> {
        let token = self.playlist_token(epoch)?;
        let store = self.session.persistent_store()?;
        let owner = self.session.with_token(&token, |owner| {
            project::id(&json!(owner)).ok_or(Error::State("invalid community publication owner"))
        })?;
        let reply = |value| AccountData {
            epoch: epoch.into(),
            value,
        };
        let revision = match action {
            ComposerPublishAction::Submit { revision } => revision,
            ComposerPublishAction::CheckOwnNotes => {
                let value = self.check_own_note_receipt(&token, &store, &owner).await?;
                return Ok(reply(value));
            }
            ComposerPublishAction::Status => {
                // Opening the editor and checking a local receipt never go online.
                let value = self.session.with_token(&token, |uid| {
                    match store.read_snapshot(|tx| read_record(tx, uid))? {
                        None => Ok(ComposerPublication::Ready),
                        Some(record) => self.publication_view(&token, uid, record),
                    }
                })?;
                return Ok(reply(value));
            }
        };
        handle(&revision)?;
        let _lock = OperationLock::acquire(store.directory(), JOURNAL, &owner)?;
        let current = self.session.with_token(&token, |uid| {
            store.read_snapshot(|tx| {
                if let Some(record) = read_record(tx, uid)?
                    && (matches!(record.phase, Phase::Pending)
                        || (record.revision == revision
                            && matches!(record.phase, Phase::Confirmed { .. })))
                {
                    return Ok(Some(record));
                }
                read_draft(tx, uid, &revision)?;
                Ok(None)
            })
        })?;
        if let Some(record) = current {
            let value = self
                .session
                .with_token(&token, |uid| self.publication_view(&token, uid, record))?;
            return Ok(reply(value));
        }
        let api = self.session.native_api(&token)?;
        let known_binding = self
            .session
            .with_verified_profile(&token, &api, |_, profile| {
                Ok(profile["bindings"].as_array().is_some_and(|rows| {
                    rows.len() <= 32 && rows.iter().any(|row| row["type"].as_u64() == Some(1))
                }))
            })?;
        if !known_binding {
            let bindings = AccountProfile::new(self.session.clone())
                .bindings_for(&token)
                .await;
            self.session.check_generation(&token)?;
            let Ok(bindings) = bindings else {
                return Ok(reply(ComposerPublication::PreflightUnavailable));
            };
            if !bindings.iter().any(|binding| binding.type_code == 1) {
                return Ok(reply(ComposerPublication::PhoneRequired));
            }
        }
        let Ok(Some(device)) = api
            .local_device_id()
            .map(|id| id.filter(|device| !device.trim().is_empty() && device.len() <= 1024))
        else {
            self.session.check_generation(&token)?;
            return Ok(reply(ComposerPublication::PreflightUnavailable));
        };
        self.session.require_api(&token, &api)?;
        let uuid = fresh_uuid(&owner, &device)?;
        let mut record = Record {
            revision,
            uuid,
            phase: Phase::Pending,
        };
        let body = self.session.with_token(&token, |uid| {
            store.transaction(|tx| {
                if read_record(tx, uid)?
                    .is_some_and(|previous| matches!(previous.phase, Phase::Pending))
                {
                    return Err(Error::MutationBusy);
                }
                let draft = read_draft(tx, uid, &record.revision)?;
                let attachment = draft.attachment.as_ref().ok_or(Error::StaleResponse)?;
                let body = request(attachment, &draft.text, &record.uuid);
                tx.put(uid, JOURNAL, CURRENT, &serde_json::to_value(&record)?)?;
                Ok(body)
            })
        })?;
        let result = self
            .session
            .run_for(&token, |current, _| {
                let checked = if std::sync::Arc::ptr_eq(&api, &current) {
                    self.session.require_api(&token, &api)
                } else {
                    Err(Error::StaleResponse)
                };
                async move {
                    checked.map_err(|error| Error::RequestNotSent(Box::new(error)))?;
                    current.call(PUBLISH, body, false, &[200]).await
                }
            })
            .await;
        record.phase = match result {
            Ok(response) => {
                confirmed_event(&response, &owner, &record.uuid).map_or(Phase::Pending, |event| {
                    Phase::Confirmed {
                        event,
                        detail: Uuid::new_v4().to_string(),
                    }
                })
            }
            Err(error) => {
                rejection(&error).map_or(Phase::Pending, |reason| Phase::Rejected { reason })
            }
        };
        let value = self.settle_publication(&token, &store, record)?;
        Ok(reply(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_or_uuid_alone_does_not_turn_a_bad_acknowledgement_into_success() {
        let uuid = "4ec9f512024b09735df27529275b0a86";
        let valid = json!({"event":{"id":"42","user":{"userId":2},"uuid":uuid}});
        assert_eq!(confirmed_event(&valid, "2", uuid).as_deref(), Some("42"));
        for invalid in [
            json!({"event":{"uuid":uuid,"user":{"userId":2}}}),
            json!({"event":{"id":42,"user":{"userId":3},"uuid":uuid}}),
            json!({"event":{"id":42,"user":{"userId":2},"uuid":"other"}}),
            json!({"event":{"id":42,"user":{"userId":2},"uuid":uuid,"anonymityInfo":{}}}),
        ] {
            assert!(confirmed_event(&invalid, "2", uuid).is_none());
        }
    }

    #[test]
    fn uuid_matches_executed_official_module_vectors() {
        for (nonce, time, uid, device, expected) in [
            (
                "IIIIIIII",
                1_700_000_100_000,
                "2",
                "SYNTH-DEVICE-00",
                "4ec9f512024b09735df27529275b0a86",
            ),
            (
                "3LLLLLLL",
                1_700_000_100_017,
                "9007199254740997",
                "Synthetic 测试/device+A%*'",
                "75c8ab20e5c64a7a0096aed199b910d6",
            ),
            (
                "F4BIPX4B",
                1_700_000_100_029,
                "7",
                "SYNTH/(a)!_%",
                "aa7146e6cffbd0a6c8323216bc8afe1f",
            ),
        ] {
            assert_eq!(publication_uuid(nonce, time, uid, device), expected);
        }
    }
}
