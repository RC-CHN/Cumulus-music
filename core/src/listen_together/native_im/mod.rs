//! Native-only NIM transport. Blocking SDK lifecycle operations belong on a
//! host worker, never a UI/async executor thread. No sender/invitation API is
//! loaded. REST reports remain the source of outgoing together commands.
mod callbacks;
mod ffi;

use super::{ImCredentials, Room};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
};
use callbacks::{Received, Registration};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    cell::Cell,
    ffi::CString,
    fmt,
    marker::PhantomData,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::RecvTimeoutError,
    },
    time::{Duration, Instant},
};

/// Explicit trusted native SDK paths and secrets, never an IPC DTO. Hosts must
/// persist database_key privately; losing it requires replacing the SDK cache.
pub struct NimConfig {
    pub library_directory: PathBuf,
    pub data_root: PathBuf,
    pub app_key: String,
    pub database_key: String,
}

/// Native-only events. Raw chatroom messages are deliberately not serializable
/// and Debug hides payloads/identities. The business reducer must whitelist data
/// before it reaches a host/frontend DTO.
pub enum ImEvent {
    Login {
        step: i32,
        code: i32,
        retrying: bool,
    },
    Grant {
        code: i32,
    },
    Enter {
        room: i64,
        step: i32,
        code: i32,
    },
    Exit {
        room: i64,
        code: i32,
        reason: i32,
    },
    Link {
        room: i64,
        condition: i32,
    },
    Message {
        room: i64,
        body: Value,
    },
    Notification {
        room: i64,
        body: Value,
    },
    Business {
        messages: Vec<super::RoomMessage>,
    },
    Kicked {
        reason: i32,
    },
    Disconnected,
}
impl fmt::Debug for ImEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Login {
                step,
                code,
                retrying,
            } => f
                .debug_struct("Login")
                .field("step", step)
                .field("code", code)
                .field("retrying", retrying)
                .finish(),
            Self::Grant { code } => f.debug_struct("Grant").field("code", code).finish(),
            Self::Enter { step, code, .. } => f
                .debug_struct("Enter")
                .field("step", step)
                .field("code", code)
                .finish(),
            Self::Exit { code, reason, .. } => f
                .debug_struct("Exit")
                .field("code", code)
                .field("reason", reason)
                .finish(),
            Self::Link { condition, .. } => f
                .debug_struct("Link")
                .field("condition", condition)
                .finish(),
            Self::Kicked { reason } => f.debug_struct("Kicked").field("reason", reason).finish(),
            Self::Message { .. } => f.write_str("Message([redacted])"),
            Self::Notification { .. } => f.write_str("Notification([redacted])"),
            Self::Business { .. } => f.write_str("Business([redacted])"),
            Self::Disconnected => f.write_str("Disconnected"),
        }
    }
}

static BUSY: AtomicBool = AtomicBool::new(false);
struct Lease;
impl Lease {
    fn acquire() -> Result<Self> {
        BUSY.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| Error::State("NIM SDK already active or cleanup unresolved"))?;
        Ok(Self)
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::Release);
    }
}

pub struct NativeIm {
    native: Arc<ffi::Bindings>,
    callbacks: Registration,
    lease: Option<Lease>,
    session: Arc<AccountSession>,
    token: SessionToken,
    logged_in: bool,
    room: Option<i64>,
    entering: bool,
    initialized: bool,
    cleanup_failed: bool,
    // Login parameters are retained until cleanup; do not rely on the SDK
    // copying async request inputs before returning.
    _arguments: Vec<CString>,
    _not_sync: PhantomData<Cell<()>>,
}

fn cstring(value: &str) -> Result<CString> {
    CString::new(value).map_err(|_| Error::InvalidInput("NUL in NIM argument"))
}
fn is_current(session: &AccountSession, token: &SessionToken) -> Result<()> {
    session.with_token(token, |_| Ok(()))
}

impl NativeIm {
    /// Starts asynchronous SDK login for exactly this account generation.
    /// Consume Login(step=3,code=200) before requesting a chatroom grant.
    pub fn open(
        config: NimConfig,
        session: Arc<AccountSession>,
        token: SessionToken,
        credentials: ImCredentials,
    ) -> Result<Self> {
        is_current(&session, &token)?;
        if token.uid.as_deref() != Some(&credentials.uid) {
            return Err(Error::StaleResponse);
        }
        if config.app_key.len() != 32
            || !config.app_key.bytes().all(|b| b.is_ascii_hexdigit())
            || config.database_key.len() != 32
            || credentials.account_id.is_empty()
            || credentials.token.is_empty()
        {
            return Err(Error::InvalidInput("invalid NIM configuration/credentials"));
        }
        let lease = Lease::acquire()?;
        let native = ffi::Bindings::load(&config.library_directory)?;
        let directory = config
            .data_root
            .join(hex::encode(Sha256::digest(credentials.uid.as_bytes())));
        std::fs::create_dir_all(&directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
        }
        let directory = directory.canonicalize()?;
        let directory = directory
            .to_str()
            .ok_or(Error::InvalidInput("NIM requires a UTF-8 data path"))?;
        let arguments = vec![
            cstring(&format!("{directory}{}", std::path::MAIN_SEPARATOR))?,
            cstring(
                &json!({"app_key":config.app_key,"global_config":{
                    "db_encrypt_key":config.database_key,"encrypt_unencrypted_db":true,
                    "encrypt_unencrypted_db_threshold":0,"preload_attach":false,
                "sync_session_ack":false,"upload_sdk_events_after_login":false,"sdk_log_level":2
                }})
                .to_string(),
            )?,
            cstring(&config.app_key)?,
            cstring(&credentials.account_id)?,
            cstring(&credentials.token)?,
        ];
        let callbacks = Registration::new()?;
        // SAFETY: valid C strings are retained in self; callbacks use only a
        // registry cookie and have exact vendor signatures. Lease serializes
        // all process-global initialization/cleanup, including across accounts.
        unsafe {
            if !(native.init)(arguments[0].as_ptr(), c"".as_ptr(), arguments[1].as_ptr()) {
                return Err(Error::State("NIM initialization failed"));
            }
        }
        let adapter = Self {
            native,
            callbacks,
            lease: Some(lease),
            session,
            token,
            logged_in: false,
            room: None,
            entering: false,
            initialized: true,
            cleanup_failed: false,
            _arguments: arguments,
            _not_sync: PhantomData,
        };
        // Initialize the companion module even if the account changes before
        // login; Drop then always has a matching chatroom cleanup to perform.
        unsafe {
            (adapter.native.room_init)(c"{}".as_ptr());
        }
        adapter.session.with_token(&adapter.token, |_| unsafe {
            let n = &adapter.native;
            let data = adapter.callbacks.data();
            (n.relogin_cb)(c"{}".as_ptr(), callbacks::login, data);
            (n.kickout_cb)(c"{}".as_ptr(), callbacks::kick, data);
            (n.disconnected_cb)(c"{}".as_ptr(), callbacks::disconnected, data);
            (n.talk_cb)(c"{}".as_ptr(), callbacks::talk, data);
            (n.talks_cb)(c"{}".as_ptr(), callbacks::talk, data);
            (n.enter_cb)(c"{}".as_ptr(), callbacks::enter, data);
            (n.exit_cb)(c"{}".as_ptr(), callbacks::exit, data);
            (n.link_cb)(c"{}".as_ptr(), callbacks::link, data);
            (n.message_cb)(c"{}".as_ptr(), callbacks::message, data);
            (n.notifications_cb)(c"{}".as_ptr(), callbacks::notification, data);
            (n.login)(
                adapter._arguments[2].as_ptr(),
                adapter._arguments[3].as_ptr(),
                adapter._arguments[4].as_ptr(),
                c"{}".as_ptr(),
                callbacks::login,
                data,
            );
            Ok(())
        })?;
        Ok(adapter)
    }

    /// One room per adapter lifetime. Recreate the adapter after an expired
    /// grant/room change; this prevents late grant replies entering a new room.
    pub fn enter(&mut self, room: &Room) -> Result<()> {
        is_current(&self.session, &self.token)?;
        if !self.initialized || !self.logged_in || self.room.is_some() {
            return Err(Error::State("NIM is not ready for a new room"));
        }
        room.role_for(self.token.uid.as_deref().ok_or(Error::StaleResponse)?)?;
        let id = room
            .chat_room_id
            .parse::<i64>()
            .ok()
            .filter(|id| *id > 0)
            .ok_or(Error::InvalidInput(
                "NIM chatroom ID exceeds signed 64-bit range",
            ))?;
        self.room = Some(id);
        self.entering = true;
        // SAFETY: id validated, extension/static callback outlive request.
        self.session.with_token(&self.token, |_| unsafe {
            (self.native.grant)(id, c"{}".as_ptr(), callbacks::grant, self.callbacks.data());
            Ok(())
        })?;
        Ok(())
    }

    /// Poll on the native worker. A stale account never receives an old event.
    pub fn next_event(&mut self, timeout: Duration) -> Result<Option<ImEvent>> {
        is_current(&self.session, &self.token)?;
        if !self.initialized {
            return Err(Error::State("NIM transport is closed"));
        }
        self.callbacks.check()?;
        let received = match self.callbacks.receiver.recv_timeout(timeout) {
            Ok(value) => value,
            Err(RecvTimeoutError::Timeout) => return Ok(None),
            Err(RecvTimeoutError::Disconnected) => {
                return Err(Error::State("NIM callbacks closed"));
            }
        };
        is_current(&self.session, &self.token)?;
        match received {
            Received::Event(event) => {
                match &event {
                    ImEvent::Login { code, step, .. } if *step == 3 => {
                        self.logged_in = *code == 200
                    }
                    ImEvent::Kicked { .. } | ImEvent::Disconnected => self.logged_in = false,
                    _ => {}
                }
                Ok(Some(event))
            }
            Received::Grant { code, data } => {
                if !self.entering {
                    return Err(Error::State("unexpected NIM grant callback"));
                }
                self.entering = false;
                if code == 200 {
                    let data = cstring(&data)?;
                    // SAFETY: the SDK grant is passed verbatim only to this
                    // adapter's originally requested room, with retained input.
                    let accepted = self.session.with_token(&self.token, |_| unsafe {
                        Ok((self.native.enter)(
                            self.room.ok_or(Error::State("NIM room missing"))?,
                            data.as_ptr(),
                            c"{}".as_ptr(),
                            c"{}".as_ptr(),
                        ))
                    })?;
                    self._arguments.push(data);
                    if !accepted {
                        return Err(Error::Response("NIM rejected room entry arguments"));
                    }
                }
                Ok(Some(ImEvent::Grant { code }))
            }
            Received::Malformed => Err(Error::Response("invalid NIM callback payload")),
            _ => Err(Error::State("unexpected NIM lifecycle callback")),
        }
    }

    /// Ends this SDK connection only; does not end the REST together room.
    /// A cleanup timeout poisons process-global reuse rather than risking two
    /// accounts sharing a still-running SDK singleton.
    pub fn close(&mut self) -> Result<()> {
        if !self.initialized {
            return if self.cleanup_failed {
                Err(Error::State(
                    "NIM cleanup timed out; restart before another account",
                ))
            } else {
                Ok(())
            };
        }
        self.initialized = false;
        // SAFETY: live library/context; close may run after account invalidation
        // specifically to release the old account's transport.
        unsafe {
            if let Some(room) = self.room {
                (self.native.exit)(room, c"{}".as_ptr());
            }
            (self.native.logout)(3, c"{}".as_ptr(), callbacks::logout, self.callbacks.data());
        }
        let logout_deadline = Instant::now() + Duration::from_secs(3);
        while let Some(wait) = logout_deadline.checked_duration_since(Instant::now()) {
            if matches!(
                self.callbacks.receiver.recv_timeout(wait),
                Ok(Received::Logout) | Err(_)
            ) {
                break;
            }
        }
        unsafe {
            (self.native.room_cleanup)(c"{}".as_ptr());
            (self.native.cleanup)(callbacks::cleanup, c"{}".as_ptr(), self.callbacks.data());
        }
        let cleanup_deadline = Instant::now() + Duration::from_secs(8);
        while let Some(wait) = cleanup_deadline.checked_duration_since(Instant::now()) {
            match self.callbacks.receiver.recv_timeout(wait) {
                Ok(Received::Cleanup) => {
                    self.lease.take();
                    return Ok(());
                }
                Err(_) => break,
                _ => {}
            }
        }
        if let Some(lease) = self.lease.take() {
            std::mem::forget(lease);
        }
        // Completion was not acknowledged. The SDK might still borrow async
        // inputs; retain them for this (now non-reusable) process lifetime.
        std::mem::forget(std::mem::take(&mut self._arguments));
        self.cleanup_failed = true;
        Err(Error::State(
            "NIM cleanup timed out; restart before another account",
        ))
    }
}
impl Drop for NativeIm {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
