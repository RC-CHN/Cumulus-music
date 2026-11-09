//! Recovered Windows MAC observations, used only by the explicit probe.
//! No observation constructs an identity or writes state.

#[cfg(any(test, all(target_os = "windows", target_arch = "x86_64")))]
#[path = "mac/fallback.rs"]
mod fallback;

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
#[path = "mac/native.rs"]
mod native;
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
pub(crate) use fallback::{
    FallbackError, FallbackObservation, FallbackSource, collect_fallback_mac,
};
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
pub(crate) use native::collect_registry_mac;

const MAX_WIDE: usize = 260;
const NDIS_OUTPUT: usize = 260;
const MAX_CARDS: u32 = 4096;

// Deliberately no Debug, Display, or serialization for an actual hardware fact.
pub(crate) struct MacAddress([u8; 6]);

impl MacAddress {
    pub(crate) fn official_ascii(&self) -> [u8; 17] {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        let mut text = [b':'; 17];
        for (index, &byte) in self.0.iter().enumerate() {
            text[index * 3] = HEX[usize::from(byte >> 4)];
            text[index * 3 + 1] = HEX[usize::from(byte & 15)];
        }
        text
    }

    fn accepted(&self) -> bool {
        let text = self.official_ascii();
        const ZERO_PREFIX: &[u8] = b"00:00:00:00:00:";
        text != *b"FF:FF:FF:FF:FF:FF"
            && !text
                .windows(ZERO_PREFIX.len())
                .any(|part| part == ZERO_PREFIX)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fallback {
    NoRegistryCandidate,
    RejectedByOuterFilter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Inconclusive {
    EnumerationLimit,
    RegistryKey,
    Description,
    DeviceName,
    DriverReply,
}

pub(crate) enum Observation {
    RegistryNdis(MacAddress),
    FallbackRequired(Fallback),
    Inconclusive(Inconclusive),
}

enum RegistryEntry<K> {
    Open(K),
    Skip,
    End,
    Malformed,
}

enum ServiceName {
    Unreadable,
    Value(Vec<u16>),
    Malformed,
}

enum DriverReply {
    Failed,
    Bytes(MacAddress),
    Malformed,
}

trait NetworkCards {
    type Key;

    fn entry(&self, index: u32) -> RegistryEntry<Self::Key>;
    // The adapter supplies the real CRT's lowercased units. Empty means the
    // Description value was absent, as the original zeroed query buffer is.
    fn lower_description(&self, key: &Self::Key) -> Option<Vec<u16>>;
    fn service_name(&self, key: &Self::Key) -> ServiceName;
    fn permanent_address(&self, path: &[u16]) -> DriverReply;
}

fn scan(cards: &impl NetworkCards) -> Observation {
    for index in 0..MAX_CARDS {
        let key = match cards.entry(index) {
            RegistryEntry::Open(key) => key,
            RegistryEntry::Skip => continue,
            RegistryEntry::End => {
                return Observation::FallbackRequired(Fallback::NoRegistryCandidate);
            }
            RegistryEntry::Malformed => {
                return Observation::Inconclusive(Inconclusive::RegistryKey);
            }
        };
        let Some(description) = cards.lower_description(&key) else {
            return Observation::Inconclusive(Inconclusive::Description);
        };
        if excluded_description(&description) {
            continue;
        }
        let service = match cards.service_name(&key) {
            ServiceName::Unreadable => continue,
            ServiceName::Value(service) => service,
            ServiceName::Malformed => return Observation::Inconclusive(Inconclusive::DeviceName),
        };
        let Some(path) = device_path(&service) else {
            return Observation::Inconclusive(Inconclusive::DeviceName);
        };
        match cards.permanent_address(&path) {
            DriverReply::Failed => continue,
            DriverReply::Malformed => return Observation::Inconclusive(Inconclusive::DriverReply),
            // CF0130 returns the first successful response. CEF5E0 then
            // filters it; a rejected value must never select the next key.
            DriverReply::Bytes(mac) if mac.accepted() => return Observation::RegistryNdis(mac),
            DriverReply::Bytes(_) => {
                return Observation::FallbackRequired(Fallback::RejectedByOuterFilter);
            }
        }
    }
    Observation::Inconclusive(Inconclusive::EnumerationLimit)
}

fn excluded_description(lower: &[u16]) -> bool {
    [b"virtual".as_slice(), b"1394"].into_iter().any(|needle| {
        lower.windows(needle.len()).any(|word| {
            word.iter()
                .copied()
                .eq(needle.iter().copied().map(u16::from))
        })
    })
}

fn device_path(name: &[u16]) -> Option<Vec<u16>> {
    // Only a leaf from the fixed HKLM key can reach this function. Reject
    // path/stream syntax and trailing dot or space normalization.
    if name.is_empty()
        || name.len() + 5 > MAX_WIDE
        || name.iter().any(|&unit| {
            unit < 32 || [u16::from(b'\\'), u16::from(b'/'), u16::from(b':')].contains(&unit)
        })
        || name
            .last()
            .is_some_and(|&unit| unit == u16::from(b'.') || unit == u16::from(b' '))
    {
        return None;
    }
    Some(
        r"\\.\"
            .encode_utf16()
            .chain(name.iter().copied())
            .chain([0])
            .collect(),
    )
}

fn registry_units(buffer: &[u16; MAX_WIDE], bytes: u32, require_nul: bool) -> Option<&[u16]> {
    let bytes = usize::try_from(bytes).ok()?;
    if !bytes.is_multiple_of(2) {
        return None;
    }
    let value = buffer.get(..bytes / 2)?;
    match value.iter().position(|&unit| unit == 0) {
        Some(end) => Some(&value[..end]),
        // Only the Description array was zero-filled in CF0130. A returned
        // prefix can rely on that terminator if it fits within the array.
        None if !require_nul && value.len() < buffer.len() => Some(value),
        None => None,
    }
}

fn driver_reply(success: bool, reported: u32, data: &[u8; NDIS_OUTPUT]) -> DriverReply {
    if !success {
        return DriverReply::Failed;
    }
    if !(6..=NDIS_OUTPUT as u32).contains(&reported) {
        // CEFA40 would format a short, zero-filled success anyway. Do not
        // treat a padded value as a real fact or scan past the selected key.
        return DriverReply::Malformed;
    }
    DriverReply::Bytes(MacAddress(data[..6].try_into().expect("exact six bytes")))
}

#[cfg(test)]
#[path = "mac/tests.rs"]
mod tests;
