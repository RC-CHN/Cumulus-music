// Artist catalog fixture: inspect selected handoffs without submitting writes.
(async () => {
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, facts = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...facts } });
  const one = (selector) => document.querySelector(selector);
  const button = (text, root = document) => [...root.querySelectorAll('button')]
    .find((node) => node.textContent.trim() === text);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  async function wait(test, label) {
    for (let index = 0; index < 210; index++) {
      const result = await test();
      if (result) return result;
      await sleep(70);
    }
    throw Error(label);
  }
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
      (await wait(() => button('批量操作'), 'artist batch action')).click();
      return wait(() => one('dialog.artist-track-batch[open]'), 'batch dialog');
    }
    let batch = await open();
    if (['播放所选歌曲', '添加到歌单', '下载所选歌曲'].some((name) => !button(name, batch).disabled))
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
    await emit('complete', {
      ok: true, artistBatchVerified: true, selectedPlayback: true, writesSubmitted: false,
    });
  } catch (error) {
    await emit('failed', { error: String(error?.message ?? error) });
  }
})();
