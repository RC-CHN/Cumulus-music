use crate::{Error, Result};
use serde::{Deserialize, Serialize};

/// Canonical legacy virtual-key numbers describe logical keys, not an OS hook.
/// Native adapters must translate them and report unsupported keys explicitly.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Chord(Vec<u16>);
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KeyInput {
    pub key_code: u16,
    pub ctrl_key: bool,
    pub shift_key: bool,
    pub alt_key: bool,
    pub meta_key: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "keys", rename_all = "snake_case")]
pub enum Capture {
    Ignore,
    Clear,
    Set(Chord),
}
const RESERVED: &[&[u16]] = &[
    &[116],
    &[32],
    &[76],
    &[69],
    &[17, 56],
    &[17, 83],
    &[17, 70],
    &[17, 67],
    &[17, 86],
    &[17, 88],
];
fn key_allowed(key: u16) -> bool {
    matches!(key, 32..=40 | 48..=57 | 65..=90 | 96..=107 | 109..=122 | 186..=192 | 219..=222)
}
impl Chord {
    pub fn keys(&self) -> &[u16] {
        &self.0
    }
    /// Canonical storage prevents duplicate or differently ordered collisions.
    pub fn new(mut keys: Vec<u16>) -> Result<Self> {
        if keys.is_empty() || keys.len() > 4 {
            return Err(Error::InvalidInput("invalid shortcut chord"));
        }
        keys.sort_unstable();
        let original_len = keys.len();
        keys.dedup();
        if keys.len() != original_len
            || keys.iter().filter(|k| !matches!(k, 16..=18)).count() != 1
            || keys
                .iter()
                .any(|k| !matches!(k, 16..=18) && !key_allowed(*k))
        {
            return Err(Error::InvalidInput("invalid shortcut chord"));
        }
        Ok(Self(keys))
    }
    pub(super) fn fixed(keys: &[u16]) -> Self {
        let mut keys = keys.to_vec();
        keys.sort_unstable();
        Self(keys)
    }
    pub(super) fn validate_custom(&self) -> Result<()> {
        if Self::new(self.0.clone())? != *self || RESERVED.contains(&self.0.as_slice()) {
            return Err(Error::InvalidInput("reserved or invalid shortcut chord"));
        }
        // Official local dispatch overrides Alt+F4 regardless of edited rows.
        // Reject the misleading custom assignment rather than displaying it as active.
        if self.0 == [18, 115] {
            return Err(Error::InvalidInput("reserved window close shortcut"));
        }
        Ok(())
    }
}
impl KeyInput {
    /// Recovered M3097 X: global unmodified keys acquire Ctrl+Alt; Backspace
    /// clears; Enter, Clear and the official reserved list leave settings alone.
    pub fn capture(self, global: bool) -> Capture {
        // Windows source silently drops Meta from the chord. Do not turn a
        // Super-key gesture into an unmodified global grab on another desktop.
        if self.meta_key || self.key_code == 13 {
            return Capture::Ignore;
        }
        if self.key_code == 8 {
            return Capture::Clear;
        }
        if !key_allowed(self.key_code) {
            return Capture::Ignore;
        }
        let mut keys = vec![self.key_code];
        if self.ctrl_key {
            keys.push(17);
        }
        if self.shift_key {
            keys.push(16);
        }
        if self.alt_key {
            keys.push(18);
        }
        if global && !self.ctrl_key && !self.shift_key && !self.alt_key {
            keys.extend([17, 18]);
        }
        match Chord::new(keys) {
            Ok(chord) if chord.validate_custom().is_ok() => Capture::Set(chord),
            _ => Capture::Ignore,
        }
    }
}
