// Production album batch controls with the catalog fixture and native null audio.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, facts = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...facts } });
  const button = (text, root = document) =>
    [...root.querySelectorAll('button')].find((node) => node.textContent.trim() === text);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  async function wait(check, label) {
    for (let i = 0; i < 210; i++) {
      const value = await check();
      if (value) return value;
      await sleep(70);
    }
    throw Error(label);
  }
  const click = (node) => {
    if (!node || node.disabled) throw Error('unavailable control');
    node.focus({ preventScroll: true });
    node.click();
  };
  async function capture(stage) {
    await wait(() => !one('.modal-enter-active,.modal-leave-active'), 'settled modal');
    await sleep(450);
    const dialog = one('dialog[open]');
    const close = dialog?.querySelector('button[aria-label*="关闭"]');
    const box = close?.getBoundingClientRect();
    if (!box || Number(getComputedStyle(dialog).opacity) !== 1 ||
        !close.contains(document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2)))
      throw Error('modal must be painted and its close control reachable');
    window.__CUMULUS_ALBUM_CAPTURE_ACK__ = '';
    await emit(stage, { activeAlbumTab: 'detail' });
    await wait(() => window.__CUMULUS_ALBUM_CAPTURE_ACK__ === stage, 'capture acknowledgement');
  }
  const modal = () => one('dialog.album-track-batch[open]');
  try {
    click(await wait(() => one('.track-credit-links button'), 'daily artist credit'));
    await wait(() => one('.catalog-heading h1')?.textContent === '测试歌手', 'artist');
    click(one('[data-catalog-tab="albums"]'));
    click(await wait(() => one('.catalog-card'), 'album card'));
    const detail = await wait(() => one('.album-tabs [id$="-details"]'), 'album detail tab');
    click(detail);
    const onDetail = () => detail.getAttribute('aria-selected') === 'true';
    await wait(onDetail, 'selected detail tab');
    const more = one('button[aria-label="更多专辑操作"]');
    const { epoch } = await invoke('account_snapshot');
    const before = await invoke('player_snapshot', { epoch });
    async function open() {
      click(more);
      click(await wait(() => button('批量操作'), 'album batch action'));
      return wait(modal, 'album batch dialog');
    }
    let batch = await open();
    if (!onDetail() || !batch.querySelector('h2')?.textContent.includes('专辑'))
      throw Error('album header must retain its selected tab and album scope');
    for (const action of ['播放所选歌曲', '添加到歌单', '下载所选歌曲']) {
      if (!button(action, batch)?.disabled) throw Error('empty selection must disable actions');
    }
    const rows = [...batch.querySelectorAll('[data-batch-index]')];
    const names = [0, 2].map((i) => rows[i].querySelector('.batch-track-name').textContent.trim());
    for (const i of [2, 0]) click(rows[i].querySelector('input'));
    await wait(() => batch.textContent.includes('已选 2 首'), 'album selection');
    await capture('ux_album_batch_selection');
    click(button('下载所选歌曲', batch));
    const download = await wait(() => one('dialog.download-dialog[open]'), 'album download handoff');
    if (download.querySelector('.download-selection').title !== names.join('、'))
      throw Error('download scope differs from the selection');
    await capture('ux_album_batch_download');
    download.dispatchEvent(new Event('cancel', { cancelable: true }));
    await wait(() => !one('.download-dialog') && document.activeElement === more, 'restore album opener');
    const cancelled = await invoke('player_snapshot', { epoch });
    if (!onDetail() || cancelled.queueRevision !== before.queueRevision)
      throw Error('cancelling changed the album tab or playback');
    batch = await open();
    const collectRow = batch.querySelector('[data-batch-index="1"]');
    const collectName = collectRow.querySelector('.batch-track-name').textContent.trim();
    click(collectRow.querySelector('input'));
    await wait(() => !button('添加到歌单', batch).disabled, 'collect selected is enabled');
    click(button('添加到歌单', batch));
    const collect = await wait(() => one('dialog.playlist-collect-dialog[open]'), 'album collect handoff');
    if (!collect.textContent.includes('收藏' + collectName)) throw Error('collect scope differs');
    collect.dispatchEvent(new Event('cancel', { cancelable: true }));
    await wait(() => !one('.playlist-collect-dialog') && document.activeElement === more,
      'restore album opener after collect');
    batch = await open();
    for (const i of [2, 0]) click(batch.querySelector(`[data-batch-index="${i}"] input`));
    await wait(() => !button('播放所选歌曲', batch).disabled, 'play selected is enabled');
    click(button('播放所选歌曲', batch));
    await wait(async () => {
      if (one('.playerbar')?.dataset.state !== 'playing') return false;
      const queue = await invoke('player_queue', { epoch });
      return queue.tracks.map((entry) => entry.track.id).join(',') === '1,3' &&
        queue.current === queue.tracks[0].entryId;
    }, 'native queue matches album selection');
    if (!onDetail() || modal()) throw Error('playback should not change the album tab');
    await emit('complete', { ok: true, albumBatchVerified: true, writesSubmitted: false });
  } catch (error) {
    await emit('failed', { error: String(error?.message ?? error) });
  }
})();
