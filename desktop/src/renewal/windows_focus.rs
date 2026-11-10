//! WM_ACTIVATE observes the top-level HWND. WebView2 child keyboard focus
//! can change without the app losing or regaining activation.

#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    WA_ACTIVE, WA_CLICKACTIVE, WA_INACTIVE, WM_ACTIVATE,
};
// Same Win32 values for the portable message decoder test.
#[cfg(not(windows))]
use portable::{WA_ACTIVE, WA_CLICKACTIVE, WA_INACTIVE, WM_ACTIVATE};
#[cfg(not(windows))]
mod portable {
    pub(super) const WM_ACTIVATE: u32 = 6;
    pub(super) const WA_INACTIVE: u32 = 0;
    pub(super) const WA_ACTIVE: u32 = 1;
    pub(super) const WA_CLICKACTIVE: u32 = 2;
}

fn activation(message: u32, word: usize) -> Option<bool> {
    if message != WM_ACTIVATE {
        return None;
    }
    let minimized = word & 0xffff_0000 != 0;
    match (word & 0xffff) as u32 {
        WA_INACTIVE => Some(false),
        WA_ACTIVE | WA_CLICKACTIVE => Some(!minimized),
        _ => None,
    }
}

#[cfg(windows)]
pub(super) use native::install;

#[cfg(windows)]
mod native {
    use std::{
        cell::RefCell,
        collections::{HashMap, hash_map::Entry},
        io,
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };
    use tauri::{AppHandle, Manager, Runtime};
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::Threading::GetCurrentThreadId,
        UI::{
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{GetWindowThreadProcessId, WM_NCDESTROY},
        },
    };

    // Callback address + ID identify a subclass; the state never leaves the
    // owning window thread or crosses the ABI as an application pointer.
    const ID: usize = 1;
    type Handler = Rc<dyn Fn(bool)>;
    thread_local! {
        static HANDLERS: RefCell<HashMap<usize, Handler>> = RefCell::default();
    }

    pub(in crate::renewal) fn install<R: Runtime>(app: &AppHandle<R>) -> io::Result<()> {
        let window = app
            .get_webview_window("main")
            .ok_or_else(|| io::Error::other("missing main window"))?;
        let hwnd = window
            .hwnd()
            .map_err(|_| io::Error::other("missing main HWND"))?
            .0;
        // SAFETY: read the thread owning this live Tauri HWND before changing
        // its subclass chain. The two queries do not retain a pointer.
        if hwnd.is_null()
            || unsafe {
                GetWindowThreadProcessId(hwnd, std::ptr::null_mut()) != GetCurrentThreadId()
            }
        {
            return Err(io::Error::other("main HWND is not on the setup thread"));
        }
        let app = app.clone();
        let key = hwnd as usize;
        let inserted = HANDLERS.with(|handlers| {
            if let Entry::Vacant(slot) = handlers.borrow_mut().entry(key) {
                slot.insert(Rc::new(move |focused| {
                    super::super::focus_changed(&app, focused);
                }));
                true
            } else {
                false
            }
        });
        if !inserted {
            return Ok(());
        }
        // SAFETY: install only on the owning thread; the callback never
        // interprets arbitrary reference data. Failed installation releases
        // the owned handler; WM_NCDESTROY releases it on success.
        if unsafe { SetWindowSubclass(hwnd, Some(subclass), ID, 0) } == 0 {
            HANDLERS.with(|handlers| handlers.borrow_mut().remove(&key));
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    unsafe extern "system" fn subclass(
        hwnd: HWND,
        message: u32,
        word: WPARAM,
        data: LPARAM,
        id: usize,
        _reference: usize,
    ) -> LRESULT {
        let key = hwnd as usize;
        if message == WM_NCDESTROY {
            // SAFETY: this callback owns this exact subclass on this thread.
            unsafe { RemoveWindowSubclass(hwnd, Some(subclass), id) };
            let _ = catch_unwind(AssertUnwindSafe(|| {
                let _ = HANDLERS.try_with(|handlers| handlers.borrow_mut().remove(&key));
            }));
        } else if let Some(focused) = super::activation(message, word) {
            let _ = catch_unwind(AssertUnwindSafe(|| {
                let handler = HANDLERS
                    .try_with(|handlers| handlers.borrow().get(&key).cloned())
                    .ok()
                    .flatten();
                if let Some(handler) = handler {
                    handler(focused);
                }
            }));
        }
        // SAFETY: always pass the original arguments to the rest of the
        // native subclass chain, including during final window destruction.
        unsafe { DefSubclassProc(hwnd, message, word, data) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activation_ignores_webview_child_keyboard_focus_messages() {
        assert_eq!(activation(WM_ACTIVATE, WA_ACTIVE as usize), Some(true));
        assert_eq!(activation(WM_ACTIVATE, WA_CLICKACTIVE as usize), Some(true));
        assert_eq!(activation(WM_ACTIVATE, WA_INACTIVE as usize), Some(false));
        assert_eq!(
            activation(WM_ACTIVATE, (WA_ACTIVE as usize) | (1 << 16)),
            Some(false)
        );
        assert_eq!(activation(0x0007, 0), None); // WM_SETFOCUS
        assert_eq!(activation(0x0008, 0), None); // WM_KILLFOCUS
    }
}
