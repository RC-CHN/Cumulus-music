// Drive the ordinary playlist route; only the sealed native adapter toggles reachability.
(async () => {
  if (window.__offlinePlaylistStarted) return;
  window.__offlinePlaylistStarted = true;
  const one = selector => document.querySelector(selector);
  const all = selector => [...document.querySelectorAll(selector)];
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const event = (stage, details = {}) =>
    invoke('plugin:event|emit', { event: 'local-proof', payload: { stage, ...details } });
  const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
  const check = (ok, message) => { if (!ok) throw Error(message); };
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const rows = () => all('.track-scroll .track-row[data-track-id]').map(row => row.dataset.trackId);
  const row = id => one('.track-scroll .track-row[data-track-id="' + CSS.escape(id) + '"]');
  const status = () => one('.playlist-offline-status');
  const retry = () => status()?.querySelector('button');
  const opaque = 'a10c0000000000000000000000000001';
  let step = 'initialize';
  async function wait(get, label) {
    const deadline = performance.now() + 16000;
    while (performance.now() < deadline) {
      const value = await get();
      if (value) return value;
      await delay(60);
    }
    throw Error(label);
  }
  function click(node) {
    check(node?.isConnected && !node.disabled, 'visible enabled control');
    node.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    node.focus({ preventScroll: true });
    node.click();
  }
  async function mode(mode) {
    await event('mode', { mode });
    await wait(() => window.__offlineMode === mode, 'synthetic transport mode ' + mode);
  }
  async function localLibrary() {
    const trigger = await wait(() => all('button').find(button => !button.disabled &&
      (button.textContent.trim() === '本地音乐' || button.getAttribute('aria-label') === '本地音乐')),
      'local library navigation');
    click(trigger);
    await wait(() => one('.local-page'), 'real local page');
  }
  async function playlist() {
    const group = await wait(() => one('[data-sidebar-group="created"] .sidebar-group-toggle'),
      'created playlists navigation');
    if (group.getAttribute('aria-expanded') === 'false') click(group);
    group.closest('header').scrollIntoView({ block: 'start', behavior: 'instant' });
    click(await wait(() => one('[data-sidebar-id="11"]'), 'exact created playlist'));
  }
  async function capture(stage, extra = {}) {
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    check(document.documentElement.scrollWidth <= innerWidth, 'bounded horizontal layout');
    await event(stage, { rows: rows(), ...extra });
    if (window.__localProof.capture)
      await wait(() => window.__offlineCapture === stage, 'captured ' + stage);
  }
  try {
    const account = await wait(async () => {
      const view = await invoke('account_snapshot');
      return view.status === 'authenticated' && one('.playerbar') && view;
    }, 'verified synthetic account');
    const { epoch } = account;
    const player = () => invoke('player_snapshot', { epoch });
    step = 'seed an indexed CUE after its scan completes';
    await localLibrary();
    const cue = await wait(async () => {
      const page = await invoke('local_page', { query: { keyword: '', sort: 'title', descending: true,
        group: 'list', groupKey: null, includeMissing: true, offset: 0, limit: 100, revision: null } });
      return page.roots.length === 1 && page.roots[0].status === 'ready' &&
        page.tracks.find(item => item.cue && item.track.name === 'Fixture offline CUE');
    }, 'current native CUE identity');
    const local = cue.track.id;
    const start = (await invoke('playlist_page', { id: '11', page: { offset: 0, limit: 1 } })).value;
    check(start.playlist.owned && !start.playlistLocal.hasMembers, 'clean owned playlist');
    const seeded = await invoke('mutate_playlist_local', { epoch, id: '11', revision: start.playlistLocal.revision,
      change: { op: 'add', rows: [{ id: local, revision: cue.revision }] } });
    check(seeded.epoch === epoch && seeded.value.changed, 'exact CUE membership inserted once');
    const expected = [local, '701', opaque, '702'];
    step = 'hydrate and persist the complete online view';
    await playlist();
    await wait(() => same(rows(), expected) && !status() && one('.playlist-edit'), 'online complete view');
    check(!row('701').querySelector('[data-result-action="play"]').disabled, 'online control enabled');
    await capture('ux_01_live');

    step = 'open last complete snapshot with transport unavailable';
    await mode(1);
    await localLibrary();
    await playlist();
    await wait(() => status() && same(rows(), expected), 'cached view and same order');
    check(status().textContent.includes('本机缓存') && !one('.playlist-edit') &&
      one('.playlist-hero [aria-label="更多歌单操作"]') && !one('[aria-label="歌单评论"]') &&
      !one('.playlist-hero [aria-label="下载当前歌曲列表"]'), 'cached view has no online actions');
    check(['701', opaque, '702'].every(id => row(id).querySelector('[data-result-action="play"]').disabled) &&
      !row(local).querySelector('[data-result-action="play"]').disabled &&
      !row(local).querySelector('[data-result-action="more"]'), 'only the indexed CUE is playable');
    const cachedPlay = one('.playlist-actions .offline-playlist-play');
    check(cachedPlay?.textContent.includes('播放可用曲目'), 'cached play action label');
    const content = document.createRange();
    content.selectNodeContents(cachedPlay);
    const ink = content.getBoundingClientRect();
    const control = cachedPlay.getBoundingClientRect();
    check(ink.left >= control.left + 4 && ink.right <= control.right - 4,
      'cached play label and icon fit the button');
    const cachedMenu = one('.playlist-hero [aria-label="更多歌单操作"]');
    click(cachedMenu);
    const addAll = await wait(() => one('[data-menu-key^="queue-offline:"]'), 'offline-only add all');
    check(addAll.textContent.trim() === '添加全部至播放列表' &&
      addAll.closest('[role="menu"]').querySelectorAll('[role="menuitem"]').length === 1,
      'no online menu actions on the cached detail');
    addAll.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await wait(() => cachedMenu.getAttribute('aria-expanded') === 'false' &&
      !one('[data-menu-key^="queue-offline:"]'), 'cached menu closed');
    await capture('ux_02_cached');

    step = 'play cached CUE through native decoder';
    click(row(local).querySelector('[data-result-action="play"]'));
    const audible = await wait(async () => {
      const v = await player();
      return v.state === 'playing' && v.track?.id === local && v.resourceKind === 'local' && v.position > .08 && v;
    }, 'actual native CUE audio clock advances');
    const queue = await invoke('player_queue', { epoch });
    check(queue.tracks.length === 1 && queue.tracks[0].track.id === local &&
      audible.track.durationMs >= 7900 && audible.track.durationMs <= 8100,
      'only the second eight-second CUE segment was placed in the queue');
    await capture('ux_03_cue', { playId: audible.playId, position: audible.position,
      durationMs: audible.track.durationMs, queue: queue.tracks.map(x => x.track.id) });

    step = 'retry while still disconnected';
    click(retry());
    await wait(() => status() && retry()?.getAttribute('aria-disabled') === 'false' &&
      retry()?.textContent.trim() === '重试联网读取' && same(rows(), expected), 'offline retry settled');
    await capture('ux_04_retry_offline');

    step = 'replace cached view with a fresh response';
    await mode(0);
    click(retry());
    await wait(() => !status() && same(rows(), expected) && one('.playlist-edit') &&
      !row('701').querySelector('[data-result-action="play"]').disabled, 'fresh online controls restored');
    await capture('ux_05_reconnected');

    step = 'hard denial invalidates the previous cache';
    await mode(1);
    await localLibrary();
    await playlist();
    await wait(() => status() && same(rows(), expected), 'second cached view');
    await mode(2);
    click(retry());
    await wait(() => one('.error-state') && !status() && rows().length === 0, 'explicit denial clears cached content');
    await capture('ux_06_denied');
    await mode(1);
    await localLibrary();
    await playlist();
    await wait(() => one('.error-state') && !status() && rows().length === 0, 'a later transport error cannot resurrect the denied playlist');
    await capture('ux_07_no_cache');
    await event('complete', { ok: true, local });
  } catch (error) {
    await event('failed', { step, message: String(error), rows: rows() });
  }
})();
