//! A fresh query independent of Tauri/GTK's cached window-state flag.
use tauri::Runtime;

pub fn applied<R: Runtime>(window: &tauri::WebviewWindow<R>) -> Option<bool> {
    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::*;
        use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};
        let surface = window.gtk_window().ok()?.window()?;
        let x11 = surface.downcast::<gdkx11::X11Window>().ok()?;
        let xid = u32::try_from(x11.xid()).ok()?;
        let (connection, _) =
            x11rb::connect(Some(&x11.upcast_ref::<gtk::gdk::Window>().display().name())).ok()?;
        let state = connection
            .intern_atom(false, b"_NET_WM_STATE")
            .ok()?
            .reply()
            .ok()?
            .atom;
        let above = connection
            .intern_atom(false, b"_NET_WM_STATE_ABOVE")
            .ok()?
            .reply()
            .ok()?
            .atom;
        let reply = connection
            .get_property(false, xid, state, AtomEnum::ATOM, 0, 128)
            .ok()?
            .reply()
            .ok()?;
        if reply.type_ == u32::from(AtomEnum::NONE) {
            return Some(false);
        }
        reply.value32().map(|mut values| values.any(|v| v == above))
    }
    #[cfg(not(target_os = "linux"))]
    window.is_always_on_top().ok()
}
