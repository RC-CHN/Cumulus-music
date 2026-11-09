(async () => {
  const one = selector => document.querySelector(selector);
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, payload = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...payload },
  });
  const assert = (value, message) => { if (!value) throw Error(message); };
  async function wait(read, message) {
    const end = performance.now() + 20000;
    while (performance.now() < end) {
      const result = await read();
      if (result) return result;
      await new Promise(resolve => setTimeout(resolve, 60));
    }
    throw Error(message);
  }
  const action = () => one('.lyrics-panel [data-lyric-contribution]');
  const openError = () => one('.lyrics-panel .lyric-contribution-error');
  const available = label => action()?.textContent.trim() === label && action()?.offsetWidth > 0;
  let epoch, count = 0;
  const playback = () => invoke('player_snapshot', { epoch });
  async function select(song) {
    await invoke('play_selection', {
      epoch, selection: { kind: 'songs', ids: [song], index: 0, quality: 'standard' },
    });
    const current = await wait(async () => {
      const state = await playback();
      return state.state === 'playing' && state.track?.id === song && state;
    }, 'actual native playback ' + song);
    await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    await wait(async () => (await playback()).state === 'paused', 'paused native playback');
    if (!one('.lyrics-panel')) one('.playerbar [aria-label="歌词"]').click();
    return current.playId;
  }
  async function capture(name, song) {
    await wait(() => !one('.lyrics-panel-enter-active,.lyrics-panel-leave-active'), 'settled lyrics');
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const current = await playback();
    assert(current.state === 'paused' && current.track?.id === song, 'same paused native song');
    const stage = 'ux_' + String(++count).padStart(2, '0') + '_' + name;
    await emit(stage, { captureHandshake: true, song, playId: current.playId });
  }
  async function rejected(playId) {
    try { await invoke('lyric_contribution_open', { epoch, playId }); }
    catch { return; }
    throw Error('unavailable contribution opened');
  }
  try {
    const account = await wait(async () => {
      const value = await invoke('account_snapshot');
      return value.status === 'authenticated' && one('.playerbar') && value;
    }, 'sealed signed-in account');
    epoch = account.epoch;
    const first = await select('1');
    await wait(() => available('上传歌词'), 'ordinary empty lyrics upload action');
    await capture('empty_upload', '1');
    action().click();
    await wait(() => openError() && action()?.getAttribute('aria-disabled') === 'false',
      'first shell failure leaves the original retry action');
    await capture('open_failed', '1');
    action().click();
    await wait(() => !openError() && action()?.getAttribute('aria-disabled') === 'false',
      'browser retry succeeds');
    assert((await playback()).playId === first, 'opening browser does not reselect playback');
    const second = await select('2');
    await rejected(first);
    await wait(() => available('上传滚动歌词或翻译'), 'missing scrolling and translation');
    action().click();
    await wait(() => action()?.getAttribute('aria-disabled') === 'false' && !openError(),
      'timed and translation handoff accepted');
    await capture('scroll_and_translation', '2');
    const third = await select('3');
    await rejected(second);
    await wait(() => available('上传翻译'), 'missing translation only');
    await capture('translation_only', '3');
    const pure = await select('4');
    await rejected(third);
    await wait(() => !action() && one('.lyrics-panel .empty-state')?.textContent.includes('纯音乐，请欣赏'),
      'instrumental has no contribution entry');
    await rejected(pure);
    await capture('instrumental', '4');
    const failed = await select('5');
    await wait(() => !action() && one('.lyrics-panel .empty-state')?.textContent.includes('歌词加载失败')
      && one('.lyrics-panel .empty-state button')?.textContent.trim() === '刷新',
      'lyric fetch failure has retry only');
    await rejected(failed);
    await capture('lyric_failed', '5');
    await emit('complete', { ok: true, stageCount: count, staleRejected: true });
  } catch (error) {
    await emit('failed', { error: String(error), stack: error.stack,
      panel: one('.lyrics-panel')?.innerText.slice(0, 400), stageCount: count });
  }
})();
