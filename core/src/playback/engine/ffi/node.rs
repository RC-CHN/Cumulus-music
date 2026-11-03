//! The documented mpv_node ABI for commands and property replies.
use std::ffi::{c_char, c_int, c_void};

pub(super) const STRING: c_int = 1;
pub(super) const NODE: c_int = 6;
pub(super) const ARRAY: c_int = 7;
pub(super) const MAP: c_int = 8;

#[repr(C)]
pub(super) union Value {
    pub(super) string: *mut c_char,
    pub(super) flag: c_int,
    pub(super) int64: i64,
    pub(super) double_: f64,
    pub(super) list: *mut List,
    pub(super) bytes: *mut c_void,
}
#[repr(C)]
pub(super) struct Node {
    pub(super) value: Value,
    pub(super) format: c_int,
}
#[repr(C)]
pub(super) struct List {
    pub(super) num: c_int,
    pub(super) values: *mut Node,
    pub(super) keys: *mut *mut c_char,
}
