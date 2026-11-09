// Real playlist menus, typed IPC, and one XTest queue gesture; no service traffic.
(async () => {
  if (window.__mixedPlaylistQueueStarted) return;
  window.__mixedPlaylistQueueStarted = true;
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, value = {}) => invoke('plugin:event|emit', { event: 'local-proof', payload: { stage, ...value } });
  const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const require = (ok, message) => { if (!ok) throw Error(message); };
  async function wait(read, label) {
    for (let i = 0; i < 240; i++) {
      const value = await read();
      if (value) return value;
      await delay(50);
    }
    throw Error(label);
  }
  function click(node) {
    require(node?.isConnected && !node.disabled, 'visible enabled control');
    node.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    node.focus({ preventScroll: true });
    node.click();
  }
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const legacy = 'a10c0000000000000000000000000001';
  const audioName = '合成混排本地曲目';
  const anchorName = '独立暂停基准';
  const visibleRows = () => all('.track-scroll .track-row[data-track-id]').map((row) => row.dataset.trackId);
  const row = (id) => one('.track-scroll .track-row[data-track-id="' + CSS.escape(id) + '"]');
  const source = (id) => row(id)?.querySelector('.track-row-title-text');
  const queueViewport = () => one('.queue-scroll');
  const selected = () => all('.track-scroll .track-row[aria-selected="true"]').map((row) => row.dataset.trackId);
  const item = (key) => all('.action-menu:not([inert]) [data-menu-key="' + key + '"]')
    .find((button) => !button.disabled && button.getClientRects().length);
  const libraryRow = (name) => all('.local-tracks tbody tr')
    .find((candidate) => candidate.querySelector('.local-title')?.textContent.trim() === name);
  let step = 'initialize', epoch, baseline, localId, anchorId;
  const state = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });

  function pointerPoint(node) {
    require(node?.isConnected, 'source is in the virtualized table');
    const rect = node.getBoundingClientRect();
    const point = { x: rect.left + Math.min(5, rect.width / 2), y: rect.top + rect.height / 2 };
    require(node.contains(document.elementFromPoint(point.x, point.y)), 'source is reachable in the shown viewport');
    return point;
  }
  function choose(id, ctrlKey = false) {
    const node = source(id);
    node.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    const { x, y } = pointerPoint(node);
    const props = { bubbles: true, cancelable: true, pointerId: 57, pointerType: 'mouse',
      button: 0, ctrlKey, clientX: x, clientY: y };
    node.dispatchEvent(new PointerEvent('pointerdown', { ...props, buttons: 1 }));
    node.dispatchEvent(new PointerEvent('pointerup', { ...props, buttons: 0 }));
    node.dispatchEvent(new MouseEvent('click', props));
  }
  function compact(player, view) {
    return {
      player: { state: player.state, playId: player.playId, entryId: player.entryId,
        position: player.position, id: player.track.id, resourceKind: player.resourceKind },
      queue: { revision: view.revision, current: view.current,
        rows: view.tracks.map(({ entryId, track, origin, localMenu }) =>
          ({ entryId, id: track.id, local: track.local, origin, localMenu: !!localMenu })) },
    };
  }
  async function checkpoint(label, ids, extra = {}) {
    const view = await wait(async () => {
      const value = await queue();
      return same(value.tracks.map(({ track }) => track.id), ids) && value;
    }, label + ': exact native queue IDs');
    const player = await state();
    require(player.queueRevision === view.revision && view.current === player.entryId,
      label + ': native queue and player agree on the active entry');
    require(player.state === 'paused' && player.playId === baseline.playId &&
      player.entryId === baseline.entryId && player.track?.id === anchorId &&
      player.resourceKind === 'local' && player.position === baseline.position && !player.error,
      label + ': paused native file instance and clock stay exact');
    require(same(view.tracks.map(({ track }) => track.local), ids.map((id) => id === localId || id === anchorId)),
      label + ': catalog and native resources retain their original types');
    require(view.tracks.slice(1).every(({ origin }) => origin?.kind === 'playlist' && origin.id === '11'),
      label + ': additions retain the verified mixed playlist origin');
    require(view.tracks.filter(({ track }) => track.id === localId).every((entry) => entry.localMenu),
      label + ': the mixed member has a native local row, not a catalog placeholder');
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    require(document.documentElement.scrollWidth <= innerWidth, 'document fits in the window');
    await emit(label, { ...compact(player, view), playlist: visibleRows(), ...extra });
    if (window.__localProof?.capture)
      await wait(() => window.__mixedCapture === label, 'capture ack ' + label);
    return view;
  }
  async function openRowMenu(id) {
    click(row(id)?.querySelector('[data-result-action="more"]'));
    return wait(() => item('next'), 'enabled next item for ' + id);
  }
  async function openPlaylist() {
    let entry = one('[data-sidebar-id="11"]');
    if (!entry) {
      const group = await wait(() => one('[data-sidebar-group="created"] .sidebar-group-toggle'), 'owned sidebar group');
      if (group.getAttribute('aria-expanded') === 'false') click(group);
      group.closest('header').scrollIntoView({ block: 'start', behavior: 'instant' });
      entry = await wait(() => one('[data-sidebar-id="11"]'), 'owned playlist sidebar row');
    }
    click(entry);
    await wait(() => same(visibleRows(), [localId, '701', legacy, '702']) &&
      !one('.page-enter-active,.page-leave-active'), 'complete mixed table with inert legacy row');
  }

  try {
    ({ epoch } = await wait(async () => {
      const value = await invoke('account_snapshot');
      return value.status === 'authenticated' && one('.playerbar') && value;
    }, 'synthetic signed in account'));
    const library = await wait(() => all('button').find((button) => !button.disabled &&
      (button.textContent.trim() === '本地音乐' || button.getAttribute('aria-label') === '本地音乐')),
      'local library navigation');
    click(library);
    const local = await wait(() => libraryRow(audioName), 'indexed mixed audio file');
    localId = local.dataset.localId;
    anchorId = (await wait(() => libraryRow(anchorName), 'indexed independent playback anchor')).dataset.localId;
    require(localId && anchorId && localId !== anchorId, 'independent indexed local references');
    step = 'add the local member through the production library menu';
    click(local.querySelector('[data-local-track-menu]'));
    click(await wait(() => item('add_to_playlist'), 'local file add action'));
    const dialog = () => one('dialog[open][aria-labelledby="playlist-local-member-title"]');
    click(await wait(() => [...(dialog()?.querySelectorAll('button') ?? [])]
      .find((button) => button.textContent.trim() === '添加' && !button.disabled), 'owned device-local add confirmation'));
    await wait(() => !dialog(), 'local membership save finished');

    step = 'play the independent local anchor';
    const search = one('[aria-label="搜索本地音乐"]');
    search.value = anchorName;
    search.dispatchEvent(new Event('input', { bubbles: true }));
    await wait(() => all('.local-tracks tbody tr').length === 1 && libraryRow(anchorName), 'filter selects only the playback anchor');
    click(libraryRow(anchorName).querySelector('.local-title'));
    const playing = await wait(async () => {
      const player = await state();
      return player.track?.id === anchorId && player.state === 'playing' && player.position > .10 && player;
    }, 'actual libmpv anchor clock advances');
    await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    baseline = await wait(async () => {
      const player = await state();
      return player.state === 'paused' && player.playId === playing.playId && player;
    }, 'paused anchor state has settled');
    await openPlaylist();
    const anchorOnly = [anchorId];
    await checkpoint('ux_01_paused_baseline', anchorOnly);

    step = 'bound local row next';
    const localNext = await openRowMenu(localId);
    require(localNext.textContent.includes('下一首播放'), 'official next label on the bound local menu');
    await checkpoint('ux_02_local_next_menu', anchorOnly);
    click(localNext);
    const afterLocal = [anchorId, localId];
    await checkpoint('ux_03_local_next', afterLocal);

    step = 'online row next in the mixed view';
    const onlineNext = await openRowMenu('702');
    require(onlineNext.textContent.includes('下一首播放'), 'official next label on the catalog row');
    click(onlineNext);
    await checkpoint('ux_04_online_next', [anchorId, '702', localId]);

    step = 'whole mixed playlist from its header';
    click(one('.playlist-hero [aria-label="更多歌单操作"]'));
    const whole = await wait(() => item('queue'), 'enabled whole playlist queue action');
    require(whole.textContent.includes('添加全部至播放列表'), 'official whole playlist menu label');
    click(whole);
    const full = [anchorId, localId, '701', '702'];
    await checkpoint('ux_05_entire_playlist', full);

    step = 'trusted mixed selection drop at a positive queue gap';
    click(one('.playerbar [aria-label="播放队列"]'));
    await wait(() => queueViewport()?.querySelectorAll('.queue-row').length === 4 &&
      !one('.queue-panel-enter-active'), 'settled queue drawer');
    choose('702');
    choose(localId, true);
    await wait(() => same(selected(), [localId, '702']), 'reverse-selected local and catalog rows');
    const target = queueViewport();
    target.scrollTop = 0;
    const box = target.getBoundingClientRect();
    const from = pointerPoint(source(localId));
    const to = { x: box.left + box.width / 2, y: box.top + 198 };
    require(target.contains(document.elementFromPoint(to.x, to.y)), 'queue gap before row 4 is pointer reachable');
    const absolute = (point) => ({ x: Math.floor((screenX + point.x) * devicePixelRatio),
      y: Math.floor((screenY + point.y) * devicePixelRatio) });
    let trustedDown = false, trustedUp = false;
    document.addEventListener('pointerdown', (event) => {
      if (event.isTrusted && source(localId)?.contains(event.target)) trustedDown = true;
    }, { capture: true, once: true });
    document.addEventListener('pointerup', (event) => {
      if (event.isTrusted && queueViewport()?.contains(event.target)) trustedUp = true;
    }, { capture: true, once: true });
    await checkpoint('ux_06_drag_target', full, { selected: selected(), pointer: { from: absolute(from), to: absolute(to) } });
    const marker = await wait(() => {
      const line = one('.queue-drop-marker');
      return line?.style.top === '192px' && line;
    }, 'actual pointer paints the third queue boundary');
    require(trustedDown, 'native WebView received trusted pointer down on the playlist row');
    const rect = marker.getBoundingClientRect();
    await checkpoint('ux_07_drag_boundary', full, { trustedDown, insertBefore: 3, ratio: devicePixelRatio,
      screenX, screenY, paintSamples: [{ x: rect.left + 8, y: rect.top + 1,
        color: getComputedStyle(marker).backgroundColor }] });
    const dropped = [anchorId, '701', '702', localId];
    await checkpoint('ux_08_drag_inserted', dropped, { trustedUp: await wait(() => trustedUp, 'trusted pointer up') });
    require(!one('.queue-drop-marker'), 'drop marker cleaned up');

    step = 'verify original files remain indexed';
    click(one('[aria-label="关闭播放队列"]'));
    await wait(() => !one('.queue-panel'), 'queue drawer closed');
    click(library);
    await wait(() => libraryRow(anchorName)?.dataset.localId === anchorId, 'same anchor file remains indexed');
    await emit('complete', { ok: true, localId, anchorId, trustedDown, trustedUp, states: 8,
      ...compact(await state(), await queue()) });
  } catch (error) {
    await emit('failed', { step, error: String(error), playlist: visibleRows(),
      menus: all('.action-menu button').map((button) => ({ text: button.textContent.trim(), disabled: button.disabled })) });
  }
})();
