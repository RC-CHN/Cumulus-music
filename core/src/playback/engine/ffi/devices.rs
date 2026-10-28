//! Read only the documented audio-device-list node shape. These are borrowed
//! mpv event allocations; no pointer or borrowed string escapes this module.
use crate::{Error, Result, playback::engine::OutputDevice};
use std::{
    collections::HashSet,
    ffi::{CStr, c_char, c_int, c_void},
    slice,
};

const STRING: c_int = 1;
const NODE: c_int = 6;
const ARRAY: c_int = 7;
const MAP: c_int = 8;

#[cfg(test)]
#[path = "devices_tests.rs"]
mod tests;

#[repr(C)]
struct Property {
    name: *const c_char,
    format: c_int,
    data: *mut c_void,
}
#[repr(C)]
union Value {
    string: *mut c_char,
    flag: c_int,
    int64: i64,
    double_: f64,
    list: *mut List,
    bytes: *mut c_void,
}
#[repr(C)]
struct Node {
    value: Value,
    format: c_int,
}
#[repr(C)]
struct List {
    num: c_int,
    values: *mut Node,
    keys: *mut *mut c_char,
}
fn malformed() -> Error {
    Error::Media("invalid native audio device list")
}

/// # Safety
/// `data` must be the live MPV_EVENT_GET_PROPERTY_REPLY payload from libmpv.
/// All tagged pointers follow the pinned mpv client ABI and remain live until
/// the next wait call. Shape checks bound copies, not arbitrary invalid memory.
pub(super) unsafe fn copy(data: *mut c_void) -> Result<Vec<OutputDevice>> {
    // SAFETY: event payload validity is required by the caller. Check the tag
    // before interpreting its data; unsupported formats are never dereferenced.
    let property = unsafe { data.cast::<Property>().as_ref() }.ok_or_else(malformed)?;
    if property.format != NODE {
        return Err(malformed());
    }
    let root = unsafe { property.data.cast::<Node>().as_ref() }.ok_or_else(malformed)?;
    // SAFETY: root is the borrowed node belonging to this same live event.
    unsafe { devices(root) }
}

unsafe fn list(node: &Node, format: c_int, limit: usize) -> Result<&List> {
    if node.format != format {
        return Err(malformed());
    }
    // SAFETY: the validated tag selects list; pointers come from libmpv.
    let list = unsafe { node.value.list.as_ref() }.ok_or_else(malformed)?;
    if list.num < 0 || list.num as usize > limit || (list.num > 0 && list.values.is_null()) {
        return Err(malformed());
    }
    Ok(list)
}
unsafe fn string(pointer: *const c_char) -> Result<String> {
    if pointer.is_null() {
        return Err(malformed());
    }
    // SAFETY: libmpv strings are NUL terminated and valid for this event.
    let bytes = unsafe { CStr::from_ptr(pointer) }.to_bytes();
    if bytes.len() > 16384 {
        return Err(malformed());
    }
    Ok(std::str::from_utf8(bytes).map_err(|_| malformed())?.into())
}
unsafe fn devices(root: &Node) -> Result<Vec<OutputDevice>> {
    // SAFETY: each list/union is accessed only under its expected tag. libmpv
    // supplies arrays of `num` entries; zero-length NULL arrays aren't sliced.
    unsafe {
        let items = list(root, ARRAY, 1024)?;
        if items.num == 0 {
            return Ok(vec![]);
        }
        let mut result = Vec::with_capacity(items.num as usize);
        let mut ids = HashSet::new();
        for item in slice::from_raw_parts(items.values, items.num as usize) {
            let fields = list(item, MAP, 64)?;
            if fields.num == 0 || fields.keys.is_null() {
                return Err(malformed());
            }
            let values = slice::from_raw_parts(fields.values, fields.num as usize);
            let keys = slice::from_raw_parts(fields.keys, fields.num as usize);
            let (mut id, mut description) = (None, None);
            for (key, value) in keys.iter().zip(values) {
                let target = match string(*key)?.as_str() {
                    "name" => &mut id,
                    "description" => &mut description,
                    // New mpv fields are ignored without touching their union.
                    _ => continue,
                };
                if target.is_some() || value.format != STRING {
                    return Err(malformed());
                }
                *target = Some(string(value.value.string)?);
            }
            let id = id.ok_or_else(malformed)?;
            if !super::super::output::valid_id(&id) || !ids.insert(id.clone()) {
                return Err(malformed());
            }
            result.push(OutputDevice {
                id,
                description: description.ok_or_else(malformed)?,
            });
        }
        Ok(result)
    }
}
