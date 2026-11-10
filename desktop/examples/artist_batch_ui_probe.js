// Artist catalog fixture: inspect selected handoffs without submitting writes.
(async () => {
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, facts = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...facts } });
  const one = (selector) => document.querySelector(selector);
  const button = (text, root = document) => [...root.querySelectorAll('button')]
    .find((node) => node.textContent.trim() === text);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const paint = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const queueButton = '添加所选至播放列表';
  async function wait(test, label) {
    for (let index = 0; index < 210; index++) {
      const result = await test();
      if (result) return result;
      await sleep(70);
    }
    throw Error(typeof label === 'function' ? label() : label);
  }
  async function capture(stage, facts) {
    await paint();
    await emit(stage, facts);
    await wait(() => window.__CUMULUS_ALBUM_CAPTURE_ACK__ === stage, 'capture ' + stage);
  }
  function assertFacts(label, checks) {
    const changed = Object.fromEntries(Object.entries(checks)
      .filter(([, [actual, expected]]) => actual !== expected)
      .map(([key, [actual, expected]]) => [key, { actual: actual ?? null, expected: expected ?? null }]));
    if (Object.keys(changed).length) throw Error(`${label}: ${JSON.stringify(changed)}`);
  }
  const pausedIdentity = (snapshot, baseline) => ({
    state: [snapshot.state, 'paused'],
    playId: [snapshot.playId, baseline.playId],
    entryId: [snapshot.entryId, baseline.entryId],
    trackId: [snapshot.track?.id, baseline.track?.id],
  });
  try {
    (await wait(() => one('.track-credit-links button'), 'synthetic artist credit')).click();
    await wait(() => one('.catalog-heading h1')?.textContent === '测试歌手', 'artist page');
    button('查看全部歌曲 →').click();
    const more = await wait(
      () => one('button[aria-label="更多歌手歌曲操作"]:not(:disabled)'), 'all-songs menu',
    );
    const { epoch } = await invoke('account_snapshot');
    const before = await invoke('player_snapshot', { epoch });
    async function open() {
      more.focus();
      more.click();
      (await wait(() => {
        const menu = one('.action-menu:not([inert])');
        return menu && button('批量操作', menu);
      }, 'artist batch action')).click();
      return wait(() => one('dialog.artist-track-batch[open]'), 'batch dialog');
    }
    let batch = await open();
    if (['播放所选歌曲', queueButton, '添加到歌单', '下载所选歌曲'].some((name) => !button(name, batch)?.disabled))
      throw Error('empty selection allowed an operation');
    const rows = batch.querySelectorAll('[data-batch-index]');
    if (rows.length < 3 || rows.length > 20) throw Error('bounded loaded track selection');
    const names = [rows[0], rows[2]].map((row) => row.querySelector('.batch-track-name').textContent.trim());
    [rows[0], rows[2]].forEach((row) => row.querySelector('input').click());
    await wait(() => batch.textContent.includes('已选 2 首'), 'two selected songs');
    await sleep(450);
    await emit('ux_artist_batch_selected', { count: 2 });
    await sleep(400);
    button('下载所选歌曲', batch).click();
    const download = await wait(() => one('dialog.download-dialog[open]'), 'download handoff');
    if (!download.querySelector('h2').textContent.includes('下载 2 首歌曲') ||
        download.querySelector('.download-selection').title !== names.join('、'))
      throw Error('download contains songs outside the selection');
    await sleep(450);
    await emit('ux_artist_batch_download', { count: 2 });
    await sleep(400);
    download.dispatchEvent(new Event('cancel', { cancelable: true }));
    await wait(() => !one('.download-dialog'), 'cancel download before submission');
    batch = await open();
    batch.querySelector('[data-batch-index="1"] input').click();
    const chosen = batch.querySelector('[data-batch-index="1"] .batch-track-name').textContent.trim();
    (await wait(() => {
      const action = button('添加到歌单', batch);
      return action && !action.disabled && action;
    }, 'selected collect action')).click();
    const collect = await wait(() => one('dialog.playlist-collect-dialog[open]'), 'collect handoff');
    if (!collect.textContent.includes('收藏' + chosen)) throw Error('collect changed selection');
    collect.dispatchEvent(new Event('cancel', { cancelable: true }));
    await wait(() => !one('.playlist-collect-dialog'), 'cancel collect before submission');
    const after = await invoke('player_snapshot', { epoch });
    if (after.playId !== before.playId || after.queueRevision !== before.queueRevision)
      throw Error('batch preparation changed native playback');
    const baseline = await invoke('play_selection', {
      epoch, selection: { kind: 'songs', ids: ['5', '7'], index: 0, quality: 'standard' },
    });
    if (baseline.queueLen !== 2) throw Error('distinct playback baseline');
    batch = await open();
    // Select in reverse click order: playback follows the displayed track order.
    for (const index of [2, 0]) batch.querySelector(`[data-batch-index="${index}"] input`).click();
    (await wait(() => {
      const action = button('播放所选歌曲', batch);
      return action && !action.disabled && action;
    }, 'selected play action')).click();
    await wait(async () => {
      if (one('.playerbar')?.dataset.state !== 'playing') return false;
      const queue = await invoke('player_queue', { epoch });
      return queue.tracks.map((entry) => entry.track.id).join(',') === '1,3' &&
        queue.current === queue.tracks[0].entryId;
    }, 'native playback starts at the first of exactly two selected songs');
    await invoke('play_selection', {
      epoch, selection: { kind: 'songs', ids: ['91', '3', '1', '92'], index: 1, quality: 'standard' },
    });
    const playing = await wait(async () => {
      const snapshot = await invoke('player_snapshot', { epoch });
      return snapshot.state === 'playing' && snapshot.track?.id === '3' && snapshot.position > .35 &&
        snapshot.playId && snapshot.entryId && snapshot;
    }, 'loaded queue baseline');
    const pauseReply = await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    assertFacts('artist pause did not retain the loaded song', pausedIdentity(pauseReply, playing));
    // Adjacent UI samples can both precede the native 200 ms clock poll.
    // Settle the original decoder across two full polls before mutating the queue.
    let observed = pauseReply;
    let stableSince = performance.now();
    let stableFor = 0;
    const paused = await wait(async () => {
      const snapshot = await invoke('player_snapshot', { epoch });
      assertFacts('artist paused baseline changed transport', pausedIdentity(snapshot, pauseReply));
      const now = performance.now();
      if (snapshot.position !== observed.position) stableSince = now;
      stableFor = now - stableSince;
      observed = snapshot;
      return snapshot.position > .3 && stableFor >= 420 && snapshot;
    }, () => `native paused baseline clock did not settle: ${JSON.stringify({
      lastPosition: observed.position, stableForMs: stableFor, requiredMs: 420,
    })}`);
    const queueBefore = await invoke('player_queue', { epoch });
    function assertRetained(label, snapshot, queue, duplicateIndex) {
      assertFacts(label, {
        ...pausedIdentity(snapshot, paused),
        position: [snapshot.position, paused.position],
        queueCurrent: [queue.current, queueBefore.current],
        originalSongEntry: [queue.tracks[duplicateIndex]?.entryId, queueBefore.tracks[2].entryId],
      });
    }
    let staleAccepted = true;
    try {
      await invoke('player_action', { epoch, action: {
        kind: 'queue_artist_songs_batch', ids: ['2'], quality: 'standard',
        expected_queue_revision: paused.queueRevision, pages: [{ query: { kind: 'artist_songs', id: '12', offset: 0, limit: 20 }, revision: 'stale' }],
      } });
    } catch {
      staleAccepted = false;
    }
    if (staleAccepted || (await invoke('player_queue', { epoch })).revision !== queueBefore.revision)
      throw Error('stale catalog page changed the queue');
    const prepareSelectedQueue = async () => {
      const dialog = await open();
      for (const index of [3, 1]) dialog.querySelector(`[data-batch-index="${index}"] input`).click();
      await wait(() => dialog.textContent.includes('已选 2 首'), 'reversed selected songs');
      const order = [...dialog.querySelectorAll('footer button')].map((item) => item.textContent.trim());
      if (order.join(',') !== ['播放所选歌曲', queueButton, '下载所选歌曲', '添加到歌单'].join(','))
        throw Error('artist batch action order changed');
      return wait(() => {
        const action = button(queueButton, dialog);
        const animations = dialog.getAnimations();
        return action && !action.disabled && !dialog.inert &&
          !animations.some((animation) => animation.pending || animation.playState === 'running') && action;
      }, 'selected queue action available');
    };
    let queueSelected = await prepareSelectedQueue();
    await capture('ux_artist_batch_queue_selection', { count: 2 });
    queueSelected.click();
    const selectedQueue = await wait(async () => {
      const queue = await invoke('player_queue', { epoch });
      return queue.tracks.map((entry) => entry.track.id).join(',') === '91,3,2,4,1,92' && queue;
    }, 'only selected songs insert in page order after the current song');
    const selectedPaused = await invoke('player_snapshot', { epoch });
    assertRetained('selected artist queue changed paused playback or row identity',
      selectedPaused, selectedQueue, 4);
    await wait(() => document.activeElement === more, 'selected queue restores the exact More trigger');
    await capture('ux_artist_batch_queued', {
      queue: selectedQueue.tracks.map((entry) => entry.track.id),
      position: selectedPaused.position, pausedStableMs: stableFor, retainedPlayId: true, restoredFocus: true,
    });
    more.focus();
    more.click();
    const add = await wait(() => {
      const action = button('添加已加载歌曲至播放列表');
      return action && !action.disabled && action;
    }, 'add loaded artist songs');
    await sleep(450);
    await emit('ux_artist_queue_menu', { available: true });
    await sleep(400);
    add.click();
    const expected = ['91', '3', ...Array.from({ length: 20 }, (_, i) => String(i + 1))
      .filter((id) => id !== '3'), '92'];
    const queueAfter = await wait(async () => {
      const queue = await invoke('player_queue', { epoch });
      return queue.tracks.map((entry) => entry.track.id).join(',') === expected.join(',') && queue;
    }, 'insert exactly the loaded artist page after the current song');
    await wait(() => [...document.querySelectorAll('[role="status"]')]
      .some((node) => node.textContent.includes('已添加到播放列表')), 'queue update feedback');
    const unchanged = await invoke('player_snapshot', { epoch });
    assertRetained('loaded artist queue changed paused playback or row identity', unchanged, queueAfter, 2);
    await invoke('player_action', { epoch, action: {
      kind: 'queue_for', expected_queue_revision: queueAfter.revision, edit: { kind: 'clear' },
    } });
    await wait(async () => !(await invoke('player_queue', { epoch })).tracks.length,
      'clear only the synthetic queue');
    queueSelected = await prepareSelectedQueue();
    queueSelected.click();
    const first = await wait(async () => {
      const player = await invoke('player_snapshot', { epoch });
      const queue = await invoke('player_queue', { epoch });
      return player.state === 'playing' && player.track?.id === '2' && player.position > .2 &&
        queue.tracks.map((row) => row.track.id).join(',') === '2,4' && player;
    }, 'empty queue starts only the first of the selected songs');
    await capture('ux_artist_batch_queue_empty', { playing: true, position: first.position });
    await emit('complete', {
      ok: true, artistBatchVerified: true, selectedPlayback: true, loadedQueueInsertion: true,
      selectedQueueInsertion: true, selectedEmptyQueueStarted: true, writesSubmitted: false,
    });
  } catch (error) {
    await emit('failed', { error: String(error?.message ?? error) });
  }
})();
