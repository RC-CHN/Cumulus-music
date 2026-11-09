// Real pointer moves come from the isolated XTest driver at named screenshots.
(async () => {
  if (window.__mixedReorderStarted) return;
  window.__mixedReorderStarted = true;
  const one = selector => document.querySelector(selector);
  const all = selector => [...document.querySelectorAll(selector)];
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const report = (stage, details = {}) =>
    invoke('plugin:event|emit', { event: 'local-proof', payload: { stage, ...details } });
  const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
  const check = (ok, message) => { if (!ok) throw Error(message); };
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  let step = 'bootstrap';
  async function wait(get, label) {
    const end = performance.now() + 16000;
    while (performance.now() < end) {
      const value = await get();
      if (value) return value;
      await delay(50);
    }
    throw Error(label);
  }
  function click(node) {
    check(node?.isConnected && !node.disabled, 'enabled visible control');
    node.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    node.focus({ preventScroll: true });
    node.click();
  }
  const rows = () => all('.track-scroll .track-row[data-track-id]').map(row => row.dataset.trackId);
  const row = id => one('.track-scroll .track-row[data-track-id="' + CSS.escape(id) + '"]');
  const table = () => one('.track-scroll.playlist-reorder-scroll');
  const status = () => one('.playlist-mixed-order-status');
  const marker = () => one('.playlist-reorder-marker');
  const reconcile = () => [...(status()?.querySelectorAll('button') ?? [])]
    .find(button => button.textContent.trim() === '核对结果' && !button.disabled);
  const gestures = [];
  let epoch, localId, paused, queue;
  const snapshot = () => invoke('player_snapshot', { epoch });
  const currentReceipt = async () => (await invoke('playlist_mixed_reorder_receipt', { epoch, id: '11' })).value;
  for (const eventName of ['pointerdown', 'pointerup']) {
    document.addEventListener(eventName, event => {
      if (paused && event.button === 0 && event.pointerType === 'mouse')
        gestures.push({ event: eventName, trusted: event.isTrusted });
    }, true);
  }
  function point(x, y) {
    return { x, y, width: innerWidth, height: innerHeight, ratio: devicePixelRatio };
  }
  function dragPoints(from, before) {
    const title = row(from)?.querySelector('.track-row-title-text');
    const target = row(before), tableBox = table()?.getBoundingClientRect();
    check(title && target && tableBox, 'displayed drag source and target');
    const text = title.getBoundingClientRect(), to = target.getBoundingClientRect();
    const start = point(text.left + Math.min(14, text.width / 2), text.top + text.height / 2);
    const end = point(to.left + Math.min(300, to.width / 2), to.top + 12);
    const sourceHit = document.elementFromPoint(start.x, start.y);
    const targetHit = document.elementFromPoint(end.x, end.y);
    check(row(from).contains(sourceHit) && !sourceHit.closest('button,a,input,select') &&
      target.contains(targetHit) && end.y >= tableBox.top && end.y < tableBox.bottom,
      'real pointer points hit the visible source text and target row');
    return { from: start, to: end };
  }
  function outsidePoint() {
    const heading = one('.playlist-hero h1') ?? one('.playlist-hero');
    const box = heading.getBoundingClientRect();
    const outside = point(box.left + 15, box.top + 15);
    check(!table().contains(document.elementFromPoint(outside.x, outside.y)), 'outside the reorder table');
    return outside;
  }
  async function capture(name, data = {}) {
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const native = await snapshot();
    const actualQueue = await invoke('player_queue', { epoch });
    check(native.state === 'paused' && native.playId === paused.playId && native.entryId === paused.entryId &&
      native.position === paused.position && native.queueRevision === paused.queueRevision &&
      same(actualQueue.tracks.map(entry => entry.track.id), queue),
      'reorder never replaces or resumes the native local queue');
    await report(name, { rows: rows(), native, queue: actualQueue.tracks.map(entry => entry.track.id),
      gestures: [...gestures], ...data });
    await wait(() => window.__mixedCapture === name, 'capture acknowledgment ' + name);
  }
  async function prepareDrag(ready, marked, from, before, dropOutside = false) {
    const pointer = dragPoints(from, before);
    await capture(ready, { pointer });
    await wait(() => {
      const node = marker(), box = node?.getBoundingClientRect();
      return box?.width && box.height && box.top >= table().getBoundingClientRect().top - 2 &&
        box.top < table().getBoundingClientRect().bottom;
    }, 'real pointer move draws the insertion marker');
    await capture(marked, dropOutside ? { outside: outsidePoint() } : {});
  }

  try {
    const account = await wait(async () => {
      const state = await invoke('account_snapshot');
      return state.status === 'authenticated' && one('.playerbar') && state;
    }, 'sealed account is ready');
    epoch = account.epoch;
    const localEntry = await wait(() => all('button').find(button => !button.disabled &&
      (button.textContent.trim() === '本地音乐' || button.getAttribute('aria-label') === '本地音乐')),
      'local library navigation');
    click(localEntry);
    const local = await wait(() => one('.local-tracks tbody tr[data-local-id]'), 'native file is indexed');
    localId = local.dataset.localId;
    click(local.querySelector('[data-local-track-menu]'));
    const add = await wait(() => one('.action-menu:not([inert]) [data-menu-key="add_to_playlist"]:not(:disabled)'),
      'local member action');
    click(add);
    const dialog = () => one('dialog[open][aria-labelledby="playlist-local-member-title"]');
    const submit = await wait(() => dialog()?.querySelector('button[type="submit"]:not(:disabled)'),
      'owned playlist membership chooser');
    check(dialog().querySelector('select')?.value === '11', 'only chosen ordinary owned playlist');
    click(submit);
    await wait(() => !dialog(), 'local fixture membership saved');
    const group = one('[data-sidebar-group="created"] .sidebar-group-toggle');
    if (group.getAttribute('aria-expanded') === 'false') click(group);
    group.closest('header').scrollIntoView({ block: 'start', behavior: 'instant' });
    click(await wait(() => one('[data-sidebar-id="11"]'), 'ordinary owned playlist is visible'));
    const initial = [localId, '701', '702'];
    await wait(() => table() && same(rows(), initial), 'mixed table accepts pointer reorder');
    click(row(localId).querySelector('[data-result-action="play"]'));
    await wait(async () => {
      const state = await snapshot();
      return state.state === 'playing' && state.track?.id === localId && state.track.local && state.position > .08;
    }, 'real indexed WAV advances');
    paused = await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    queue = (await invoke('player_queue', { epoch })).tracks.map(entry => entry.track.id);
    check(paused.state === 'paused' && same(queue, initial), 'independent native queue baseline');

    step = 'outside drop';
    await prepareDrag('ux_01_outside_ready', 'ux_02_outside_marker', localId, '702', true);
    await wait(() => !marker() && gestures.length === 2 && same(rows(), initial), 'outside drop cancels');
    await capture('ux_03_outside_cancelled');

    step = 'local-only move';
    await prepareDrag('ux_04_local_ready', 'ux_05_local_marker', localId, '702');
    await report('await_reorder_write');
    await wait(() => window.__mixedReorderWriteSeen === true && status()?.textContent.includes('正在保存'),
      'first exact write is held inside the synthetic server');
    const firstSending = await currentReceipt();
    check(firstSending?.phase === 'sending' && same(rows(), initial), 'no local projection before acknowledgment');
    await capture('ux_06_sending', { receipt: firstSending });
    await report('release_reorder_ack');
    const first = ['701', localId, '702'];
    await wait(() => table() && same(rows(), first), 'local-only move applied after positive receipt');
    await capture('ux_07_acknowledged', { receipt: await currentReceipt() });

    step = 'changed online order with lost reply';
    await prepareDrag('ux_08_online_ready', 'ux_09_online_marker', '702', '701');
    await wait(() => reconcile() && status()?.textContent.includes('本机曲目位置未更新'),
      'lost response exposes explicit readback only');
    check(rows().indexOf(localId) === 1 && !table(), 'original local row position remains and editing is gated');
    await capture('ux_10_uncertain', { receipt: await currentReceipt() });

    step = 'explicit readback';
    click(reconcile());
    await wait(() => table() && !reconcile() && same(rows(), ['702', '701', localId]),
      'exact distinguishable readback commits the complete intended order');
    await capture('ux_11_readback', { receipt: await currentReceipt() });
    check(same(gestures, ['pointerdown', 'pointerup', 'pointerdown', 'pointerup', 'pointerdown', 'pointerup']
      .map(event => ({ event, trusted: true }))), 'three trusted X11 gestures');
    await report('complete', { ok: true, localId, gestures, queue });
  } catch (error) {
    await report('failed', { step, error: String(error), rows: rows(), gestures, status: status()?.innerText });
  }
})();
