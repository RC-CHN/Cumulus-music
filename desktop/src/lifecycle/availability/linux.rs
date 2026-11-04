use gio::{
    DBusCallFlags,
    glib::{
        self,
        translate::{IntoGlib, ToGlibPtr},
        variant::ToVariant,
    },
};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const PATH: &str = "/StatusNotifierWatcher";
const ITEM: &str = "org.kde.StatusNotifierItem";
type Properties = HashMap<String, glib::Variant>;

// libappindicator-sys panics if neither library is present. Preflight the same
// fixed SONAMEs before constructing a native tray and retain the handle.
pub fn library() -> Option<libloading::Library> {
    ["libayatana-appindicator3.so.1", "libappindicator3.so.1"]
        .into_iter()
        .find_map(|name| {
            // SAFETY: fixed platform libraries with the same GTK ABI as Tauri.
            // This performs no symbol calls and the handle is retained by the host.
            unsafe { libloading::Library::new(name).ok() }
        })
}

pub async fn session_bus() -> Option<gio::DBusConnection> {
    let connection = glib::future_with_timeout(
        Duration::from_secs(2),
        gio::bus_get_future(gio::BusType::Session),
    )
    .await
    .ok()?
    .ok()?;
    if connection.is_closed() {
        return None;
    }
    // Keep a shared GIO/GTK session-bus disconnect from terminating the app.
    connection.set_exit_on_close(false);
    Some(connection)
}

struct Shared {
    stopped: AtomicBool,
    generation: AtomicU64,
    changed: Box<dyn Fn(bool) + Send + Sync>,
}
pub struct Monitor {
    watcher: u32,
    shared: Arc<Shared>,
    _connection: gio::DBusConnection,
}
impl Drop for Monitor {
    fn drop(&mut self) {
        self.shared.stopped.store(true, Ordering::Release);
        if self.watcher != 0 {
            // SAFETY: this is the ID of the one watch owned by this monitor.
            unsafe { gio::ffi::g_bus_unwatch_name(self.watcher) };
        }
    }
}
impl Shared {
    fn current(&self, generation: u64) -> bool {
        !self.stopped.load(Ordering::Acquire)
            && self.generation.load(Ordering::Acquire) == generation
    }

    fn unavailable(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        if !self.stopped.load(Ordering::Acquire) {
            (self.changed)(false);
        }
    }
}

pub fn monitor(
    connection: gio::DBusConnection,
    id: &str,
    changed: impl Fn(bool) + Send + Sync + 'static,
) -> Monitor {
    let shared = Arc::new(Shared {
        stopped: AtomicBool::new(false),
        generation: AtomicU64::new(0),
        changed: Box::new(changed),
    });
    let appeared = shared.clone();
    let vanished = shared.clone();
    let expected = format!("tray-icon tray app {id}");
    let appeared = glib::Closure::new(move |args| {
        let connection = args
            .first()
            .and_then(|arg| arg.get::<gio::DBusConnection>().ok());
        let owner = args.get(2).and_then(|arg| arg.get::<&str>().ok());
        if let (Some(connection), Some(owner)) = (connection, owner) {
            let generation = appeared.generation.fetch_add(1, Ordering::AcqRel) + 1;
            let shared = appeared.clone();
            let expected = expected.clone();
            let owner = owner.to_owned();
            if !shared.current(generation) {
                return None;
            }
            (shared.changed)(false);
            glib::MainContext::default().spawn_local(async move {
                // Bounded, sequential reads avoid signal storms and also detect
                // hosts that omit PropertiesChanged or disappear without a signal.
                while shared.current(generation) {
                    let available = registered(&connection, &owner, &expected).await;
                    if !shared.current(generation) {
                        break;
                    }
                    (shared.changed)(available);
                    glib::timeout_future(Duration::from_secs(1)).await;
                }
            });
        } else {
            appeared.unavailable();
        }
        None
    });
    // GIO supplies a null connection if the bus closes. gio 0.18's watcher
    // wrapper unwraps it inside the C callback; the vanished case needs no
    // connection at all.
    let vanished = glib::Closure::new(move |_| {
        vanished.unavailable();
        None
    });
    // SAFETY: GIO retains the two closure references for this watch. The
    // closures accept the documented GValues without assuming non-null data.
    let watcher = unsafe {
        gio::ffi::g_bus_watch_name_on_connection_with_closures(
            connection.to_glib_none().0,
            WATCHER.to_glib_none().0,
            gio::BusNameWatcherFlags::NONE.into_glib(),
            appeared.to_glib_none().0,
            vanished.to_glib_none().0,
        )
    };
    Monitor {
        watcher,
        shared,
        _connection: connection,
    }
}
async fn properties(
    connection: &gio::DBusConnection,
    owner: &str,
    path: &str,
    interface: &str,
) -> Option<Properties> {
    connection
        .call_future(
            Some(owner),
            path,
            "org.freedesktop.DBus.Properties",
            "GetAll",
            Some(&(interface,).to_variant()),
            None,
            DBusCallFlags::NONE,
            500,
        )
        .await
        .ok()?
        .get::<(Properties,)>()
        .map(|p| p.0)
}
async fn registered(connection: &gio::DBusConnection, owner: &str, expected: &str) -> bool {
    let Some(props) = properties(connection, owner, PATH, WATCHER).await else {
        return false;
    };
    if props
        .get("IsStatusNotifierHostRegistered")
        .and_then(|p| p.get::<bool>())
        != Some(true)
    {
        return false;
    }
    let Some(own) = connection.unique_name() else {
        return false;
    };
    let Some(items) = props
        .get("RegisteredStatusNotifierItems")
        .and_then(|p| p.get::<Vec<String>>())
    else {
        return false;
    };
    // libappindicator and gio use the shared session bus connection. Restrict
    // reads to our own registered objects, never arbitrary other services.
    for path in items
        .iter()
        .filter_map(|item| item.strip_prefix(own.as_str()))
        .filter(|path| path.starts_with('/'))
        .take(4)
    {
        let Some(item) = properties(connection, own.as_str(), path, ITEM).await else {
            continue;
        };
        if item.get("Id").and_then(|p| p.str()) == Some(expected)
            && item.get("Status").and_then(|p| p.str()) == Some("Active")
            && item
                .get("Menu")
                .and_then(|p| p.str())
                .is_some_and(|path| path.starts_with('/') && path != "/")
        {
            return true;
        }
    }
    false
}
