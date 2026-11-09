// Production WebView queue with two real publications of one song and one episode.
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
  const button = (label, root = document) => all('button', root).find(
    node => node.getAttribute('aria-label') === label || node.textContent.trim() === label,
  );
  function click(node) {
    assert(node?.isConnected && !node.disabled && !node.closest('[inert]'), 'connected enabled button');
    node.focus({ preventScroll: true });
    node.click();
  }
  const menu = () => one('.action-menu[aria-label="播放列表歌曲操作"],.action-menu[aria-label="播放列表声音操作"]');
  const option = label => button(label, menu());
  const locate = () => option('打开文件所在目录');
  const row = id => one(`.queue-row[data-entry-id="${CSS.escape(id)}"]`);
  async function show(id, context = false) {
    const anchor = await wait(() => one('.queue-row-more', row(id)), 'downloaded More trigger');
    if (!context) click(anchor);
    else {
      const title = one('.queue-song', row(id));
      const box = title.getBoundingClientRect();
      title.focus({ preventScroll: true });
      title.dispatchEvent(new MouseEvent('contextmenu', {
        bubbles: true, cancelable: true, button: 2, clientX: box.x + 4, clientY: box.y + 4,
      }));
    }
    await wait(() => locate() && !locate().disabled, 'downloaded file menu');
    return anchor;
  }
  async function close(anchor) {
    await wait(() => !menu() && document.activeElement === anchor, 'downloaded menu focus restored');
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
    await wait(() => !one('.menu-enter-active,.menu-leave-active,.page-enter-active,.page-leave-active'),
      'painted queue action');
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    window.__queueUiStageRecorded = null;
    await emit(stage, { captureHandshake: true });
    await wait(() => window.__queueUiStageRecorded === stage, 'exact native capture acknowledgement');
  }
  const query = category => ({
    tab: 'completed', category, keyword: '', sort: 'completed', descending: true, offset: 0, limit: 20,
  });
  const queueArgs = (revision, entryId) => ({ epoch, revision, entryId });
  const command = 'player_queue_open_download_directory';
  async function rejected(args, code) {
    try { await invoke(command, args); }
    catch (error) { assert(error?.code === code, JSON.stringify(error)); return error.code; }
    throw Error('unbound queue receipt accepted');
  }
  try {
    await wait(async () => {
      const account = await invoke('account_snapshot');
      if (account.status !== 'authenticated' || button('打开设置')?.disabled !== false || !one('.track-row')) return false;
      epoch = account.epoch;
      return true;
    }, 'synthetic account active');
    phase = 'publications';
    await fixture('seed_queue_download_reveal');
    const songQuery = query('song');
    const { tasks: songs } = await invoke('downloads', { epoch, query: songQuery });
    const [episode] = (await invoke('downloads', { epoch, query: query('voice') })).tasks;
    const original = songs.find(task => task.path?.endsWith('queued original 合成.wav'));
    const alternate = songs.find(task => task.path?.endsWith('alternate same song.wav'));
    assert(songs.length === 2 && original?.track.id === alternate?.track.id && episode?.track.voiceId === '701',
      'real exact original, same-song alternate, and own episode publication');
    await invoke('play_selection', { epoch, selection: { kind: 'downloads', query: songQuery, start_id: original.id } });
    const playing = await wait(async () => {
      const state = await player();
      return state.state === 'playing' && state.downloadTaskId === original.id && state.position > .2 && state;
    }, 'real native original download clock');
    await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    const old = await queue();
    await invoke('player_action', { epoch, action: { kind: 'queue_downloads',
      task_ids: [episode.id], expected_queue_revision: old.revision } });
    let position;
    const paused = await wait(async () => {
      const value = await player();
      if (value.state !== 'paused' || value.playId !== playing.playId || value.queueLen !== 3) return false;
      const stable = value.position === position;
      position = value.position;
      return stable && value;
    }, 'paused downloaded instance with noncurrent voice');
    const before = await queue();
    const current = before.tracks.find(item => item.entryId === before.current);
    const voice = before.tracks.find(item => item.downloadMenu === 'voice');
    const other = before.tracks.find(item => item.downloadMenu === 'song' && item !== current);
    assert(current?.downloadMenu === 'song' && current.collectible && other && voice?.track.voiceId === '701' &&
      before.tracks.every(item => !item.localMenu && !item.catalogMenu && !item.voiceMenu),
      'native types remain distinct from online voice and catalog');
    assert(!JSON.stringify(before).includes('queued original') && !JSON.stringify(before).includes(original.id),
      'queue view contains no physical path or task authority');
    click(button('播放队列'));
    const songMore = await show(current.entryId);
    assert(option('播放') && option('收藏') && option('来自：下载的单曲') &&
      !option('下载') && !option('删除下载'), 'downloaded-song actions and source');
    await capture('ux_01_exact_song_menu');
    click(locate());
    await close(songMore);
    await fixture('observe_queue_download_shell', { count: 1 });

    const voiceMore = await show(voice.entryId, true);
    assert(option('播放') && option('来自：下载的声音') && !option('收藏') &&
      !all('[role="menuitem"]', menu()).some(item => /^(歌手|专辑|主播|下载|删除下载)/.test(item.textContent.trim())),
      'downloaded episode does not inherit catalog or online episode actions');
    await capture('ux_02_exact_voice_menu');
    click(locate());
    await close(voiceMore);
    await fixture('observe_queue_download_shell', { count: 2 });
    await show(voice.entryId);
    click(option('来自：下载的声音'));
    await wait(() => one('#download-tab-voice')?.getAttribute('aria-selected') === 'true' &&
      one('#download-content')?.getAttribute('aria-busy') === 'false', 'own completed voice category');
    await capture('ux_03_voice_download_source');

    const changedMore = await show(current.entryId, true);
    await fixture('change_queue_download_file');
    click(locate());
    await close(changedMore);
    await wait(() => one('.queue-panel .queue-notice')?.textContent.includes('原下载文件已移动、变化或无法读取'),
      'changed original reports its failure without a replacement');
    await capture('ux_04_original_rejected');
    const retained = await player(), unchanged = await queue();
    assert(retained.playId === paused.playId && retained.entryId === paused.entryId &&
      retained.position === paused.position && unchanged.revision === before.revision,
      'read-only locate and source navigation retain the loaded instance and queue');
    const negative = [
      await rejected(queueArgs(before.revision, original.id), 'invalid_input'),
      await rejected(queueArgs('0', current.entryId), 'snapshot_changed'),
      await rejected({ ...queueArgs(before.revision, current.entryId), epoch: epoch + '0' }, 'stale'),
    ];
    await emit('complete', { ok: true, negative, samePlay: true, realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', { ok: false, phase, error: String(error), stack: error?.stack });
  }
})();
