// Production local-library and playlist UI, with a closed native fixture.
(async () => {
  if (window.__mixedPlaylistStarted) return;
  window.__mixedPlaylistStarted = true;
  const one = selector => document.querySelector(selector);
  const all = selector => [...document.querySelectorAll(selector)];
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const report = (stage, details = {}) =>
    invoke('plugin:event|emit', { event: 'local-proof', payload: { stage, ...details } });
  const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
  const require = (ok, message) => { if (!ok) throw Error(message); };
  async function wait(get, label) {
    const end = performance.now() + 16000;
    while (performance.now() < end) {
      const value = await get();
      if (value) return value;
      await delay(60);
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
  const opaque = 'a10c0000000000000000000000000001';
  const title = '合成混排本地曲目';
  const remote = ['701', opaque, '702'];
  const rows = () => all('.track-row[data-track-id]').map(row => row.dataset.trackId);
  const playlistRow = id => one('.track-row[data-track-id="' + CSS.escape(id) + '"]');
  const dialog = () => one('dialog[open][aria-labelledby="playlist-local-member-title"]');
  const dialogButton = name => [...(dialog()?.querySelectorAll('button') ?? [])]
    .find(button => button.textContent.trim() === name);
  const cancel = () => dialogButton('取消');
  const choice = () => dialog()?.querySelector('[aria-label="选择本地曲目目标歌单"]');
  let step = 'initialize';
  async function capture(stage, details = {}) {
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    require(document.documentElement.scrollWidth <= innerWidth, 'no horizontal window overflow');
    await report(stage, details);
    if (window.__localProof?.capture)
      await wait(() => window.__mixedCapture === stage, 'captured ' + stage);
  }
  async function openAction(trigger, action) {
    click(trigger);
    const item = await wait(() => all('.action-menu:not([inert]) [data-menu-key="' + action + '"]')
      .find(button => button.offsetWidth && !button.disabled), 'visible menu action ' + action);
    click(item);
    await wait(() => dialog() && document.activeElement === cancel(), 'modal defaults focus to cancel');
  }
  async function openLibrary() {
    const entry = await wait(() => all('button').find(button => !button.disabled &&
      (button.textContent.trim() === '本地音乐' || button.getAttribute('aria-label') === '本地音乐')),
      'local library navigation');
    click(entry);
    return wait(() => all('.local-tracks tbody tr').find(row => row.querySelector('.local-title')?.textContent.trim() === title),
      'generated file scanned by actual local library');
  }
  async function openPlaylist(ids) {
    let entry = one('[data-sidebar-id="11"]');
    if (!entry) {
      const group = await wait(() => one('[data-sidebar-group="created"] .sidebar-group-toggle'),
        'created playlists navigation');
      group.scrollIntoView({ block: 'nearest', behavior: 'instant' });
      if (group.getAttribute('aria-expanded') === 'false') click(group);
      entry = await wait(() => one('[data-sidebar-id="11"]'), 'owned playlist sidebar row');
    }
    click(entry);
    await wait(() => same(rows(), ids), 'exact playlist order: ' + ids.join(', '));
  }

  try {
    const account = await wait(async () => {
      const state = await invoke('account_snapshot');
      return state.status === 'authenticated' && one('.playerbar') && state;
    }, 'sealed account ready');
    const { epoch } = account;
    const playback = () => invoke('player_snapshot', { epoch });
    const local = await openLibrary();
    const localId = local.dataset.localId;
    require(localId && !remote.includes(localId), 'ordinary local index identity');
    const localMenu = local.querySelector('[data-local-track-menu]');
    step = 'cancel add';
    await openAction(localMenu, 'add_to_playlist');
    require(same([...choice().options].map(option => option.value), ['11']),
      'only the ordinary owned playlist is selectable; favorites and subscriptions omitted');
    require(dialog().textContent.includes('本地歌曲只在此设备上显示'), 'device-local scope is explained');
    await capture('ux_01_add_dialog', { localId });
    click(cancel());
    await wait(() => !dialog() && document.activeElement === localMenu, 'cancel returns local row focus');
    await capture('ux_02_add_cancelled', { localId });

    step = 'add and read';
    await openAction(localMenu, 'add_to_playlist');
    click(dialogButton('添加'));
    await wait(() => !dialog(), 'explicit add saved');
    const mixed = [localId, ...remote];
    await openPlaylist(mixed);
    require(playlistRow(localId)?.textContent.includes(title) &&
      playlistRow(opaque)?.classList.contains('unavailable') &&
      playlistRow(opaque)?.querySelector('[data-result-action="play"]')?.disabled,
      'actual local row is available; unbound legacy native placeholder cannot play');
    await capture('ux_03_mixed_read', { playlist: rows() });

    step = 'play local mixed member';
    click(playlistRow(localId).querySelector('[data-result-action="play"]'));
    const playing = await wait(async () => {
      const state = await playback();
      return state.state === 'playing' && state.track?.id === localId && state.track.local &&
        state.position > .08 && state;
    }, 'actual local libmpv playback advances');
    require(playing.origin?.kind === 'playlist' && playing.origin.id === '11', 'queue belongs to exact playlist');
    const expectedQueue = [localId, '701', '702'];
    const queue = await invoke('player_queue', { epoch });
    require(same(queue.tracks.map(row => row.track.id), expectedQueue) &&
      same(queue.tracks.map(row => row.track.local), [true, false, false]),
      'native mixed queue keeps valid local and catalog order; opaque row remains inert');
    const paused = await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    require(paused.playId === playing.playId && paused.state === 'paused', 'pause exact selected local decoder');
    click(one('.playerbar [aria-label="播放队列"]'));
    await wait(() => {
      const visible = all('.queue-panel .queue-row').map(row => row.textContent);
      return visible.length === 3 && [title, '合成线上曲目 701', '合成线上曲目 702']
        .every((label, index) => visible[index].includes(label));
    }, 'actual queue drawer projects the three playable entries');
    await capture('ux_04_local_playing', { queue: expectedQueue, playId: paused.playId,
      nativePosition: paused.position, queueRevision: paused.queueRevision });
    click(one('[aria-label="关闭播放队列"]'));
    await wait(() => !one('.queue-panel'), 'leave queue drawer');

    step = 'cancel remove';
    const menu = () => playlistRow(localId)?.querySelector('[data-result-action="more"]');
    await openAction(menu(), 'remove_local_member');
    require(dialog().textContent.includes('移除不会删除本地文件'), 'remove dialog explains disk file preservation');
    await capture('ux_05_remove_dialog', { playlist: rows() });
    click(cancel());
    await wait(() => !dialog() && document.activeElement === menu() && same(rows(), mixed),
      'cancel preserves membership and row focus');
    await capture('ux_06_remove_cancelled', { playlist: rows() });

    step = 'remove and recheck original';
    await openAction(menu(), 'remove_local_member');
    click(dialogButton('移除'));
    await wait(() => !dialog() && same(rows(), remote), 'only the local overlay is removed');
    const after = await playback();
    require(after.state === 'paused' && after.playId === paused.playId &&
      after.position === paused.position && after.queueRevision === paused.queueRevision,
      'membership change does not replace already paused playback or the current queue');
    await capture('ux_07_removed', { playlist: rows(), playId: after.playId,
      nativePosition: after.position, queueRevision: after.queueRevision });
    const original = await openLibrary();
    require(original.dataset.localId === localId, 'same local file is still indexed');
    await report('complete', { ok: true, localId, unchangedQueue: true });
  } catch (error) {
    await report('failed', { step, error: String(error), rows: rows(), dialog: dialog()?.innerText?.slice(0, 280) });
  }
})();
