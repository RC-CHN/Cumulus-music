//! A registry-wide fence for a forced phone registration with an unknown result.
use super::*;
use crate::storage::StateTransaction;
use hmac::{Hmac, Mac};
use sha2::Sha256;
const REGISTRATION: &str = "phone-registration-v1";
const SEED: &str = "phone-registration-seed-v1";
#[derive(Clone)]
pub(super) struct Binding {
    tag: String,
    observed: Option<String>,
}
pub(crate) struct Intent {
    tag: String,
    id: String,
}
impl LoginCandidate {
    pub(crate) fn bind_registration_phone(&self, phone: Option<(&str, &str)>) -> Result<()> {
        let tag = phone
            .map(|(number, country)| {
                let seed = self.registry.store.transaction(|tx| {
                    if let Some(seed) = tx.get(OWNER, SEED, "seed")? {
                        return seed
                            .as_str()
                            .map(str::to_owned)
                            .ok_or(Error::Response("invalid registration seed"));
                    }
                    let seed = hex::encode(crate::protocol::wire::crypto::random_bytes::<32>()?);
                    tx.put(OWNER, SEED, "seed", &json!(seed))?;
                    Ok(seed)
                })?;
                let key =
                    hex::decode(seed).map_err(|_| Error::Response("invalid registration seed"))?;
                if key.len() != 32 {
                    return Err(Error::Response("invalid registration seed"));
                }
                let mut hmac = Hmac::<Sha256>::new_from_slice(&key)
                    .map_err(|_| Error::State("invalid registration key"))?;
                hmac.update(country.as_bytes());
                hmac.update(b":");
                hmac.update(number.as_bytes());
                let tag = hex::encode(hmac.finalize().into_bytes());
                let observed = self
                    .registry
                    .store
                    .get(OWNER, REGISTRATION, &tag)?
                    .and_then(|v| v["id"].as_str().map(str::to_owned));
                Ok(Binding { tag, observed })
            })
            .transpose()?;
        *self
            .registration_phone
            .lock()
            .map_err(|_| Error::LockPoisoned)? = tag;
        Ok(())
    }
    pub(crate) fn registration_pending(&self) -> Result<bool> {
        let tag = self
            .registration_phone
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .clone();
        match tag {
            Some(binding) => Ok(self
                .registry
                .store
                .get(OWNER, REGISTRATION, &binding.tag)?
                .is_some()),
            None => Ok(false),
        }
    }
    pub(crate) fn registration_begin(&self) -> Result<Intent> {
        let tag = self
            .registration_phone
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .clone()
            .ok_or(Error::State("fresh SMS login is required for registration"))?
            .tag;
        let id = Uuid::new_v4().to_string();
        self.registry.store.transaction(|tx| {
            if tx.get(OWNER, REGISTRATION, &tag)?.is_some() {
                return Err(Error::State("registration result is still unresolved"));
            }
            tx.put(
                OWNER,
                REGISTRATION,
                &tag,
                &json!({"id":id,"candidate":self.id}),
            )
        })?;
        Ok(Intent { tag, id })
    }
    pub(crate) fn registration_not_sent(&self, intent: &Intent) -> Result<()> {
        self.registry.store.transaction(|tx| {
            let value = tx
                .get(OWNER, REGISTRATION, &intent.tag)?
                .ok_or(Error::StaleResponse)?;
            if value["id"] != intent.id || value["candidate"] != self.id {
                return Err(Error::StaleResponse);
            }
            tx.delete(OWNER, REGISTRATION, &intent.tag)
        })
    }
}
pub(super) fn references_candidate(tx: &StateTransaction<'_>, id: &str) -> Result<bool> {
    for value in tx.values(OWNER, REGISTRATION)?.into_values() {
        match value["candidate"].as_str() {
            Some(candidate)
                if super::id(candidate)
                    && candidate != id
                    && value["id"]
                        .as_str()
                        .is_some_and(|intent| Uuid::parse_str(intent).is_ok()) => {}
            _ => return Ok(true),
        }
    }
    Ok(false)
}

pub(super) fn settle(
    tx: &mut StateTransaction<'_>,
    binding: Option<&Binding>,
    candidate: &str,
) -> Result<()> {
    if let Some(binding) = binding
        && let Some(intent) = tx.get(OWNER, REGISTRATION, &binding.tag)?
    {
        // A login already in flight before another candidate registered must
        // not erase that newer write's unresolved result.
        if intent["candidate"] == candidate || intent["id"].as_str() == binding.observed.as_deref()
        {
            tx.delete(OWNER, REGISTRATION, &binding.tag)?;
        }
    }
    Ok(())
}
