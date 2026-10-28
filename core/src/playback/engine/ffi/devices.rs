//! Decode the fixed expand-text command's raw audio-device-list JSON. The
//! command reply avoids the bundled Windows libmpv async-property destructor
//! crash, independently reproduced without Rust parsing (see audio-output docs).
use crate::{Error, Result, playback::engine::OutputDevice};
use serde::{Deserialize, de};
use std::{
    collections::HashSet,
    ffi::{CStr, c_char, c_int, c_void},
    fmt,
};

const STRING: c_int = 1;
const MAX_JSON: usize = 32 * 1024 * 1024;

#[cfg(test)]
#[path = "devices_tests.rs"]
mod tests;

#[repr(C)]
union Value {
    string: *const c_char,
    // All other union members are opaque. Preserve mpv_node's x86_64 layout.
    opaque: u64,
}
#[repr(C)]
struct Node {
    value: Value,
    format: c_int,
}
#[repr(C)]
struct Command {
    result: Node,
}
fn malformed() -> Error {
    Error::Media("invalid native audio device list")
}

/// # Safety
/// `data` must be the live MPV_EVENT_COMMAND_REPLY payload for our fixed
/// expand-text command. libmpv owns the result until the next wait call.
pub(super) unsafe fn copy(data: *mut c_void) -> Result<Vec<OutputDevice>> {
    // SAFETY: event provenance is required by the caller. Check the tag before
    // touching the union. Copy into Rust ownership before another wait call.
    let command = unsafe { data.cast::<Command>().as_ref() }.ok_or_else(malformed)?;
    if command.result.format != STRING {
        return Err(malformed());
    }
    let pointer = unsafe { command.result.value.string };
    if pointer.is_null() {
        return Err(malformed());
    }
    // SAFETY: libmpv supplies a live, NUL-terminated string for this event.
    let bytes = unsafe { CStr::from_ptr(pointer) }.to_bytes();
    parse(bytes)
}

#[derive(Deserialize)]
struct Device {
    #[serde(rename = "name")]
    id: String,
    description: String,
}
struct Devices(Vec<Device>);
impl<'de> Deserialize<'de> for Devices {
    fn deserialize<D: de::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct Bounded;
        impl<'de> de::Visitor<'de> for Bounded {
            type Value = Devices;
            fn expecting(&self, out: &mut fmt::Formatter) -> fmt::Result {
                out.write_str("at most 1024 audio devices")
            }
            fn visit_seq<A: de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Devices, A::Error> {
                let mut result = Vec::new();
                while let Some(device) = seq.next_element()? {
                    if result.len() == 1024 {
                        return Err(de::Error::custom("too many audio devices"));
                    }
                    result.push(device);
                }
                Ok(Devices(result))
            }
        }
        deserializer.deserialize_seq(Bounded)
    }
}
fn parse(bytes: &[u8]) -> Result<Vec<OutputDevice>> {
    if bytes.len() > MAX_JSON {
        return Err(malformed());
    }
    let Devices(rows) = serde_json::from_slice(bytes).map_err(|_| malformed())?;
    let mut ids = HashSet::new();
    rows.into_iter()
        .map(|row| {
            if !super::super::output::valid_id(&row.id)
                || !ids.insert(row.id.clone())
                || row.description.len() > 16384
                || row.description.contains('\0')
            {
                return Err(malformed());
            }
            Ok(OutputDevice {
                id: row.id,
                description: row.description,
            })
        })
        .collect()
}
