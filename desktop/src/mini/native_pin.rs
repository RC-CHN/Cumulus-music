//! Read only this owned native window. GTK's cached ABOVE flag can remain
//! unchanged after KWin has already updated the X11 property.
use super::*;

pub(super) enum Handle {
    Native(Option<bool>),
    #[cfg(target_os = "linux")]
    X11 {
        display: String,
        window: u32,
    },
}

pub(super) async fn capture<R: Runtime>(
    app: &tauri::AppHandle<R>,
    label: String,
) -> Result<Handle> {
    gui(app, move |app| {
        let window = app.get_webview_window(&label).ok_or(Error::StaleResponse)?;
        #[cfg(target_os = "linux")]
        {
            use gtk::prelude::*;
            if let Some(surface) = window.gtk_window().ok().and_then(|w| w.window())
                && let Ok(x11) = surface.downcast::<gdkx11::X11Window>()
            {
                return Ok(Handle::X11 {
                    display: x11
                        .upcast_ref::<gtk::gdk::Window>()
                        .display()
                        .name()
                        .to_string(),
                    window: u32::try_from(x11.xid())
                        .map_err(|_| Error::State("invalid mini X11 window"))?,
                });
            }
        }
        Ok(Handle::Native(window.is_always_on_top().ok()))
    })
    .await
}

#[derive(Default)]
pub(super) struct Reader {
    #[cfg(target_os = "linux")]
    x11: Mutex<Option<x11::Connection>>,
}
impl Reader {
    pub(super) fn read(&self, handle: Handle) -> Option<bool> {
        match handle {
            Handle::Native(value) => value,
            #[cfg(target_os = "linux")]
            Handle::X11 { display, window } => {
                let mut slot = self.x11.lock().ok()?;
                if slot.as_ref().is_none_or(|c| c.display != display) {
                    *slot = x11::Connection::open(display).ok();
                }
                let result = slot.as_ref()?.read(window);
                if result.is_err() {
                    *slot = None;
                }
                result.ok().flatten()
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod x11 {
    use x11rb::{
        connection::Connection as _,
        protocol::xproto::{AtomEnum, ConnectionExt as _},
        rust_connection::RustConnection,
    };
    type Failure = Box<dyn std::error::Error + Send + Sync>;
    pub(super) struct Connection {
        pub display: String,
        connection: RustConnection,
        root: u32,
        state: u32,
        above: u32,
        manager: u32,
    }
    impl Connection {
        pub fn open(display: String) -> Result<Self, Failure> {
            let (connection, screen) = x11rb::connect(Some(&display))?;
            let root = connection.setup().roots[screen].root;
            let atom = |name| -> Result<_, Failure> {
                Ok(connection.intern_atom(false, name)?.reply()?.atom)
            };
            let state = atom(b"_NET_WM_STATE")?;
            let above = atom(b"_NET_WM_STATE_ABOVE")?;
            let manager = atom(b"_NET_SUPPORTING_WM_CHECK")?;
            Ok(Self {
                display,
                connection,
                root,
                state,
                above,
                manager,
            })
        }
        pub fn read(&self, window: u32) -> Result<Option<bool>, Failure> {
            let manager = self
                .connection
                .get_property(false, self.root, self.manager, AtomEnum::WINDOW, 0, 1)?
                .reply()?;
            if manager
                .value32()
                .and_then(|mut v| v.next())
                .is_none_or(|v| v == 0)
            {
                return Ok(None); // A bare Xvfb server cannot confirm window policy.
            }
            let reply = self
                .connection
                .get_property(false, window, self.state, AtomEnum::ATOM, 0, 128)?
                .reply()?;
            if reply.type_ == u32::from(AtomEnum::NONE) {
                return Ok(Some(false));
            }
            if reply.type_ != u32::from(AtomEnum::ATOM)
                || reply.format != 32
                || reply.bytes_after != 0
            {
                return Ok(None);
            }
            Ok(reply
                .value32()
                .map(|mut atoms| atoms.any(|a| a == self.above)))
        }
    }
}
