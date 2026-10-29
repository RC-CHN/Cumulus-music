//! RegisterHotKey(NULL) binds to this worker's message queue. Every Win32 call,
//! including cleanup, executes on the same owning thread.
use super::Failure;
use cumulus_core::preferences::shortcuts::Chord;
use std::{
    collections::{BTreeMap, BTreeSet},
    ptr::null_mut,
};
use windows_sys::Win32::{
    Foundation::{ERROR_HOTKEY_ALREADY_REGISTERED, GetLastError},
    UI::{
        Input::KeyboardAndMouse::{
            GetAsyncKeyState, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey,
            UnregisterHotKey,
        },
        WindowsAndMessaging::{MSG, PM_NOREMOVE, PM_REMOVE, PeekMessageW, WM_HOTKEY},
    },
};
pub(super) struct Backend {
    ids: BTreeSet<u32>,
    held_at_registration: BTreeMap<u32, u16>,
}
impl Backend {
    pub fn open() -> Result<Self, Failure> {
        // SAFETY: stack-owned output; PeekMessage creates this thread's queue.
        unsafe {
            let mut message: MSG = std::mem::zeroed();
            PeekMessageW(&mut message, null_mut(), 0, 0, PM_NOREMOVE);
        }
        Ok(Self {
            ids: BTreeSet::new(),
            held_at_registration: BTreeMap::new(),
        })
    }
    pub fn register(&mut self, id: u32, chord: &Chord) -> Result<(), Failure> {
        let mut modifiers = MOD_NOREPEAT;
        let mut key = None;
        for &value in chord.keys() {
            match value {
                16 => modifiers |= MOD_SHIFT,
                17 => modifiers |= MOD_CONTROL,
                18 => modifiers |= MOD_ALT,
                value if key.is_none() => key = Some(value),
                _ => return Err(Failure::Unsupported),
            }
        }
        let key = key.ok_or(Failure::Unsupported)?;
        // SAFETY: no borrowed pointers; NULL selects this worker thread. The id
        // is bounded by the caller and is not reused during its lifetime.
        if unsafe { RegisterHotKey(null_mut(), id as i32, modifiers, u32::from(key)) } == 0 {
            // SAFETY: read immediately after the failed call on this thread.
            return Err(
                if unsafe { GetLastError() } == ERROR_HOTKEY_ALREADY_REGISTERED {
                    Failure::Conflict
                } else {
                    Failure::Unavailable
                },
            );
        }
        self.ids.insert(id);
        // SAFETY: scalar key-state query on this interactive desktop. Only the
        // high bit means held; the low 'pressed since query' bit is not a state.
        if unsafe { GetAsyncKeyState(i32::from(key)) } < 0 {
            self.held_at_registration.insert(id, key);
        }
        Ok(())
    }
    pub fn clear(&mut self) -> Result<(), Failure> {
        self.held_at_registration.clear();
        let mut failed = false;
        self.ids.retain(|&id| {
            // SAFETY: registration belongs to this thread and contains no HWND.
            let remains = unsafe { UnregisterHotKey(null_mut(), id as i32) } == 0;
            failed |= remains;
            remains
        });
        if failed { Err(Failure::Fatal) } else { Ok(()) }
    }
    pub fn poll(&mut self) -> Result<Vec<u32>, Failure> {
        let mut pressed = vec![];
        // SAFETY: owned output and this thread's messages only. MOD_NOREPEAT
        // supplies press edges without a release-poll thread per keystroke.
        unsafe {
            let mut message: MSG = std::mem::zeroed();
            for _ in 0..512 {
                if PeekMessageW(&mut message, null_mut(), WM_HOTKEY, WM_HOTKEY, PM_REMOVE) == 0 {
                    break;
                }
                if let Ok(id) = u32::try_from(message.wParam)
                    && self.ids.contains(&id)
                    && !self.held_at_registration.contains_key(&id)
                {
                    pressed.push(id);
                }
            }
            self.held_at_registration
                .retain(|_, key| GetAsyncKeyState(i32::from(*key)) < 0);
        }
        Ok(pressed)
    }
}
impl Drop for Backend {
    fn drop(&mut self) {
        let _ = self.clear();
    }
}
