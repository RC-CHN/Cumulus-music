// Actual cached playlist UI with a native ledger and no network media source.
(async () => {
  if (window.__offlinePlaylistDownloadStarted) return;
  window.__offlinePlaylistDownloadStarted = true;
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
  const rowPlay = id => row(id)?.querySelector('[data-result-action="play"]');
  const status = () => one('.playlist-offline-status');
  const retry = () => status()?.querySelector('button');
  const enabled = () => rows().filter(id => rowPlay(id)?.disabled === false);
  const headerPlay = () => one('.playlist-actions .offline-playlist-play');
  const own = '701';
  const foreign = '702';
  const legacy = 'a10c0000000000000000000000000001';
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
    check(node?.isConnected && !node.disabled && node.getAttribute('aria-disabled') !== 'true',
      'visible enabled control');
    node.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    node.focus({ preventScroll: true });
    node.click();
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
      'created playlist navigation');
    if (group.getAttribute('aria-expanded') === 'false') click(group);
    group.closest('header').scrollIntoView({ block: 'start', behavior: 'instant' });
    click(await wait(() => one('[data-sidebar-id="11"]'), 'exact synthetic playlist'));
  }
  async function capture(stage, extra = {}) {
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    check(document.documentElement.scrollWidth <= innerWidth, 'bounded horizontal layout');
    await event(stage, { rows: rows(), enabled: enabled(), ...extra });
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
    const queue = async () => (await invoke('player_queue', { epoch })).tracks.map(item => item.track.id);
    const transport = label => one('.playerbar [aria-label="' + label + '"]');
    async function stop() {
      click(await wait(() => {
        const node = transport('停止播放');
        return node && !node.disabled && node;
      }, 'native stop control'));
      await wait(async () => (await player()).state === 'idle', 'native decoder stopped');
    }
    async function playing(id, resourceKind, label) {
      return wait(async () => {
        const v = await player();
        if (v.state !== 'playing' || v.track?.id !== id || v.resourceKind !== resourceKind ||
          v.position <= .10 || !v.playId) return false;
        return { id: v.track.id, resourceKind, position: v.position, durationMs: v.track.durationMs,
          playId: v.playId, queue: await queue() };
      }, label);
    }
    async function changeFile(action) {
      await event('download_file', { action });
      await wait(() => window.__offlineDownloadFile === action, 'file action acknowledged');
    }
    step = 'seed the exact indexed CUE in a disposable playlist';
    await localLibrary();
    const cue = await wait(async () => {
      const page = await invoke('local_page', { query: { keyword: '', sort: 'title', descending: true,
        group: 'list', groupKey: null, includeMissing: true, offset: 0, limit: 100, revision: null } });
      return page.roots.length === 1 && page.roots[0].status === 'ready' &&
        page.tracks.find(item => item.cue && item.track.name === 'Fixture offline CUE');
    }, 'current native CUE identity');
    const local = cue.track.id;
    const expected = [local, own, legacy, foreign];
    const start = (await invoke('playlist_page', { id: '11', page: { offset: 0, limit: 1 } })).value;
    check(start.playlist.owned && !start.playlistLocal.hasMembers, 'clean owned playlist');
    let setupAttempts = 0;
    for (;;) {
      setupAttempts++;
      try {
        const seeded = await invoke('mutate_playlist_local', { epoch, id: '11',
          revision: start.playlistLocal.revision, change: { op: 'add', rows: [{ id: local, revision: cue.revision }] } });
        check(seeded.epoch === epoch && seeded.value.changed, 'exact CUE inserted once');
        break;
      } catch (error) {
        if (error?.kind !== 'busy' || setupAttempts >= 4) throw error;
        await delay(200 * setupAttempts);
      }
    }
    step = 'hydrate and persist complete catalog and CUE order';
    await playlist();
    await wait(() => same(rows(), expected) && !status() && one('.playlist-edit'), 'online complete view');
    const ownName = '合成线上曲目 701';
    check(row(own).textContent.includes(ownName) && row(foreign).textContent.includes('合成线上曲目 702'),
      'real catalog-named rows');
    await capture('ux_01_live');

    step = 'transport cut exposes exactly same-account download and CUE';
    await event('mode', { mode: 1 });
    await wait(() => window.__offlineMode === 1, 'transport cut acknowledged');
    await localLibrary();
    await playlist();
    async function ready(wanted) {
      await wait(() => same(rows(), expected) && same(enabled(), wanted) &&
        retry()?.getAttribute('aria-disabled') === 'false' && retry()?.textContent.trim() === '重试联网读取',
      'cached controls and eligibility ' + JSON.stringify(wanted));
      check(status().textContent.includes('本机缓存') && headerPlay()?.textContent.includes('播放可用曲目') &&
        !one('.playlist-edit') && !one('[aria-label="更多歌单操作"]') &&
        !one('[aria-label="歌单评论"]') && !one('.playlist-hero [aria-label="下载当前歌曲列表"]'),
      'cached view remains read-only');
      check(row(own).textContent.includes(ownName) &&
        row(foreign).textContent.includes('合成线上曲目 702') &&
        [own, foreign, legacy, local].every(id => rowPlay(id) && !row(id).querySelector('[data-result-action="more"]')),
      'metadata and disabled controls stay in their original positions');
    }
    await ready([local, own]);
    await capture('ux_02_eligible');

    step = 'play catalog row as native Downloaded, without replacement';
    click(rowPlay(own));
    const downloaded = await playing(own, 'downloaded', 'native downloaded WAV clock advances');
    check(same(downloaded.queue, [local, own]), 'downloaded click only queues eligible rows in playlist order');
    await capture('ux_03_downloaded', { player: downloaded });
    await stop();

    step = 'play indexed CUE in the same cached list';
    click(rowPlay(local));
    const cuePlayback = await playing(local, 'local', 'native CUE audio clock advances');
    check(cuePlayback.durationMs >= 7900 && cuePlayback.durationMs <= 8100 &&
      same(cuePlayback.queue, [local, own]), 'exact second CUE segment and eligible order');
    await capture('ux_04_cue', { player: cuePlayback });
    await stop();

    step = 'the actor rejects same-size tamper before playback';
    await changeFile('damage-digest');
    check(!rowPlay(own).disabled, 'the already displayed row is still a candidate');
    click(rowPlay(own));
    const rejected = await wait(async () => {
      const v = await player();
      check(v.state !== 'playing', 'tampered file began playback');
      return v.state === 'error' && v.error && v.position <= .10 && v.track?.id === own && {
        id: v.track.id, resourceKind: v.resourceKind, state: v.state, position: v.position, error: true,
      };
    }, 'native digest rejection without advancing the clock');
    await capture('ux_05_digest_rejected', { rejected });
    await stop();

    step = 'a length mismatch disables only the owned download on retry';
    await changeFile('damage-length');
    click(retry());
    await ready([local]);
    await capture('ux_06_size_disabled');

    step = 'original bytes restore eligibility on retry';
    await changeFile('restore');
    click(retry());
    await ready([local, own]);
    await capture('ux_07_restored');

    step = 'header playback and next keep native offline order and kinds';
    click(headerPlay());
    const head = await playing(local, 'local', 'native CUE at available playlist start');
    check(same(head.queue, [local, own]), 'header queues only available rows in cached order');
    await capture('ux_08_play_available', { player: head });
    click(await wait(() => {
      const node = transport('下一首');
      return node && !node.disabled && node;
    }, 'next available track control'));
    const next = await playing(own, 'downloaded', 'next resolves to native downloaded WAV');
    check(same(next.queue, [local, own]) && next.playId !== head.playId, 'next retains offline queue and gets a new play');
    await capture('ux_09_next_downloaded', { player: next });
    const frontendEntry = new URL(one('script[type="module"][src]').src).pathname.split('/').pop();
    await event('complete', { ok: true, local, setupAttempts, frontendEntry });
  } catch (error) {
    await event('failed', { step, message: String(error), kind: error?.kind, rows: rows(), enabled: enabled() });
  }
})();
