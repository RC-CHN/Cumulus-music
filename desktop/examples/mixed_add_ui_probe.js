// A real catalogue-row context gesture, ordinary picker, and a lost add ACK.
(async () => {
  if (window.__mixedAddStarted) return;
  window.__mixedAddStarted = true;
  const one = (selector) => document.querySelector(selector);
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const report = (stage, details = {}) =>
    invoke('plugin:event|emit', { event: 'local-proof', payload: { stage, ...details } });
  const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const check = (ok, message) => { if (!ok) throw Error(message); };
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const row = (id) => one('.track-scroll .track-row[data-track-id="' + CSS.escape(id) + '"]');
  const rows = () => [...document.querySelectorAll('.track-scroll .track-row[data-track-id]')]
    .map((node) => node.dataset.trackId);
  const button = (text, root = document) => [...root.querySelectorAll('button')]
    .find((node) => node.textContent.trim() === text && !node.disabled);
  const picker = () => one('dialog.playlist-collect-dialog[open]:not([inert])');
  const target = () => picker()?.querySelector('button[data-playlist-id="11"]');
  const status = () => picker()?.querySelector('.mutation-result');
  const enabled = (node) => node && !node.disabled && node.getAttribute('aria-disabled') !== 'true';
  const menu = () => one('.action-menu:not([inert])');
  const menuItem = () => menu() && button('收藏', menu());
  const context = [];
  let epoch, localId, baseline, queue, step = 'bootstrap';
  document.addEventListener('contextmenu', (event) => {
    const target = event.target.closest('.track-row[data-track-id]');
    if (target) context.push([target.dataset.trackId, event.isTrusted]);
  }, true);
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
    check(node?.isConnected && !node.disabled, 'enabled control');
    node.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    node.focus({ preventScroll: true });
    node.click();
  }
  function point(id) {
    const title = row(id)?.querySelector('.track-row-title-text');
    const rect = title?.getBoundingClientRect();
    check(rect, 'row title is visible');
    const x = rect.left + Math.min(rect.width / 2, 10), y = rect.top + rect.height / 2;
    const hit = document.elementFromPoint(x, y);
    check(row(id).contains(hit) && !hit.closest('button,a,input,select'), 'trusted point hits row text');
    return { x, y, width: innerWidth, height: innerHeight, ratio: devicePixelRatio };
  }
  async function navigate(id) {
    const group = await wait(() => one('[data-sidebar-group="created"] .sidebar-group-toggle'), 'created sidebar');
    if (group.getAttribute('aria-expanded') === 'false') click(group);
    group.closest('header').scrollIntoView({ block: 'start', behavior: 'instant' });
    click(await wait(() => one('[data-sidebar-id="' + id + '"]'), 'playlist ' + id));
  }
  const player = () => invoke('player_snapshot', { epoch });
  async function capture(stage, detail = {}) {
    await Promise.race([
      new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))),
      delay(1500).then(() => { throw Error('paint deadline ' + stage); }),
    ]);
    const state = await player();
    const actual = (await invoke('player_queue', { epoch })).tracks.map((entry) => entry.track.id);
    check(['state', 'playId', 'entryId', 'queueRevision', 'position'].every((key) => state[key] === baseline[key])
      && same(actual, queue), 'collection changed paused playback or its queue');
    await report(stage, { ...detail, rows: rows(), context: [...context], state, queue });
    await wait(() => window.__mixedCapture === stage, 'screenshot acknowledgement ' + stage);
  }
  try {
    const account = await wait(async () => {
      const value = await invoke('account_snapshot');
      return value.status === 'authenticated' && one('.playerbar') && value;
    }, 'sealed account');
    epoch = account.epoch;
    const indexed = await wait(async () => {
      const page = await invoke('local_page', { query: { keyword: '', sort: 'title', descending: true,
        group: 'list', groupKey: null, includeMissing: true, offset: 0, limit: 200, revision: null } });
      return page.roots.length === 1 && page.roots[0].status === 'ready' && page.tracks[0];
    }, 'indexed local fixture');
    localId = indexed.track.id;
    const page = (await invoke('playlist_page', { id: '11', page: { offset: 0, limit: 1 } })).value;
    const seeded = await invoke('mutate_playlist_local', { epoch, id: '11', revision: page.playlistLocal.revision,
      change: { op: 'add', rows: [{ id: localId, revision: indexed.revision }] } });
    check(seeded.value.changed, 'one known device member');
    await navigate('11');
    await wait(() => same(rows(), [localId, '701', '702']), 'live mixed target');
    click(row(localId).querySelector('[data-result-action="play"]'));
    await wait(async () => {
      const state = await player();
      return state.state === 'playing' && state.track?.id === localId && state.position > .12;
    }, 'real native local decoder');
    const pause = await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    let position = pause.position, since = performance.now();
    baseline = await wait(async () => {
      const state = await player();
      check(state.state === 'paused' && state.playId === pause.playId && state.entryId === pause.entryId,
        'paused baseline keeps decoder');
      if (state.position !== position) { position = state.position; since = performance.now(); }
      return performance.now() - since >= 420 && state;
    }, 'stable native clock');
    queue = (await invoke('player_queue', { epoch })).tracks.map((entry) => entry.track.id);
    check(same(queue, [localId, '701', '702']), 'only original tracks are queued');
    await navigate('12');
    await wait(() => same(rows(), ['703', '704']), 'ordinary source tracks');
    step = 'choose first';
    await capture('ux_01_source', { point: point('703') });
    click(await wait(menuItem, 'song context menu'));
    await wait(() => enabled(target()) && target(), 'choose mixed destination');
    await capture('ux_02_picker');
    click(target());
    await wait(() => status()?.dataset.phase === 'uncertain' && status().textContent.includes('收藏结果待确认'),
      'lost acknowledgement remains uncertain');
    await capture('ux_03_uncertain');
    click(picker().querySelector('[aria-label="关闭歌单操作"]'));
    await wait(() => !one('dialog.playlist-collect-dialog'), 'close prior picker');
    step = 'choose different song';
    await capture('ux_04_earlier', { point: point('704') });
    click(await wait(menuItem, 'second song context menu'));
    await wait(() => enabled(target()) && target(), 'same mixed destination');
    click(target());
    const verify = await wait(() => {
      if (!status()?.textContent.includes('先前的收藏待确认')) return;
      const action = button('核对结果', status());
      return enabled(action) && action;
    }, 'earlier operation distinguished and readback enabled');
    click(verify);
    await wait(() => status()?.dataset.phase === 'confirmed' &&
      status().textContent.includes('本次未提交'), 'earlier confirmation does not close second picker');
    await capture('ux_05_readback');
    click(picker().querySelector('[aria-label="关闭歌单操作"]'));
    await wait(() => !one('dialog.playlist-collect-dialog'), 'close second picker');
    await navigate('11');
    await wait(() => same(rows(), ['703', localId, '701', '702']), 'confirmed complete mixed order');
    check(same(context, [['703', true], ['704', true]]), 'trusted context gestures for distinct songs');
    await capture('ux_06_target');
    await report('complete', { ok: true, localId, context });
  } catch (error) {
    await report('failed', { error: String(error?.message ?? error), step, rows: rows(), context,
      pickerBusy: picker()?.getAttribute('aria-busy'), status: status()?.textContent.trim() });
  }
})();
