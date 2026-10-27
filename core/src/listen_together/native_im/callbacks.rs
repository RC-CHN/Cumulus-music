//! SDK userdata is a never-reused integer cookie, not a dereferenceable Rust
//! allocation. Late callbacks after shutdown/account switch find no recipient.
use super::{ImEvent, ffi};
use crate::{Error, Result};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    ffi::{CStr, c_char, c_void},
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
};

pub(super) enum Received {
    Event(ImEvent),
    Grant { code: i32, data: String },
    Logout,
    Cleanup,
    Malformed,
}
struct Inbox {
    sender: SyncSender<Received>,
    overflow: AtomicBool,
}
static NEXT: AtomicUsize = AtomicUsize::new(1);
static RECIPIENTS: Mutex<BTreeMap<usize, Weak<Inbox>>> = Mutex::new(BTreeMap::new());
pub(super) struct Registration {
    cookie: usize,
    inbox: Arc<Inbox>,
    pub receiver: Receiver<Received>,
}
impl Registration {
    pub fn new() -> Result<Self> {
        let cookie = NEXT
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_add(1))
            .map_err(|_| Error::State("NIM callback generation exhausted"))?;
        let (sender, receiver) = mpsc::sync_channel(256);
        let inbox = Arc::new(Inbox {
            sender,
            overflow: AtomicBool::new(false),
        });
        RECIPIENTS
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .insert(cookie, Arc::downgrade(&inbox));
        Ok(Self {
            cookie,
            inbox,
            receiver,
        })
    }
    pub fn data(&self) -> *const c_void {
        self.cookie as *const c_void
    }
    pub fn check(&self) -> Result<()> {
        if self.inbox.overflow.load(Ordering::Acquire) {
            Err(Error::State(
                "NIM events were lost; reconnect and reconcile room state",
            ))
        } else {
            Ok(())
        }
    }
}
impl Drop for Registration {
    fn drop(&mut self) {
        if let Ok(mut recipients) = RECIPIENTS.lock() {
            recipients.remove(&self.cookie);
        }
    }
}
fn deliver(data: *const c_void, make: impl FnOnce() -> Received) {
    // No unwind crosses an extern C boundary, including malformed SDK JSON.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let inbox = RECIPIENTS
            .lock()
            .ok()
            .and_then(|r| r.get(&(data as usize)).and_then(Weak::upgrade));
        if let Some(inbox) = inbox {
            let event = catch_unwind(AssertUnwindSafe(make)).unwrap_or(Received::Malformed);
            if inbox.sender.try_send(event).is_err() {
                inbox.overflow.store(true, Ordering::Release);
            }
        }
    }));
}
unsafe fn string(value: *const c_char) -> Option<String> {
    if value.is_null() {
        return None;
    }
    // SAFETY: vendor callback contract owns a valid NUL-terminated UTF-8 string
    // until return. Copy it here; no SDK pointer escapes this callback.
    let bytes = unsafe { CStr::from_ptr(value) }.to_bytes();
    if bytes.len() > 1024 * 1024 {
        return None;
    }
    std::str::from_utf8(bytes).ok().map(str::to_owned)
}
unsafe fn json(value: *const c_char) -> Option<Value> {
    serde_json::from_str(&unsafe { string(value) }?).ok()
}
pub(super) unsafe extern "C" fn login(value: *const c_char, data: *const c_void) {
    deliver(data, || {
        let Some(v) = (unsafe { json(value) }) else {
            return Received::Malformed;
        };
        #[derive(Deserialize)]
        struct Reply {
            err_code: i32,
            login_step: i32,
            #[serde(default)]
            retrying: bool,
        }
        let Ok(reply) = serde_json::from_value::<Reply>(v) else {
            return Received::Malformed;
        };
        Received::Event(ImEvent::Login {
            code: reply.err_code,
            step: reply.login_step,
            retrying: reply.retrying,
        })
    });
}
pub(super) unsafe extern "C" fn kick(value: *const c_char, data: *const c_void) {
    deliver(data, || {
        let v = unsafe { json(value) }.unwrap_or_default();
        Received::Event(ImEvent::Kicked {
            reason: v["reason_code"].as_i64().unwrap_or(0) as i32,
        })
    });
}
pub(super) unsafe extern "C" fn disconnected(_: *const c_char, data: *const c_void) {
    deliver(data, || Received::Event(ImEvent::Disconnected));
}
pub(super) unsafe extern "C" fn grant(
    code: i32,
    result: *const c_char,
    _: *const c_char,
    data: *const c_void,
) {
    deliver(data, || Received::Grant {
        code,
        data: unsafe { string(result) }.unwrap_or_default(),
    });
}
pub(super) unsafe extern "C" fn enter(
    room: i64,
    step: i32,
    code: i32,
    _: *const c_char,
    _: *const c_char,
    data: *const c_void,
) {
    deliver(data, || {
        Received::Event(ImEvent::Enter { room, step, code })
    });
}
pub(super) unsafe extern "C" fn exit(
    room: i64,
    code: i32,
    reason: i32,
    _: *const c_char,
    data: *const c_void,
) {
    deliver(data, || {
        Received::Event(ImEvent::Exit { room, code, reason })
    });
}
pub(super) unsafe extern "C" fn link(
    room: i64,
    condition: i32,
    _: *const c_char,
    data: *const c_void,
) {
    deliver(data, || Received::Event(ImEvent::Link { room, condition }));
}
pub(super) unsafe extern "C" fn message(
    room: i64,
    value: *const c_char,
    _: *const c_char,
    data: *const c_void,
) {
    deliver(data, || match unsafe { json(value) } {
        Some(body) => Received::Event(ImEvent::Message { room, body }),
        None => Received::Malformed,
    });
}
pub(super) unsafe extern "C" fn notification(
    room: i64,
    value: *const c_char,
    _: *const c_char,
    data: *const c_void,
) {
    deliver(data, || match unsafe { json(value) } {
        Some(body) => Received::Event(ImEvent::Notification { room, body }),
        None => Received::Malformed,
    });
}
pub(super) unsafe extern "C" fn talk(value: *const c_char, _: *const c_char, data: *const c_void) {
    deliver(data, || {
        let messages = unsafe { json(value) }
            .map(|v| super::super::message::from_sdk(&v))
            .unwrap_or_default();
        Received::Event(ImEvent::Business { messages })
    });
}
pub(super) unsafe extern "C" fn logout(_: *const c_char, data: *const c_void) {
    deliver(data, || Received::Logout);
}
pub(super) unsafe extern "C" fn cleanup(_: *const c_char, data: *const c_void) {
    deliver(data, || Received::Cleanup);
}

// Compile-time checks against all independently declared C ABI signatures.
const _: ffi::JsonCb = login;
const _: ffi::GrantCb = grant;
const _: ffi::EnterCb = enter;
const _: ffi::ExitCb = exit;
const _: ffi::LinkCb = link;
const _: ffi::MessageCb = message;
const _: ffi::TalkCb = talk;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callbacks_are_copied_private_and_retired_without_dereferencing_userdata() {
        let first = Registration::new().unwrap();
        let data = first.data();
        unsafe {
            login(c"{\"err_code\":200,\"login_step\":3,\"other_clients_pres\":[{\"token\":\"private\"}]}".as_ptr(), data)
        };
        assert!(matches!(
            first.receiver.recv().unwrap(),
            Received::Event(ImEvent::Login {
                code: 200,
                step: 3,
                ..
            })
        ));
        drop(first);
        let second = Registration::new().unwrap();
        assert_ne!(data, second.data());
        // A late callback can even contain an unusable body pointer: lookup
        // rejects the retired generation before accessing that pointer.
        unsafe { login(std::ptr::dangling(), data) };
        assert!(second.receiver.try_recv().is_err());
        unsafe { login(c"not JSON".as_ptr(), second.data()) };
        assert!(matches!(
            second.receiver.recv().unwrap(),
            Received::Malformed
        ));
    }
    #[test]
    fn overload_is_explicit_and_callback_never_blocks() {
        let recipient = Registration::new().unwrap();
        for _ in 0..257 {
            unsafe { disconnected(std::ptr::null(), recipient.data()) };
        }
        assert!(recipient.check().is_err());
    }
    #[test]
    fn talk_callback_copies_batch_attachments_and_retired_generation_is_not_read() {
        let recipient = Registration::new().unwrap();
        let attachment=serde_json::json!({"content":{"type":20008,"content":{"roomId":"private-room","ignoreUserIds":[]}}}).to_string();
        let input=std::ffi::CString::new(serde_json::json!([{"rescode":200,"content":{"msg_attach":attachment,"msg_body":"PRIVATE"}}]).to_string()).unwrap();
        unsafe { talk(input.as_ptr(), std::ptr::null(), recipient.data()) };
        drop(input);
        let Received::Event(event) = recipient.receiver.recv().unwrap() else {
            panic!("event missing")
        };
        assert_eq!(format!("{event:?}"), "Business([redacted])");
        assert!(
            matches!(event,ImEvent::Business{messages} if messages.len()==1 && messages[0].belongs_to("private-room"))
        );
        let old = recipient.data();
        drop(recipient);
        unsafe { talk(std::ptr::dangling(), std::ptr::null(), old) };
    }
    #[test]
    fn payload_is_copied_and_debug_never_exposes_private_body() {
        let recipient = Registration::new().unwrap();
        let body = std::ffi::CString::new(r#"{"body":"PRIVATE","room_id":123}"#).unwrap();
        unsafe { message(123, body.as_ptr(), std::ptr::null(), recipient.data()) };
        drop(body);
        let Received::Event(event) = recipient.receiver.recv().unwrap() else {
            panic!("message missing")
        };
        assert_eq!(format!("{event:?}"), "Message([redacted])");
        assert!(matches!(event, ImEvent::Message { room:123, body } if body["body"]=="PRIVATE"));
        unsafe {
            login(
                c"{\"err_code\":200,\"login_step\":4294967299}".as_ptr(),
                recipient.data(),
            )
        };
        assert!(matches!(
            recipient.receiver.recv().unwrap(),
            Received::Malformed
        ));
    }
}
