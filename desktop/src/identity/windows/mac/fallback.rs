//! CEFD10 selection with validated operating-system inputs.

use super::{DriverReply, MacAddress, device_path};
use std::collections::BTreeSet;

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
#[path = "fallback/native.rs"]
mod native;
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
pub(crate) use native::collect_fallback_mac;

const ETHERNET: u32 = 6;
const NAME_GUARD: u16 = 0xa55a;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FallbackError {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    WinsockStartup,
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    HostName,
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    HostIpv4,
    AdapterList,
    AdapterIp,
    NameConversion,
    DeviceName,
    DriverReply,
    AddressLength,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FallbackSource {
    Ndis,
    AdapterRecord,
}

pub(crate) enum FallbackObservation {
    Available(FallbackSource, MacAddress),
    NoMatch,
    RejectedByOuterFilter,
}

struct Adapter<'a> {
    kind: u32,
    first_ip: &'a [i8],
    name: &'a [i8],
    address: &'a [u8],
    address_length: u32,
}

fn c_string(field: &[i8]) -> Option<Vec<u8>> {
    let end = field.iter().position(|&byte| byte == 0)?;
    Some(field[..=end].iter().map(|&byte| byte as u8).collect())
}

fn short_name_buffer(control: &[u16]) -> Vec<u16> {
    [NAME_GUARD]
        .into_iter()
        .chain(control.iter().map(|unit| !unit))
        .chain([NAME_GUARD])
        .collect()
}

fn verified_short_name<'a>(short: &'a [u16], control: &[u16]) -> Option<&'a [u16]> {
    let body = short.get(1..short.len().checked_sub(1)?)?;
    (short.first() == Some(&NAME_GUARD) && short.last() == Some(&NAME_GUARD) && body == control)
        .then_some(body)
}

fn select<'a>(
    host_first_ip: u32,
    adapters: impl IntoIterator<Item = Result<Adapter<'a>, FallbackError>>,
    parse_ip: impl Fn(&[u8]) -> Option<u32>,
    convert_name: impl Fn(&[u8]) -> Option<Vec<u16>>,
    query: impl Fn(&[u16]) -> DriverReply,
) -> Result<FallbackObservation, FallbackError> {
    for adapter in adapters {
        let adapter = adapter?;
        if adapter.kind != ETHERNET {
            continue;
        }
        let first_ip = c_string(adapter.first_ip).ok_or(FallbackError::AdapterIp)?;
        if parse_ip(&first_ip) != Some(host_first_ip) {
            continue;
        }
        let name = c_string(adapter.name).ok_or(FallbackError::NameConversion)?;
        let name = convert_name(&name).ok_or(FallbackError::NameConversion)?;
        let path = device_path(&name).ok_or(FallbackError::DeviceName)?;
        let (source, address) = match query(&path) {
            DriverReply::Bytes(address) => (FallbackSource::Ndis, address),
            DriverReply::Failed => {
                // The original unconditionally reads six bytes. Only admit
                // them when IP Helper attests those bytes as part of Address.
                let bytes = adapter
                    .address
                    .get(..adapter.address_length as usize)
                    .and_then(|bytes| bytes.first_chunk::<6>())
                    .ok_or(FallbackError::AddressLength)?;
                (FallbackSource::AdapterRecord, MacAddress(*bytes))
            }
            DriverReply::Malformed => return Err(FallbackError::DriverReply),
        };
        // Outer CEF5E0 rejects the first result. It does not retry another
        // adapter or try current bytes after a rejected permanent address.
        return Ok(if address.accepted() {
            FallbackObservation::Available(source, address)
        } else {
            FallbackObservation::RejectedByOuterFilter
        });
    }
    Ok(FallbackObservation::NoMatch)
}

struct Records {
    base: usize,
    bytes: usize,
    record_size: usize,
    alignment: usize,
    seen: BTreeSet<usize>,
}

impl Records {
    fn new(base: usize, bytes: usize, record_size: usize, alignment: usize) -> Self {
        Self {
            base,
            bytes,
            record_size,
            alignment,
            seen: BTreeSet::new(),
        }
    }

    fn next_offset(&mut self, address: usize) -> Result<usize, FallbackError> {
        let offset = address
            .checked_sub(self.base)
            .filter(|offset| {
                address.is_multiple_of(self.alignment)
                    && offset
                        .checked_add(self.record_size)
                        .is_some_and(|end| end <= self.bytes)
            })
            .ok_or(FallbackError::AdapterList)?;
        let overlaps_previous = self
            .seen
            .range(..=offset)
            .next_back()
            .is_some_and(|previous| offset - previous < self.record_size);
        let overlaps_next = self
            .seen
            .range(offset..)
            .next()
            .is_some_and(|next| next - offset < self.record_size);
        if overlaps_previous || overlaps_next {
            return Err(FallbackError::AdapterList);
        }
        self.seen.insert(offset);
        Ok(offset)
    }
}

#[cfg(test)]
#[path = "fallback/tests.rs"]
mod tests;
