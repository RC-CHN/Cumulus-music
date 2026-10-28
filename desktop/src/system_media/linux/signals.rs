use super::*;

pub(super) fn publish(host: &Host, view: &NativeMediaDetails) {
    let Ok(mut guard) = host.bus.lock() else {
        return;
    };
    let Some(bus) = guard.as_mut() else {
        return;
    };
    let Some(connection) = &bus.connection else {
        return;
    };
    let mut values = properties::player(view);
    values.remove("Position");
    values.remove("CanControl");
    let changed = values
        .iter()
        .filter(|(key, value)| bus.properties.get(*key) != Some(*value))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect::<Properties>();
    if !changed.is_empty()
        && connection
            .emit_signal(
                None,
                PATH,
                PROPERTIES,
                "PropertiesChanged",
                Some(&(PLAYER, changed, Vec::<String>::new()).to_variant()),
            )
            .is_ok()
    {
        bus.properties = values;
    }
    let position = properties::position(view);
    let id = view.item.as_ref().map(|i| i.id.clone()).unwrap_or_default();
    let moved = bus.anchor.as_ref().map_or(position != 0, |anchor| {
        if anchor.id != id {
            return position != 0;
        }
        let expected = anchor.position as f64
            + if anchor.moving {
                anchor.at.elapsed().as_secs_f64() * anchor.speed * 1_000_000.0
            } else {
                0.0
            };
        anchor.seek_revision != view.seek_revision || (position as f64 - expected).abs() > 750_000.0
    });
    let delivered = moved
        && connection
            .emit_signal(
                None,
                PATH,
                PLAYER,
                "Seeked",
                Some(&(position,).to_variant()),
            )
            .is_ok();
    if bus.anchor.is_none()
        || delivered
        || bus.anchor.as_ref().is_some_and(|a| {
            a.id != id || a.moving != view.transport.playing || a.speed != view.speed
        })
    {
        bus.anchor = Some(Anchor {
            at: Instant::now(),
            position,
            moving: view.transport.playing,
            speed: view.speed,
            seek_revision: view.seek_revision,
            id,
        });
    }
}
