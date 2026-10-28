use super::*;

fn fail(invocation: gio::DBusMethodInvocation, failure: commands::Failure) {
    invocation.return_dbus_error(failure.0, failure.1);
}
pub(super) fn handle<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    host: &Arc<Host>,
    interface: &str,
    method: &str,
    parameters: &glib::Variant,
    invocation: gio::DBusMethodInvocation,
) {
    if host.stopped.load(Ordering::Acquire) {
        return fail(
            invocation,
            commands::Failure(
                "org.freedesktop.DBus.Error.Disconnected",
                "Media session has closed",
            ),
        );
    }
    if interface == ROOT {
        if method == "Raise" {
            crate::lifecycle::show(app);
        }
        invocation.return_value(None);
        return;
    }
    let Ok(view) = current(app) else {
        return fail(
            invocation,
            commands::Failure(
                "org.freedesktop.DBus.Error.Failed",
                "Media selection changed",
            ),
        );
    };
    let command = if interface == PLAYER {
        commands::method(&view, method, parameters)
    } else {
        match method {
            "Get" => {
                let Some((interface, name)) = parameters.get::<(String, String)>() else {
                    return fail(invocation, commands::INVALID);
                };
                let values = match interface.as_str() {
                    ROOT => properties::root(),
                    PLAYER => properties::player(&view),
                    _ => return fail(invocation, commands::INVALID),
                };
                match values.get(&name) {
                    Some(value) => invocation.return_value(Some(&(value.clone(),).to_variant())),
                    None => fail(
                        invocation,
                        commands::Failure(
                            "org.freedesktop.DBus.Error.UnknownProperty",
                            "Unknown media property",
                        ),
                    ),
                }
                return;
            }
            "GetAll" => {
                let Some((interface,)) = parameters.get::<(String,)>() else {
                    return fail(invocation, commands::INVALID);
                };
                let values = match interface.as_str() {
                    ROOT => properties::root(),
                    PLAYER => properties::player(&view),
                    _ => return fail(invocation, commands::INVALID),
                };
                invocation.return_value(Some(&(values,).to_variant()));
                return;
            }
            "Set" => {
                let Some((interface, name, value)) =
                    parameters.get::<(String, String, glib::Variant)>()
                else {
                    return fail(invocation, commands::INVALID);
                };
                if interface != PLAYER {
                    return fail(invocation, commands::UNSUPPORTED);
                }
                commands::set(&view, &name, &value)
            }
            _ => Err(commands::INVALID),
        }
    };
    let command = match command {
        Ok(None) => {
            invocation.return_value(None);
            return;
        }
        Ok(Some(command)) => command,
        Err(error) => return fail(invocation, error),
    };
    let Some(target) = view.transport.target else {
        return fail(invocation, commands::UNSUPPORTED);
    };
    if host
        .busy
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return fail(
            invocation,
            commands::Failure(
                "org.freedesktop.DBus.Error.LimitsExceeded",
                "A media command is already pending",
            ),
        );
    }
    let worker_app = app.clone();
    let worker = tauri::async_runtime::spawn(async move {
        commands::execute(&worker_app.state::<MusicClient>(), &target, command).await
    });
    let app = app.clone();
    let host = host.clone();
    // Invocation is a GUI-context GObject. Only the owned business request goes
    // to Tokio; Properties.Set is acknowledged after persistence has completed.
    glib::MainContext::default().spawn_local(async move {
        let result = worker.await;
        if let Ok(view) = current(&app) {
            signals::publish(&host, &view);
        }
        host.busy.store(false, Ordering::Release);
        match result {
            Ok(Ok(())) => invocation.return_value(None),
            _ => fail(
                invocation,
                commands::Failure(
                    "org.freedesktop.DBus.Error.Failed",
                    "Media command failed or its selection changed",
                ),
            ),
        }
    });
}
