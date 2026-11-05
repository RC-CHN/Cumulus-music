//! One atomic decoded audio-params node, before output-device/filter coercion.
use super::{
    Native,
    node::{MAP, NODE, Node},
};
use crate::{Error, Result};
use std::ffi::{CStr, c_char, c_int, c_void};
type Get = unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut c_void) -> c_int;
type Free = unsafe extern "C" fn(*mut Node);
const INT64: c_int = 4;

/// # Safety
/// root and every tagged list/key belong to a live libmpv-owned property reply.
/// Bounds validate shape, not arbitrary memory supplied by another party.
unsafe fn decode(root: &Node) -> Result<Option<(u32, u32)>> {
    let malformed = || Error::Media("invalid native decoded audio format");
    if root.format != MAP {
        return Err(malformed());
    }
    // SAFETY: MAP tag selects the native list union member.
    let list = unsafe { root.value.list.as_ref() }.ok_or_else(malformed)?;
    if list.num < 0
        || list.num > 64
        || (list.num > 0 && (list.keys.is_null() || list.values.is_null()))
    {
        return Err(malformed());
    }
    if list.num == 0 {
        return Ok(None);
    }
    // SAFETY: libmpv guarantees list.num entries and NUL-terminated keys.
    let (keys, values) = unsafe {
        (
            std::slice::from_raw_parts(list.keys, list.num as usize),
            std::slice::from_raw_parts(list.values, list.num as usize),
        )
    };
    let (mut rate, mut channels) = (None, None);
    for (&key, value) in keys.iter().zip(values) {
        if key.is_null() {
            return Err(malformed());
        }
        // SAFETY: a key from this same live mpv node.
        let name = unsafe { CStr::from_ptr(key) }.to_bytes();
        let field = match name {
            b"samplerate" => &mut rate,
            b"channel-count" => &mut channels,
            _ => continue,
        };
        if field.is_some() || value.format != INT64 {
            return Err(malformed());
        }
        // SAFETY: INT64 tag validates the union's scalar member.
        let number = unsafe { value.value.int64 };
        let Ok(number) = u32::try_from(number) else {
            return Err(malformed());
        };
        if number == 0 {
            return Ok(None);
        }
        *field = Some(number);
    }
    Ok(rate.zip(channels))
}
impl Native {
    pub fn decoded_audio_format(&mut self) -> Result<Option<(u32, u32)>> {
        // SAFETY: version-supported documented libmpv client ABI; the runtime
        // owns both copied functions throughout this synchronous property read.
        unsafe {
            let get = *self
                ._runtime
                .get::<Get>(b"mpv_get_property\0")
                .map_err(|_| Error::State("native format property API unavailable"))?;
            let free = *self
                ._runtime
                .get::<Free>(b"mpv_free_node_contents\0")
                .map_err(|_| Error::State("native format property API unavailable"))?;
            let mut node = Node {
                value: super::node::Value { int64: 0 },
                format: 0,
            };
            let result = get(
                self.handle.as_ptr(),
                c"audio-params".as_ptr(),
                NODE,
                (&mut node as *mut Node).cast(),
            );
            if result < 0 {
                return Ok(None);
            }
            let decoded = decode(&node);
            // This synchronous node is caller-owned, unlike borrowed events.
            // Always free a successful reply, including malformed shape.
            free(&mut node);
            decoded
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::node::{List, Value};
    use super::*;

    fn decode_fixture(
        keys: &mut [*mut std::ffi::c_char; 2],
        values: &mut [Node; 2],
    ) -> Result<Option<(u32, u32)>> {
        let mut list = List {
            num: 2,
            values: values.as_mut_ptr(),
            keys: keys.as_mut_ptr(),
        };
        let node = Node {
            format: MAP,
            value: Value { list: &mut list },
        };
        // SAFETY: complete live synthetic tagged-node storage for this call.
        unsafe { decode(&node) }
    }

    #[test]
    fn decoded_map_requires_actual_integer_fields_and_preserves_mono() {
        let mut keys = [
            c"samplerate".as_ptr().cast_mut(),
            c"channel-count".as_ptr().cast_mut(),
        ];
        let mut values = [
            Node {
                format: INT64,
                value: Value { int64: 48000 },
            },
            Node {
                format: INT64,
                value: Value { int64: 1 },
            },
        ];
        assert_eq!(
            decode_fixture(&mut keys, &mut values).unwrap(),
            Some((48000, 1))
        );
        values[1].value = Value { int64: 2 };
        assert_eq!(
            decode_fixture(&mut keys, &mut values).unwrap(),
            Some((48000, 2))
        );
        values[1].format = 1;
        assert!(decode_fixture(&mut keys, &mut values).is_err());
        values[1].format = INT64;
        keys[1] = keys[0];
        assert!(decode_fixture(&mut keys, &mut values).is_err());
    }
}
