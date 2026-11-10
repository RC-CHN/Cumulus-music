// Catalog rows keep their selected original local file and native playback.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])];
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, payload = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...payload },
  });
  const assert = (condition, message) => { if (!condition) throw Error(message); };
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  async function wait(read, label, timeout = 16000) {
    const end = performance.now() + timeout;
    while (performance.now() < end) {
      const result = await read();
      if (result) return result;
      await sleep(50);
    }
    throw Error(label);
  }
  const button = label => all('button').find(
    node => node.getAttribute('aria-label') === label || node.textContent.trim() === label,
  );
  function click(node) {
    assert(node?.isConnected && !node.disabled && !node.closest('[inert]'), 'connected enabled button');
    node.focus({ preventScroll: true });
    node.click();
  }
  const menu = () => one('.action-menu[aria-label="播放列表歌曲操作"]');
  const option = key => one(`[data-menu-key="${key}"]`, menu());
  const row = id => one(`.queue-row[data-entry-id="${CSS.escape(id)}"]`);
  async function show(id, trigger) {
    const anchor = await wait(() => one('.queue-row-more', row(id)), 'catalog More trigger');
    if (trigger === 'more') click(anchor);
    else {
      const title = one('.queue-song', row(id));
      title.focus({ preventScroll: true });
      if (trigger === 'key') title.dispatchEvent(new KeyboardEvent('keydown', {
        key: 'F10', shiftKey: true, bubbles: true, cancelable: true,
      }));
      else {
        const box = title.getBoundingClientRect();
        title.dispatchEvent(new MouseEvent('contextmenu', {
          bubbles: true, cancelable: true, button: 2, clientX: box.x + 4, clientY: box.y + 4,
        }));
      }
    }
    await wait(() => option('comments')?.textContent.includes('9'), 'catalog menu for the selected song');
    return anchor;
  }
  async function close(anchor) {
    await wait(() => !menu() && document.activeElement === anchor, 'catalog menu focus restored');
  }
  async function fixture(mode, payload = {}) {
    window.__downloadPageAck = null;
    await emit('download-page-fixture', { mode, ...payload });
    const ack = await wait(() => window.__downloadPageAck, 'fixture ' + mode, 30000);
    assert(ack.ok, ack.message);
  }
  let phase = 'boot', epoch;
  const player = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  async function capture(stage) {
    phase = stage;
    await wait(() => !one('.menu-enter-active,.menu-leave-active'), 'painted menu');
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    window.__queueUiStageRecorded = null;
    await emit(stage, { captureHandshake: true });
    await wait(() => window.__queueUiStageRecorded === stage, 'exact native capture acknowledgement');
  }
  try {
    await wait(async () => {
      const account = await invoke('account_snapshot');
      if (account.status !== 'authenticated' || button('打开设置')?.disabled !== false || !one('.track-row')) return false;
      epoch = account.epoch;
      return true;
    }, 'synthetic account active');
    phase = 'publications';
    await fixture('seed_queue_collection');
    await invoke('play_selection', { epoch, selection: {
      kind: 'songs', ids: ['7', '8', '9'], index: 2, quality: 'standard',
    } });
    const playing = await wait(async () => {
      const state = await player();
      return state.state === 'playing' && state.resourceKind === 'catalog' && state.track?.id === '9' && state.position > .2 && state;
    }, 'real native catalog clock');
    await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    let position;
    const paused = await wait(async () => {
      const value = await player();
      if (value.state !== 'paused' || value.playId !== playing.playId) return false;
      const stable = value.position === position;
      position = value.position;
      return stable && value;
    }, 'settled native pause');
    const before = await queue();
    const [both, mapped, ambiguous] = ['7', '8', '9'].map(id => before.tracks.find(item => item.track.id === id));
    assert(both && mapped && ambiguous && before.tracks.every(item => item.catalogMenu &&
      !item.localMenu && !item.voiceMenu && !item.downloadMenu), 'ordinary catalog types');
    const args = entry => ({ epoch, revision: before.revision, entryId: entry.entryId });
    const previous = await invoke('player_queue_catalog_menu', args(both));
    assert(previous.downloaded && /^[0-9a-f]{32}$/.test(previous.directoryReceipt) &&
      !Object.hasOwn(previous, 'downloadTaskId') && !JSON.stringify(previous).includes('.wav'),
      'the native menu exposes only its one-use token');
    click(button('播放队列'));
    const songMore = await show(both.entryId, 'more');
    assert(option('directory') && !option('download'), 'completed download takes precedence over mapped audio');
    let oldRejected = false;
    try {
      await invoke('player_queue_open_catalog_directory', { ...args(both), receipt: previous.directoryReceipt });
    } catch (error) { oldRejected = error?.code === 'snapshot_changed'; }
    assert(oldRejected, 'previous menu token must not displace the currently displayed menu');
    await capture('ux_01_catalog_download_preferred');
    click(option('directory'));
    await close(songMore);
    await fixture('observe_queue_download_shell', { count: 1 });

    const mappedMore = await show(mapped.entryId, 'key');
    assert(option('directory') && option('download'), 'a mapped file is not a completed download');
    await capture('ux_02_catalog_mapped_original');
    click(option('directory'));
    await close(mappedMore);
    await fixture('observe_queue_download_shell', { count: 2 });

    const ambiguousMore = await show(ambiguous.entryId, 'right');
    assert(!option('directory') && option('download'), 'never guess which mapped original to open');
    await capture('ux_03_catalog_ambiguous');
    document.activeElement.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await close(ambiguousMore);

    const changedMore = await show(both.entryId, 'more');
    assert(option('directory'), 'exact download source captured before replacement');
    await fixture('change_queue_catalog_file');
    click(option('directory'));
    await close(changedMore);
    await wait(() => one('.queue-panel .queue-notice')?.textContent.includes('关联的原文件已移动、变化或无法读取'),
      'changed download reports failure instead of opening the mapped alternative');
    await capture('ux_04_catalog_original_rejected');
    const retained = await player(), unchanged = await queue();
    assert(retained.playId === paused.playId && retained.entryId === paused.entryId &&
      retained.position === paused.position && unchanged.revision === before.revision,
      'file selection must retain catalog playback and queue');
    await emit('complete', { ok: true, samePlay: true, oldRejected, realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', { ok: false, phase, error: String(error), stack: error?.stack });
  }
})();
