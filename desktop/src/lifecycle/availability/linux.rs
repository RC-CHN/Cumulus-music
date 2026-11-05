use gio::{
    DBusCallFlags,
    glib::{
        self,
        translate::{IntoGlib, ToGlibPtr},
        variant::ToVariant,
    },
};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const PATH: &str = "/StatusNotifierWatcher";
const ITEM: &str = "org.kde.StatusNotifierItem";
const MENU: &str = "com.canonical.dbusmenu";
const AYATANA_ITEMS: &str = "/org/ayatana/NotificationItem";
const MAX_RECOVERY_OWNERS: usize = 16;
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
    recovery_owners: Mutex<HashSet<String>>,
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

    fn may_recover(&self, owner: &str) -> bool {
        self.recovery_owners.lock().is_ok_and(|attempted| {
            attempted.len() < MAX_RECOVERY_OWNERS && !attempted.contains(owner)
        })
    }

    fn begin_recovery(&self, owner: &str) -> bool {
        self.recovery_owners.lock().is_ok_and(|mut attempted| {
            attempted.len() < MAX_RECOVERY_OWNERS && attempted.insert(owner.into())
        })
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
        recovery_owners: Mutex::new(HashSet::new()),
    });
    let appeared = shared.clone();
    let vanished = shared.clone();
    let expected = format!("tray-icon tray app {id}");
    let recovery_path = format!(
        "{AYATANA_ITEMS}/{}",
        expected
            .bytes()
            .map(|b| if b.is_ascii_alphanumeric() {
                char::from(b)
            } else {
                '_'
            })
            .collect::<String>()
    );
    let appeared = glib::Closure::new(move |args| {
        let connection = args
            .first()
            .and_then(|arg| arg.get::<gio::DBusConnection>().ok());
        let owner = args.get(2).and_then(|arg| arg.get::<&str>().ok());
        if let (Some(connection), Some(owner)) = (connection, owner) {
            let generation = appeared.generation.fetch_add(1, Ordering::AcqRel) + 1;
            let shared = appeared.clone();
            let expected = expected.clone();
            let recovery_path = recovery_path.clone();
            let owner = owner.to_owned();
            if !shared.current(generation) {
                return None;
            }
            (shared.changed)(false);
            glib::MainContext::default().spawn_local(async move {
                // Bounded, sequential reads avoid signal storms and also detect
                // hosts that omit PropertiesChanged or disappear without a signal.
                let mut first = true;
                while shared.current(generation) {
                    let mut available = false;
                    if let Some(state) = watcher_state(&connection, &owner).await {
                        available = registered(&connection, &expected, &state).await;
                        if !first
                            && !available
                            && shared.current(generation)
                            && shared.may_recover(&owner)
                            && recover_registration(
                                &connection,
                                &owner,
                                &expected,
                                &recovery_path,
                                &state,
                                &shared,
                                generation,
                            )
                            .await
                            && let Some(confirmed) = watcher_state(&connection, &owner).await
                        {
                            available = registered(&connection, &expected, &confirmed).await;
                        }
                    }
                    first = false;
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
struct WatcherState {
    host: bool,
    items: Vec<String>,
}

impl WatcherState {
    fn own_paths<'a>(&'a self, own: &'a str) -> impl Iterator<Item = &'a str> {
        self.items
            .iter()
            .filter_map(move |item| item.strip_prefix(own))
            .filter(|path| path.starts_with('/'))
    }
}

async fn watcher_state(connection: &gio::DBusConnection, owner: &str) -> Option<WatcherState> {
    let props = properties(connection, owner, PATH, WATCHER).await?;
    Some(WatcherState {
        host: props.get("IsStatusNotifierHostRegistered")?.get::<bool>()?,
        items: props
            .get("RegisteredStatusNotifierItems")?
            .get::<Vec<String>>()?,
    })
}

async fn active_menu(
    connection: &gio::DBusConnection,
    own: &str,
    path: &str,
    expected: &str,
) -> Option<String> {
    let item = properties(connection, own, path, ITEM).await?;
    if item.get("Id")?.str()? != expected || item.get("Status")?.str()? != "Active" {
        return None;
    }
    let menu = item.get("Menu")?.str()?;
    (menu.starts_with('/') && menu != "/").then(|| menu.to_owned())
}

async fn registered(
    connection: &gio::DBusConnection,
    expected: &str,
    state: &WatcherState,
) -> bool {
    if !state.host {
        return false;
    }
    let Some(own) = connection.unique_name() else {
        return false;
    };
    // libappindicator and gio use the shared session bus connection. Restrict
    // reads to our own registered objects, never arbitrary other services.
    for path in state.own_paths(&own).take(4) {
        if active_menu(connection, &own, path, expected)
            .await
            .is_some()
        {
            return true;
        }
    }
    false
}

async fn recover_registration(
    connection: &gio::DBusConnection,
    owner: &str,
    expected: &str,
    path: &str,
    initial: &WatcherState,
    shared: &Shared,
    generation: u64,
) -> bool {
    let Some(own) = connection.unique_name() else {
        return false;
    };
    if initial.own_paths(&own).next().is_some() {
        return false;
    }
    // Ayatana can export its real StatusNotifierItem/DBusMenu yet miss the
    // registration when its watcher callback beats its private bus callback.
    let Some(menu) = active_menu(connection, &own, path, expected).await else {
        return false;
    };
    if menu != format!("{path}/Menu")
        || connection
            .call_future(
                Some(&own),
                &menu,
                MENU,
                "GetLayout",
                Some(&(0_i32, 0_i32, vec!["visible"]).to_variant()),
                Some(glib::VariantTy::new("(u(ia{sv}av))").expect("DBusMenu layout type")),
                DBusCallFlags::NO_AUTO_START,
                500,
            )
            .await
            .is_err()
    {
        return false;
    }
    // Native registration may have caught up during the menu read. A failed
    // watcher read is not proof of absence, and a host is only needed to hide.
    if watcher_state(connection, owner)
        .await
        .is_none_or(|latest| latest.own_paths(&own).next().is_some())
    {
        return false;
    }
    let current_owner = connection
        .call_future(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "GetNameOwner",
            Some(&(WATCHER,).to_variant()),
            None,
            DBusCallFlags::NO_AUTO_START,
            500,
        )
        .await
        .ok()
        .and_then(|value| value.get::<(String,)>());
    if current_owner.as_ref().map(|value| value.0.as_str()) != Some(owner)
        || !shared.current(generation)
        || !shared.begin_recovery(owner)
    {
        return false;
    }
    // Target the verified unique owner, never an activated or replacement
    // service. No replay, even if the method reply is lost.
    connection
        .call_future(
            Some(owner),
            PATH,
            WATCHER,
            "RegisterStatusNotifierItem",
            Some(&(path,).to_variant()),
            Some(glib::VariantTy::UNIT),
            DBusCallFlags::NO_AUTO_START,
            500,
        )
        .await
        .is_ok()
}
