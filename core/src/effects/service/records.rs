use super::super::{
    catalog::{EffectId, MaterialDescriptor, Sound},
    rights::Authorization,
};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

/// Native receipt, not a URL/path selected by the caller. The key is derived
/// again at every use and the account namespace comes only from SessionToken.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialHandle {
    pub effect: EffectId,
    pub sound_type: u32,
    pub key: String,
}
impl MaterialHandle {
    pub(super) fn new(effect: EffectId, sound_type: u32, descriptor: &MaterialDescriptor) -> Self {
        Self {
            effect,
            sound_type,
            key: hex::encode(Sha256::digest(format!(
                "{}:{sound_type}:{}:{}",
                effect.value(),
                descriptor.size(),
                descriptor.md5()
            ))),
        }
    }
    pub(super) fn lock_key(&self) -> String {
        format!("material:{}:{}", self.effect.value(), self.sound_type)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialPhase {
    Pending,
    Ready,
    Removing,
    Removed,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Record {
    pub handle: MaterialHandle,
    pub descriptor: MaterialDescriptor,
    pub attempt: String,
    pub abandoned: Vec<String>,
    pub phase: MaterialPhase,
    pub plain_sha256: Option<String>,
}
impl Record {
    pub fn validate(&self, handle: &MaterialHandle) -> Result<()> {
        if &self.handle != handle
            || MaterialHandle::new(handle.effect, handle.sound_type, &self.descriptor) != *handle
            || !valid_attempt(&self.attempt)
            || self.abandoned.len() > 64
            || self.abandoned.iter().any(|s| !valid_attempt(s))
        {
            return Err(Error::State("invalid stored effect material identity"));
        }
        Ok(())
    }
}
fn valid_attempt(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Only returned after fresh authorization, transfer verification and bounded
/// NCAE/JSON decoding. It is not an activation or an auth-occupancy receipt.
pub struct VerifiedMaterial {
    pub path: PathBuf,
    pub parameters: Value,
    pub type_id: u16,
    pub plain_sha256: String,
}
pub struct MaterialOutcome {
    pub handle: MaterialHandle,
    pub sound: Sound,
    pub authorization: Authorization,
    pub transfer_id: String,
    pub bytes: u64,
    pub material: Option<VerifiedMaterial>,
}
