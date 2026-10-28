mod commands;
mod dispatch;
mod properties;
mod signals;
use cumulus_core::client::{
    MusicClient,
    native_media::{NativeMediaDetails, NativeMediaStatus},
};
use gio::glib::{self, variant::ToVariant};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::Manager;
type Properties = BTreeMap<String, glib::Variant>;
const ROOT: &str = "org.mpris.MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const PATH: &str = "/org/mpris/MediaPlayer2";

struct Anchor {
    at: Instant,
    position: i64,
    moving: bool,
    speed: f64,
    seek_revision: u64,
    id: String,
}
#[derive(Default)]
struct Bus {
    connection: Option<gio::DBusConnection>,
    objects: Vec<gio::RegistrationId>,
    properties: Properties,
    anchor: Option<Anchor>,
}
impl Bus {
    fn disconnect(&mut self) {
        if let Some(connection) = self.connection.take() {
            for object in self.objects.drain(..) {
                let _ = connection.unregister_object(object);
            }
            if !connection.is_closed() {
                connection.close(gio::Cancellable::NONE, |_| {});
            }
        }
        self.properties.clear();
        self.anchor = None;
    }
}
impl Drop for Bus {
    fn drop(&mut self) {
        self.disconnect();
    }
}
#[derive(Default)]
struct Host {
    stopped: AtomicBool,
    busy: AtomicBool,
    generation: AtomicU64,
    bus: Mutex<Option<Bus>>,
    retry_after: Mutex<Option<Instant>>,
    shared_lifetime: Mutex<Option<gio::DBusConnection>>,
}
fn current<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> cumulus_core::Result<NativeMediaDetails> {
    app.state::<MusicClient>().native_media_details()
}
fn remove(host: &Host) {
    host.generation.fetch_add(1, Ordering::AcqRel);
    let bus = host.bus.lock().ok().and_then(|mut bus| bus.take());
    drop(bus); // Unregister callbacks without holding their state mutex.
}
fn defer_retry(host: &Host) {
    if let Ok(mut retry) = host.retry_after.lock() {
        *retry = Some(Instant::now() + Duration::from_secs(2));
    }
    remove(host);
}
pub(super) fn stop<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(host) = app.try_state::<Arc<Host>>() {
        host.stopped.store(true, Ordering::Release);
        remove(&host);
    }
}
pub(super) fn install<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if app.try_state::<Arc<Host>>().is_some() || app.try_state::<MusicClient>().is_none() {
        return;
    }
    let host = Arc::new(Host::default());
    app.manage(host.clone());
    let app = app.clone();
    // Runs on GTK's main context; queries never initialize the audio engine.
    glib::MainContext::default().spawn_local(async move {
        while !host.stopped.load(Ordering::Acquire) {
            if let Ok(view) = current(&app) {
                if view.transport.target.is_some() {
                    ensure(&app, &host);
                    signals::publish(&host, &view);
                } else if host.bus.lock().is_ok_and(|bus| bus.is_some()) {
                    remove(&host);
                }
            }
            glib::timeout_future(Duration::from_millis(250)).await;
        }
    });
}
fn ensure<R: tauri::Runtime>(app: &tauri::AppHandle<R>, host: &Arc<Host>) {
    if host
        .retry_after
        .lock()
        .is_ok_and(|retry| retry.is_some_and(|at| at > Instant::now()))
    {
        return;
    }
    let Ok(mut guard) = host.bus.lock() else {
        return;
    };
    if guard.is_some() {
        return;
    }
    let generation = host.generation.fetch_add(1, Ordering::AcqRel) + 1;
    *guard = Some(Bus::default());
    drop(guard);
    let host = host.clone();
    let app = app.clone();
    glib::MainContext::default().spawn_local(async move {
        // GTK also holds GIO's shared connection. Prevent its default SIGTERM
        // on bus loss, while keeping MPRIS recovery on a separate connection.
        if let Ok(Ok(shared)) = glib::future_with_timeout(
            Duration::from_secs(5),
            gio::bus_get_future(gio::BusType::Session),
        )
        .await
        {
            shared.set_exit_on_close(false);
            // GIO caches only a weak reference. Retain the guarded connection so
            // a later GTK consumer cannot recreate it with exit-on-close=true.
            if let Ok(mut lifetime) = host.shared_lifetime.lock() {
                *lifetime = Some(shared);
            }
        }
        // A private connection avoids GIO's cached, possibly closed GTK bus.
        // Discovery and authentication must not block the GUI main context.
        let cancel = gio::Cancellable::new();
        let lookup_cancel = cancel.clone();
        let lookup = tauri::async_runtime::spawn_blocking(move || {
            gio::dbus_address_get_for_bus_sync(gio::BusType::Session, Some(&lookup_cancel))
        });
        let address = glib::future_with_timeout(Duration::from_secs(5), lookup).await;
        use gio::prelude::CancellableExt;
        cancel.cancel();
        let Ok(Ok(Ok(address))) = address else {
            if host.generation.load(Ordering::Acquire) == generation {
                defer_retry(&host);
            }
            return;
        };
        let result = glib::future_with_timeout(
            Duration::from_secs(5),
            gio::DBusConnection::for_address_future(
                &address,
                gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
                    | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
                None,
            ),
        )
        .await;
        let Ok(Ok(connection)) = result else {
            if host.generation.load(Ordering::Acquire) == generation {
                defer_retry(&host);
            }
            return;
        };
        connection.set_exit_on_close(false);
        if host.stopped.load(Ordering::Acquire)
            || host.generation.load(Ordering::Acquire) != generation
        {
            connection.close(gio::Cancellable::NONE, |_| {});
            return;
        }
        if register(&app, &host, &connection).is_err() {
            connection.close(gio::Cancellable::NONE, |_| {});
            defer_retry(&host);
            return;
        }
        let lost = Arc::downgrade(&host);
        connection.connect_closed(move |_, _, _| {
            if let Some(host) = lost.upgrade()
                && host.generation.load(Ordering::Acquire) == generation
            {
                defer_retry(&host);
            }
        });
        // gio 0.18's bus_own_name_on_connection unwraps a null connection in
        // its name-lost closure after disconnect and aborts across the C ABI.
        // Request ownership directly on our private connection instead. Closing
        // it releases the name; DO_NOT_QUEUE prevents a delayed unexpected owner.
        let name = format!(
            "org.mpris.MediaPlayer2.Cumulus.instance{}",
            std::process::id()
        );
        let request = (name, gio::BusNameOwnerFlags::DO_NOT_QUEUE.bits()).to_variant();
        let owned = connection
            .call_future(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "RequestName",
                Some(&request),
                None,
                gio::DBusCallFlags::NONE,
                5000,
            )
            .await
            .is_ok_and(|reply| matches!(reply.get::<(u32,)>(), Some((1 | 4,))));
        if !owned && host.generation.load(Ordering::Acquire) == generation {
            defer_retry(&host);
        }
    });
}

fn register<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    host: &Arc<Host>,
    connection: &gio::DBusConnection,
) -> Result<(), glib::Error> {
    let node = gio::DBusNodeInfo::for_xml(include_str!("interfaces.xml"))?;
    let mut objects = vec![];
    for interface in [ROOT, PLAYER, PROPERTIES] {
        let app = app.clone();
        let method_app = app.clone();
        let host = host.clone();
        let info = node
            .lookup_interface(interface)
            .expect("fixed interface XML");
        match connection.register_object(
            PATH,
            &info,
            move |_, _, _, interface, method, parameters, invocation| {
                dispatch::handle(
                    &method_app,
                    &host,
                    interface,
                    method,
                    &parameters,
                    invocation,
                );
            },
            move |_, _, _, interface, name| {
                let values = if interface == ROOT {
                    properties::root()
                } else {
                    properties::player(&current(&app).unwrap_or_default())
                };
                values.get(name).cloned().unwrap_or_else(|| ().to_variant())
            },
            |_, _, _, _, _, _| false,
        ) {
            Ok(id) => objects.push(id),
            Err(error) => {
                for id in objects {
                    let _ = connection.unregister_object(id);
                }
                return Err(error);
            }
        }
    }
    if let Ok(mut guard) = host.bus.lock()
        && let Some(bus) = guard.as_mut()
    {
        bus.disconnect();
        bus.connection = Some(connection.clone());
        bus.objects = objects;
    } else {
        for id in objects {
            let _ = connection.unregister_object(id);
        }
    }
    Ok(())
}
