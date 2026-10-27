//! NIM 10.9.75 C ABI. `callbacks` owns the complementary borrowed-string boundary.
//! Libraries are retained for the process lifetime: cleanup completion does not
//! constitute a documented guarantee that no SDK thread can execute again.
use crate::{Error, Result};
use libloading::Library;
use std::{
    ffi::{c_char, c_int, c_void},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

pub(super) type JsonCb = unsafe extern "C" fn(*const c_char, *const c_void);
pub(super) type GrantCb = unsafe extern "C" fn(c_int, *const c_char, *const c_char, *const c_void);
pub(super) type EnterCb =
    unsafe extern "C" fn(i64, c_int, c_int, *const c_char, *const c_char, *const c_void);
pub(super) type ExitCb = unsafe extern "C" fn(i64, c_int, c_int, *const c_char, *const c_void);
pub(super) type LinkCb = unsafe extern "C" fn(i64, c_int, *const c_char, *const c_void);
pub(super) type MessageCb = unsafe extern "C" fn(i64, *const c_char, *const c_char, *const c_void);
pub(super) type TalkCb = unsafe extern "C" fn(*const c_char, *const c_char, *const c_void);

pub(super) struct Bindings {
    _libraries: [Library; 2],
    pub init: unsafe extern "C" fn(*const c_char, *const c_char, *const c_char) -> bool,
    pub cleanup: unsafe extern "C" fn(JsonCb, *const c_char, *const c_void),
    pub login: unsafe extern "C" fn(
        *const c_char,
        *const c_char,
        *const c_char,
        *const c_char,
        JsonCb,
        *const c_void,
    ),
    pub logout: unsafe extern "C" fn(c_int, *const c_char, JsonCb, *const c_void),
    pub relogin_cb: unsafe extern "C" fn(*const c_char, JsonCb, *const c_void),
    pub kickout_cb: unsafe extern "C" fn(*const c_char, JsonCb, *const c_void),
    pub disconnected_cb: unsafe extern "C" fn(*const c_char, JsonCb, *const c_void),
    pub talk_cb: unsafe extern "C" fn(*const c_char, TalkCb, *const c_void),
    pub talks_cb: unsafe extern "C" fn(*const c_char, TalkCb, *const c_void),
    pub grant: unsafe extern "C" fn(i64, *const c_char, GrantCb, *const c_void),
    pub room_init: unsafe extern "C" fn(*const c_char),
    pub room_cleanup: unsafe extern "C" fn(*const c_char),
    pub enter: unsafe extern "C" fn(i64, *const c_char, *const c_char, *const c_char) -> bool,
    pub exit: unsafe extern "C" fn(i64, *const c_char),
    pub enter_cb: unsafe extern "C" fn(*const c_char, EnterCb, *const c_void),
    pub exit_cb: unsafe extern "C" fn(*const c_char, ExitCb, *const c_void),
    pub link_cb: unsafe extern "C" fn(*const c_char, LinkCb, *const c_void),
    pub message_cb: unsafe extern "C" fn(*const c_char, MessageCb, *const c_void),
    pub notifications_cb: unsafe extern "C" fn(*const c_char, MessageCb, *const c_void),
}

type Loaded = Option<(PathBuf, Arc<Bindings>)>;
static LOADED: Mutex<Loaded> = Mutex::new(None);

impl Bindings {
    pub fn load(directory: &Path) -> Result<Arc<Self>> {
        let directory = directory.canonicalize()?;
        let mut loaded = LOADED.lock().map_err(|_| Error::LockPoisoned)?;
        if let Some((path, native)) = &*loaded {
            return if path == &directory {
                Ok(native.clone())
            } else {
                Err(Error::State(
                    "cannot replace a loaded NIM SDK in this process",
                ))
            };
        }
        let (im, room) = if cfg!(target_os = "windows") {
            ("nim.dll", "nim_chatroom.dll")
        } else if cfg!(target_os = "macos") {
            ("libnim.dylib", "libnim_chatroom.dylib")
        } else {
            ("libnim.so", "libnim_chatroom.so")
        };
        // SAFETY: the native host selects this explicit trusted SDK directory,
        // never a page-provided path. All symbols below match the vendor headers.
        // Both libraries are held in LOADED for the rest of the process.
        let native = unsafe {
            let im = Library::new(directory.join(im))
                .map_err(|_| Error::State("NIM library unavailable"))?;
            let room = Library::new(directory.join(room))
                .map_err(|_| Error::State("NIM chatroom library unavailable"))?;
            macro_rules! symbol {
                ($library:ident, $name:literal) => {
                    *$library
                        .get(concat!($name, "\0").as_bytes())
                        .map_err(|_| Error::State("NIM C ABI unavailable"))?
                };
            }
            Self {
                init: symbol!(im, "nim_client_init"),
                cleanup: symbol!(im, "nim_client_cleanup2"),
                login: symbol!(im, "nim_client_login"),
                logout: symbol!(im, "nim_client_logout"),
                relogin_cb: symbol!(im, "nim_client_reg_auto_relogin_cb"),
                kickout_cb: symbol!(im, "nim_client_reg_kickout_cb"),
                disconnected_cb: symbol!(im, "nim_client_reg_disconnect_cb"),
                talk_cb: symbol!(im, "nim_talk_reg_receive_cb"),
                talks_cb: symbol!(im, "nim_talk_reg_receive_msgs_cb"),
                grant: symbol!(im, "nim_plugin_chatroom_request_enter_async"),
                room_init: symbol!(room, "nim_chatroom_init"),
                room_cleanup: symbol!(room, "nim_chatroom_cleanup"),
                enter: symbol!(room, "nim_chatroom_enter"),
                exit: symbol!(room, "nim_chatroom_exit"),
                enter_cb: symbol!(room, "nim_chatroom_reg_enter_cb"),
                exit_cb: symbol!(room, "nim_chatroom_reg_exit_cb"),
                link_cb: symbol!(room, "nim_chatroom_reg_link_condition_cb"),
                message_cb: symbol!(room, "nim_chatroom_reg_receive_msg_cb"),
                notifications_cb: symbol!(room, "nim_chatroom_reg_receive_notification_cb"),
                _libraries: [im, room],
            }
        };
        let native = Arc::new(native);
        *loaded = Some((directory, native.clone()));
        Ok(native)
    }
}
