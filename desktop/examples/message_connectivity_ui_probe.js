// Closed test of real engine-provided online/offline notifications. The
// outer proof process changes only its isolated loopback route.
(async () => {
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, data = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...data } });
  const assert = (condition, detail) => { if (!condition) throw Error(detail); };
  async function wait(predicate, detail, attempts = 240) {
    for (let i = 0; i < attempts; i++) {
      if (await predicate()) return;
      await sleep(50);
    }
    throw Error(detail);
  }
  let phase = 'startup';
  let sequence = 0;
  const events = [];
  for (const name of ['offline', 'online']) {
    window.addEventListener(name, event => events.push({
      type: event.type, trusted: event.isTrusted, navigator: navigator.onLine,
      time: performance.now(),
    }));
  }
  window.__messageFixtureAck = {};
  async function reads() {
    const id = ++sequence;
    await emit('message-fixture', { mode: 'read', id });
    await wait(() => window.__messageFixtureAck[id], 'missing fixture receipt');
    return window.__messageFixtureAck[id].requests;
  }
  const header = () => document.querySelector('.message-drawer-unread-dot');
  const trusted = name => events.filter(event => event.type === name && event.trusted).length;
  try {
    await wait(() => header() && document.querySelector('.track-row'), 'authenticated main ready');
    assert(navigator.onLine === true, 'the real WebView did not initialize online');
    const restarted = sessionStorage.getItem('connectivity-proof-reloaded');
    if (restarted) {
      phase = 'reload';
      const state = JSON.parse(restarted);
      assert((await invoke('account_snapshot')).epoch === state.epoch, 'reload unexpectedly changed the account');
      await sleep(6300);
      const calls = await reads();
      assert(calls.length === 2, 'the retired page timer caused an extra request');
      await emit('complete', { ok: true, realAccountWrites: 0, realMusicRequests: 0,
        trustedBrowserEvents: state.events, fabricatedRejected: true, reloadCanceledProbe: true });
      return;
    }
    const { epoch } = await invoke('account_snapshot');
    assert((await reads()).length === 1, 'one startup count only');
    const forged = await invoke('observe_message_connectivity', {
      report: { nonce: '0'.repeat(32), sequence: 1, initial: true, online: false },
    }).then(() => null, error => error);
    assert(forged?.code === 'stale', 'direct guessed page nonce was not rejected');
    phase = 'offline';
    await emit('ux_connectivity_link_down');
    await wait(() => trusted('offline') === 1 && navigator.onLine === false,
      'WebView never emitted an actual offline event');
    // M/119 stays online through the first 5-second mock, and equality can
    // postpone offline confirmation by another full 5-second cycle.
    await sleep(16100);
    Object.defineProperty(navigator, 'onLine', { configurable: true, value: true });
    window.dispatchEvent(new Event('online'));
    delete navigator.onLine;
    await sleep(1300);
    assert(events.some(event => event.type === 'online' && !event.trusted && event.navigator),
      'fake event did not exercise the intended browser value');
    assert((await reads()).length === 1, 'offline or untrusted event sent a count request');
    phase = 'recovery';
    await emit('ux_connectivity_link_up');
    await wait(() => trusted('online') === 1 && navigator.onLine === true,
      'WebView never emitted an actual online event');
    const recovered = events.find(event => event.type === 'online' && event.trusted).time;
    await wait(async () => (await reads()).length === 2, 'online notification did not rearm the native count');
    assert(performance.now() - recovered > 800, 'native recovery did not wait for the 1-second probe');
    await sleep(1200);
    assert((await reads()).length === 2, 'network recovery created a duplicate count timer');
    phase = 'reload-preparation';
    // A transient browser loss arms the original online-state 5-second probe.
    // A quick window online notification does not cancel it in M/119.
    await emit('ux_connectivity_reload_down');
    await wait(() => trusted('offline') === 2, 'no real offline event before reloading');
    await emit('ux_connectivity_reload_up');
    await wait(() => trusted('online') === 2, 'no real online event before reloading');
    sessionStorage.setItem('connectivity-proof-reloaded', JSON.stringify({ epoch, events }));
    window.location.reload();
  } catch (error) {
    await emit('failed', { phase, message: String(error), navigator: navigator.onLine, events });
  }
})();
