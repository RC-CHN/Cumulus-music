// Host measurements only. Sampling, limits, counters and wire encoding are Rust.
// No key values, input contents, URLs, cookies, SDK tokens or network calls.
(generation) => {
  window.__cumulusWatchmanEvents?.stop();
  let active = true, failed = false, sequence = 0, pending = 0, chain = Promise.resolve();
  const bindings = [];
  const stop = () => { active = false; for (const [target, name, fn] of bindings.splice(0)) target.removeEventListener(name, fn, true); };
  const control = { stop, flushed: () => chain, generation };
  window.__cumulusWatchmanEvents = control;
  const send = (payload, observed_ms = Date.now()) => {
    if (!active) return;
    if (pending >= 256) { payload = { kind: 'overflow' }; stop(); }
    const frame = { generation, sequence: ++sequence, observed_ms, payload };
    pending++;
    chain = chain.then(() => {
      if (!failed) return window.__TAURI_INTERNALS__.invoke('watchman_observation', { frame });
    }).catch(() => { failed = true; stop(); }).finally(() => { pending--; });
  };
  const number = v => typeof v === 'number' && Number.isFinite(v) ? v : null;
  const point = p => ({ client_x: number(p?.clientX), client_y: number(p?.clientY), screen_x: number(p?.screenX) });
  const observe = event => {
    const target = event.target || event.srcElement;
    const sample = {
      dom_type: event.type, target_present: !!target, target_id: typeof target?.id === 'string' ? target.id : '',
      trusted: typeof event.isTrusted === 'boolean' ? event.isTrusted : null, point: point(event), button: number(event.button),
    };
    if (event.changedTouches?.length) sample.touch = point(event.changedTouches[0]);
    if (event.type === 'scroll') {
      const node = document.compatMode === 'CSS1Compat' ? document.documentElement : document.body;
      sample.scroll = 'pageXOffset' in window ? [window.pageXOffset, window.pageYOffset] : [node.scrollLeft, node.scrollTop];
    }
    if (event.type === 'deviceorientation') sample.orientation = {
      alpha: number(event.alpha), beta: number(event.beta), gamma: number(event.gamma),
      absolute: typeof event.absolute === 'boolean' ? event.absolute : null,
    };
    if (event.type === 'devicemotion') {
      const vector = v => v ? [number(v.x), number(v.y), number(v.z)] : null;
      sample.motion = { acceleration: vector(event.acceleration), including_gravity: vector(event.accelerationIncludingGravity), interval: number(event.interval) };
    }
    send({ kind: 'event', sample });
  };
  const bind = (target, name, fn) => { target.addEventListener(name, fn, true); bindings.push([target, name, fn]); };
  for (const name of ['mousemove','touchmove','pointermove','mspointermove','mousedown','touchstart','pointerdown','mspointerdown','mouseup','touchend','pointerup','mspointerup','click','keydown']) bind(document, name, observe);
  for (const name of ['focus','blur','scroll','deviceorientation','devicemotion']) bind(window, name, observe);
  const battery = (reading, initial) => send({ kind: 'battery', initial, sample: {
    charging: typeof reading.charging === 'boolean' ? reading.charging : null,
    level: number(reading.level), charging_time: number(reading.chargingTime), discharging_time: number(reading.dischargingTime),
  } });
  // Both reviewed builds gate this branch with navigator.Mc(), not with
  // getBattery availability. Preserve that unusual condition; do not enable
  // an extra measurement merely because a standard API happens to exist.
  let batterySupported = false;
  try { batterySupported = !!navigator.Mc(); } catch (_) {}
  if (batterySupported && typeof navigator.getBattery === 'function') {
    navigator.getBattery().then(reading => {
      if (!active) return;
      battery(reading, true);
      // Official handlers read the change Event itself and fall back to the
      // initial reading in native state; do not replace it with the manager.
      for (const name of ['chargingchange','chargingtimechange','dischargingtimechange','levelchange']) bind(reading, name, event => battery(event, false));
    }).catch(() => {});
  }
}
