// Real Settings/IPC/SQLite path driven by the isolated native PC server.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (condition, message) => { if (!condition) throw Error(message); };
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, data = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...data } });
  const click = (node) => {
    assert(node && !node.disabled, 'enabled settings control');
    node.focus({ preventScroll: true });
    node.click();
  };
  async function wait(test, label) {
    for (let n = 0; n < 200; n++) {
      if (await test()) return;
      await sleep(50);
    }
    throw Error(label);
  }
  const settings = () => one('button[aria-label="打开设置"]');
  function inspectSettings(node) {
    const rect = node?.getBoundingClientRect();
    const hit = rect && document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2);
    return {
      connected: !!node?.isConnected, disabled: node?.disabled ?? null, inert: !!node?.closest('[inert]'),
      rect: rect ? { x: rect.x, y: rect.y, width: rect.width, height: rect.height } : null,
      hit: !!(node && hit && node.contains(hit)), hitTag: hit?.tagName ?? null,
      viewport: [innerWidth, innerHeight], heading: one('.main-content h1')?.textContent ?? null,
    };
  }
  const pref = () => one('input[name="recent-history-sync"]');
  const dialog = () => one('.settings-confirm[open]');
  const section = () => pref()?.closest('section');
  const saved = () => section()?.querySelector('[role="status"]')?.textContent.trim() === '已保存。';
  const ready = () => pref() && !pref().disabled && !one('.page-enter-active,.page-leave-active');
  let epoch, stage = 'startup', nonce = 0, count = 0, core, original, unlisten, checkbox;
  const observations = [], componentRequests = [], commands = [];
  let startupControl;
  const read = async () => {
    const reply = await invoke('history_sync_preference', { epoch, action: { kind: 'read' } });
    assert(reply.epoch === epoch, 'native settings receipt epoch');
    return reply.value;
  };
  async function refresh(mode, expected, confirmed = false) {
    stage = 'refresh_' + mode + '_' + (nonce + 1);
    const before = await read();
    const command = ++nonce;
    await emit('pc-presentation-fixture', { mode, nonce: command });
    await wait(() => window.__pcPresentationFixtureAck?.nonce === command, 'native refresh receipt');
    const ack = window.__pcPresentationFixtureAck;
    assert(ack.result?.Ok && ack.afterFull === ack.beforeFull + 1, 'one complete signed assignment');
    const after = await read();
    await wait(() => pref() === checkbox && ready() && pref().checked === expected,
      'open setting updates on the same checkbox');
    assert(after.enabled === expected && after.firstDisableConfirmed === confirmed,
      'cohort update preserves confirmation provenance');
    if (before.enabled !== expected) {
      await wait(() => observations.some((event) => event.epoch === epoch
        && event.value.revision === after.revision
        && event.value.enabled === expected
        && event.value.firstDisableConfirmed === confirmed), 'committed native history settings event');
    }
    commands.push({ mode, before, after, beforeFull: ack.beforeFull, afterFull: ack.afterFull });
    return after;
  }
  async function capture(label) {
    stage = label;
    assert(pref() === checkbox && ready() && section(), 'same mounted setting row');
    const native = await read();
    const row = pref().getBoundingClientRect(), panel = one('.settings-scroll').getBoundingClientRect();
    assert(row.top >= panel.top - 1 && row.bottom <= panel.bottom + 1, 'setting fits visible viewport');
    assert(document.documentElement.scrollWidth <= innerWidth, 'no horizontal overflow');
    assert(pref().checked === native.enabled, 'controlled checkbox agrees with native storage');
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const name = `ux_${String(++count).padStart(2, '0')}_${label}`;
    window.__searchFooterRecorded = '';
    await emit(name, {
      label, native, checked: pref().checked, dialog: !!dialog(), saved: saved(),
      focusRestored: document.activeElement === pref(), componentRequests: [...componentRequests],
      observations: [...observations], commands: [...commands],
    });
    await wait(() => window.__searchFooterRecorded === name, 'capture receipt ' + label);
  }
  try {
    await wait(async () => {
      const account = await invoke('account_snapshot');
      if (account.status !== 'authenticated') return false;
      epoch = account.epoch;
      return true;
    }, 'verified account startup');
    await wait(async () => (await invoke('pc_presentation', { epoch })).searchFooter,
      'signed startup full batch');
    const provides = one('#app').__vue_app__._context.provides;
    core = provides[Reflect.ownKeys(provides).find((key) => typeof key === 'symbol' && key.description === 'core-client')];
    assert(core, 'actual typed frontend client');
    unlisten = await core.onHistorySyncChanged((event) => observations.push(event));
    original = core.historySyncPreference;
    core.historySyncPreference = (owner, action) => {
      componentRequests.push({ epoch: owner, action });
      return original.call(core, owner, action);
    };
    await emit('resize', { width: 900, height: 650 });
    await wait(() => innerWidth === 900 && innerHeight === 650, 'bounded settings viewport');
    // Account and signed startup can settle before the shell finishes its
    // resource load. Use the actual enabled header control after resizing.
    stage = 'settings_control_ready';
    let trigger;
    await wait(() => {
      trigger = settings();
      const state = startupControl = inspectSettings(trigger);
      return state.connected && state.disabled === false && !state.inert &&
        state.rect.width > 0 && state.rect.height > 0 && state.hit;
    }, 'connected and enabled Settings control at the bounded viewport');
    stage = 'settings_open';
    click(trigger);
    await wait(ready, 'settings preference mounted');
    checkbox = pref();
    checkbox.scrollIntoView({ block: 'center' });
    const initial = await read();
    assert(initial.enabled && !initial.firstDisableConfirmed && !saved(), 'fresh treatment default');
    await capture('untouched_treatment');

    await refresh('c', false);
    await capture('untouched_control');
    await refresh('t1', true);
    await capture('untouched_treatment_again');

    click(pref());
    await wait(dialog, 'first disable confirmation');
    let pending = dialog();
    assert(pending.textContent.includes('关闭后，本设备最近播放列表仅展示本地记录'),
      'official first-disable explanation');
    await refresh('c', false);
    assert(dialog() === pending && !saved(), 'experiment does not close or save the active dialog');
    await capture('pending_during_experiment_off');
    click(dialog().querySelector('[aria-label="关闭设置确认"]'));
    await wait(() => !dialog() && document.activeElement === pref(), 'cancel restores checkbox focus');
    assert(!(await read()).firstDisableConfirmed && !saved(), 'cancel consumes no confirmation');
    await capture('cancelled_without_choice');

    await refresh('t1', true);
    click(pref());
    await wait(dialog, 'unconsumed confirmation is shown again');
    pending = dialog();
    const off = await refresh('c', false);
    assert(dialog() === pending, 'second confirmation survives committed control default');
    click(dialog().querySelector('footer .primary'));
    await wait(() => !dialog() && ready() && saved() && document.activeElement === pref(),
      'explicit confirmation commits, closes and restores focus');
    const confirmed = await read();
    assert(confirmed.firstDisableConfirmed && !confirmed.enabled
      && BigInt(confirmed.revision) > BigInt(off.revision),
    'manual confirmation has its own committed revision even with same enabled value');
    await wait(() => observations.some((event) => event.epoch === epoch
      && event.value.revision === confirmed.revision && event.value.firstDisableConfirmed),
    'manual confirmation emits the committed typed state');
    await capture('confirmed_while_effectively_off');

    const retained = await refresh('t1', false, true);
    assert(JSON.stringify(retained) === JSON.stringify(confirmed), 'signed treatment cannot change manual choice');
    await capture('manual_choice_retained');
    assert(componentRequests.length === 2 && componentRequests[0].action.kind === 'read'
      && componentRequests[1].action.kind === 'set' && componentRequests[1].action.confirmed_first_disable === true,
    'one mount read and one explicit user save; no automatic renderer writes or polling');
    await emit('complete', {
      ok: true, stageCount: count, epoch, observations, commands, componentRequests,
      realMusicRequests: 0, realAccountWrites: 0,
    });
  } catch (error) {
    await emit('failed', { ok: false, at: stage, error: String(error), observations, commands, componentRequests,
      startupControl, currentSettingsControl: inspectSettings(settings()) });
  } finally {
    if (core && original) core.historySyncPreference = original;
    unlisten?.();
  }
})();
