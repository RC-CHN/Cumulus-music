// The selected group comes from trusted XTest Ctrl-clicks and a context click.
(async () => {
  if (window.__mixedRemoveStarted) return;
  window.__mixedRemoveStarted = true;
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const report = (stage, details = {}) =>
    invoke('plugin:event|emit', { event: 'local-proof', payload: { stage, ...details } });
  const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (ok, message) => { if (!ok) throw Error(message); };
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const rows = () => all('.track-scroll .track-row[data-track-id]').map((row) => row.dataset.trackId);
  const row = (id) => one('.track-scroll .track-row[data-track-id="' + CSS.escape(id) + '"]');
  const button = (text, root = document) => [...root.querySelectorAll('button')]
    .find((node) => node.textContent.trim() === text && !node.disabled);
  const menu = () => one('.action-menu:not([inert])');
  const dialog = () => one('dialog[aria-labelledby="mixed-playlist-remove-title"][open]:not([inert])');
  const status = () => one('[data-mixed-remove-status]');
  const sidebar = () => one('[data-sidebar-id="11"]');
  const localNav = () => button('本地音乐') ?? one('button[aria-label="本地音乐"]');
  const createdGroup = () => one('[data-sidebar-group="created"] .sidebar-group-toggle');
  const visible = (node) => node?.isConnected && node.getClientRects().length &&
    getComputedStyle(node).visibility === 'visible' && Number(getComputedStyle(node).opacity) >= .99 &&
    !node.matches('.menu-enter-from,.menu-enter-active,.modal-enter-active') &&
    !node.getAnimations().some((animation) => animation.pending || animation.playState === 'running');
  function menuChoice() {
    const panel = menu();
    if (!visible(panel)) return;
    const choice = button('从歌单中删除', panel);
    if (!choice) return;
    const box = choice.getBoundingClientRect();
    if (choice.contains(document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2)))
      return choice;
  }
  const gestures = [];
  let epoch, localId, paused, queue, step = 'bootstrap';
  const snapshot = () => invoke('player_snapshot', { epoch });
  const receipt = async () => (await invoke('playlist_mixed_remove_receipt', { epoch, id: '11' })).value;
  document.addEventListener('contextmenu', (event) => {
    if (event.target.closest('.track-row')) gestures.push(event.isTrusted);
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
    assert(node?.isConnected && !node.disabled, 'enabled control');
    node.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    node.focus({ preventScroll: true });
    node.click();
  }
  function point(id) {
    const title = row(id)?.querySelector('.track-row-title-text');
    assert(title, 'selected row title is visible');
    const rect = title.getBoundingClientRect();
    const x = rect.left + Math.min(rect.width / 2, 10), y = rect.top + rect.height / 2;
    const hit = document.elementFromPoint(x, y);
    assert(row(id).contains(hit) && !hit.closest('button,a,input,select'), 'X11 point hits row text');
    return { x, y, width: innerWidth, height: innerHeight, ratio: devicePixelRatio };
  }
  async function capture(stage, facts = {}) {
    await Promise.race([
      new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))),
      delay(1500).then(() => { throw Error('paint deadline ' + stage); }),
    ]);
    const state = await snapshot();
    const actualQueue = await invoke('player_queue', { epoch });
    assert(['state', 'playId', 'entryId', 'queueRevision', 'position']
      .every((key) => state[key] === paused[key]) && same(actualQueue.tracks.map((item) => item.track.id), queue),
    'mixed removal changed the paused source or queue');
    await report(stage, { rows: rows(), receipt: await receipt(), state, queue, ...facts });
    await wait(() => window.__mixedCapture === stage, 'capture acknowledgement ' + stage);
  }

  try {
    const account = await wait(async () => {
      const state = await invoke('account_snapshot');
      return state.status === 'authenticated' && one('.playerbar') && state;
    }, 'sealed account');
    epoch = account.epoch;
    click(await wait(localNav, 'local navigation'));
    async function indexedFixture() {
      const page = await invoke('local_page', { query: { keyword: '', sort: 'title', descending: true,
        group: 'list', groupKey: null, includeMissing: true, offset: 0, limit: 200, revision: null } });
      return page.roots.length === 1 && page.roots[0].status === 'ready' && page.tracks[0];
    }
    const indexed = await wait(indexedFixture, 'indexed local fixture');
    localId = indexed.track.id;
    await wait(() => one('.local-tracks tbody tr[data-local-id]')?.dataset.localId === localId,
      'local page shows the exact native fixture');
    step = 'seed exact local member';
    await wait(async () => {
      const current = await indexedFixture();
      if (!current) return false;
      assert(current.track.id === localId && current.revision === indexed.revision,
        'the fixture is still the same indexed file');
      const page = (await invoke('playlist_page', { id: '11', page: { offset: 0, limit: 1 } })).value;
      assert(page.playlist.owned && !page.playlist.favorite && !page.playlistLocal.hasMembers,
        'fixture starts without a local member');
      try {
        const seeded = await invoke('mutate_playlist_local', { epoch, id: '11', revision: page.playlistLocal.revision,
          change: { op: 'add', rows: [{ id: localId, revision: indexed.revision }] } });
        assert(seeded.epoch === epoch && seeded.value.changed, 'one known device member seeded once');
        return true;
      } catch (error) {
        // Only the native pre-commit lock rejection is retryable here. The
        // monitor may start another scan after we observed the root as ready.
        if (error?.code === 'busy') return false;
        throw error;
      }
    }, 'local index lock is available for the fixture');
    step = 'paused mixed playback baseline';
    const group = await wait(createdGroup, 'created sidebar');
    if (group.getAttribute('aria-expanded') === 'false') click(group);
    group.closest('header').scrollIntoView({ block: 'start', behavior: 'instant' });
    click(await wait(sidebar, 'own playlist'));
    await wait(() => one('.track-scroll.playlist-reorder-scroll') && same(rows(), [localId, '701', '702']), 'live mixed table');
    click(row(localId).querySelector('[data-result-action="play"]'));
    await wait(async () => {
      const state = await snapshot();
      return state.state === 'playing' && state.track?.id === localId && state.position > .12;
    }, 'real local decoder');
    const pause = await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    let position = pause.position, since = performance.now();
    paused = await wait(async () => {
      const state = await snapshot();
      assert(state.state === 'paused' && state.playId === pause.playId && state.entryId === pause.entryId,
        'paused local baseline keeps its decoder');
      if (state.position !== position) { position = state.position; since = performance.now(); }
      return performance.now() - since >= 420 && state;
    }, 'stable native clock');
    queue = (await invoke('player_queue', { epoch })).tracks.map((item) => item.track.id);
    assert(queue.includes(localId), 'native queue contains removed local member');
    step = 'select';
    await capture('ux_01_selection_ready', { points: [point('701'), point(localId)] });
    await wait(menuChoice, 'painted mixed group remove menu');
    assert(gestures.length === 1 && gestures[0] && row('701').getAttribute('aria-selected') === 'true' &&
      row(localId).getAttribute('aria-selected') === 'true', 'trusted selection includes both types');
    await capture('ux_02_remove_menu', { gestures: [...gestures] });
    click(menuChoice());
    await wait(() => visible(dialog()) && dialog().textContent.includes('选中的 2 首'), 'explicit group confirmation');
    assert(document.activeElement === button('取消', dialog()), 'confirmation defaults to Cancel');
    await capture('ux_03_confirm');
    click(button('取消', dialog()));
    await wait(() => !one('dialog[aria-labelledby="mixed-playlist-remove-title"]'), 'cancel transition completes');
    assert(!await receipt() && same(rows(), [localId, '701', '702']), 'cancel changes nothing');
    await capture('ux_04_cancelled', { points: [point(localId)] });
    click(await wait(menuChoice, 'reopen original mixed selection'));
    await wait(() => visible(dialog()) && dialog().textContent.includes('选中的 2 首'), 'new explicit confirmation');
    step = 'lost acknowledgement';
    click(button('确认删除', dialog()));
    await wait(() => status()?.textContent.includes('删除结果待确认') &&
      button('核对结果', status()), 'pending deletion visible');
    assert((await receipt()).phase === 'uncertain' && same(rows(), [localId, '701', '702']),
      'lost response does not optimistically remove rows');
    await capture('ux_05_uncertain');
    step = 'leave and reopen';
    click(await wait(localNav, 'current local navigation'));
    await wait(() => one('.local-tracks') && !status(), 'leave original playlist visit');
    const reopenedGroup = await wait(createdGroup, 'current created sidebar');
    if (reopenedGroup.getAttribute('aria-expanded') === 'false') click(reopenedGroup);
    reopenedGroup.closest('header').scrollIntoView({ block: 'start', behavior: 'instant' });
    click(await wait(sidebar, 'same owned playlist on return'));
    await wait(() => status()?.textContent.includes('删除结果待确认') && same(rows(), [localId, '702']),
      'read-only reopen shows the saved pending removal and unchanged device membership');
    await capture('ux_06_reopened');
    step = 'readback';
    click(button('核对结果', status()));
    await wait(() => status()?.textContent.includes('已从歌单中删除。') && same(rows(), ['702']),
      'explicit readback removes the device membership and preserves the remaining online row');
    assert((await receipt()).evidence === 'changed_readback' && same(gestures, [true, true]),
      'read-only result and exact trusted context gestures');
    await capture('ux_07_readback');
    await report('complete', { ok: true, localId, gestures });
  } catch (error) {
    await report('failed', { error: String(error?.message ?? error), step, rows: rows(), gestures });
  }
})();
