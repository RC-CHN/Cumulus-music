use gio::{
    DBusCallFlags,
    glib::{self, variant::ToVariant},
};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
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
struct Shared {
    stopped: AtomicBool,
    generation: AtomicU64,
    changed: Box<dyn Fn(bool) + Send + Sync>,
}
pub struct Monitor {
    unwatch: Option<Box<dyn FnOnce() + Send + Sync>>,
    shared: Arc<Shared>,
}
impl Drop for Monitor {
    fn drop(&mut self) {
        self.shared.stopped.store(true, Ordering::Release);
        if let Some(unwatch) = self.unwatch.take() {
            unwatch();
        }
    }
}
impl Shared {
    fn current(&self, generation: u64) -> bool {
        !self.stopped.load(Ordering::Acquire)
            && self.generation.load(Ordering::Acquire) == generation
    }
}
pub fn monitor(id: &str, changed: impl Fn(bool) + Send + Sync + 'static) -> Monitor {
    let shared = Arc::new(Shared {
        stopped: AtomicBool::new(false),
        generation: AtomicU64::new(0),
        changed: Box::new(changed),
    });
    let appeared = shared.clone();
    let vanished = shared.clone();
    let expected = format!("tray-icon tray app {id}");
    let watcher = gio::bus_watch_name(
        gio::BusType::Session,
        WATCHER,
        gio::BusNameWatcherFlags::NONE,
        move |connection, _, owner| {
            let generation = appeared.generation.fetch_add(1, Ordering::AcqRel) + 1;
            let shared = appeared.clone();
            let expected = expected.clone();
            let owner = owner.to_owned();
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
                    glib::timeout_future(std::time::Duration::from_secs(1)).await;
                }
            });
        },
        move |_, _| {
            vanished.generation.fetch_add(1, Ordering::AcqRel);
            if !vanished.stopped.load(Ordering::Acquire) {
                (vanished.changed)(false);
            }
        },
    );
    Monitor {
        unwatch: Some(Box::new(move || gio::bus_unwatch_name(watcher))),
        shared,
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
