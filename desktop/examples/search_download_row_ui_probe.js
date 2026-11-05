// Actual search UI, one native publication and two real null-output decodes.
(async () => {
  const query = '合成下载优先';
  const track = '881';
  const invoke = (command, args) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, extra = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...extra } });
  const one = (selector) => document.querySelector(selector);
  const cover = () => one(`[data-result-key="song:${track}"] .search-track-cover`);
  const bar = () => one('.playerbar');
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (value, message) => { if (!value) throw Error(message); };
  async function wait(read, message, count = 200) {
    for (let attempt = 0; attempt < count; attempt++) {
      const value = await read();
      if (value) return value;
      await sleep(65);
    }
    throw Error(message);
  }
  let epoch, step = 'startup', count = 0, last;
  const buttonReady = (node) => node?.isConnected && !node.disabled && node;
  const snapshot = async () => (last = await invoke('player_snapshot', { epoch }));
  const visible = () => one('.search-results-page h1')?.textContent === query &&
    one('#search-result-panel')?.getAttribute('aria-busy') === 'false';
  const synced = (value) => bar()?.dataset.playId === value.playId &&
    bar().dataset.trackId === track && bar().dataset.state === value.state;
  async function control(mode) {
    step = mode;
    window.__searchDownloadFixtureAck = null;
    await emit('search-download-fixture', { mode });
    const receipt = await wait(() => window.__searchDownloadFixtureAck, `${mode} fixture receipt`, 340);
    assert(receipt.ok, receipt.message);
  }
  async function started(quality, departed = null) {
    let first;
    return wait(async () => {
      const state = await snapshot();
      assert(!state.error, `native decode failed: ${state.error}`);
      if (state.state !== 'playing' || state.track?.id !== track || !state.playId ||
          state.playId === departed || !Number.isFinite(state.position) || !synced(state)) return false;
      assert(state.resourceKind === 'catalog' && !state.downloadTaskId && !state.track.local,
        'search must keep catalog identity instead of entering the downloads queue');
      assert(state.requestedQuality === 'standard' && state.actualQuality === quality,
        `catalog search must retain requested standard and observe actual ${quality}`);
      first ??= { playId: state.playId, entryId: state.entryId, position: state.position };
      assert(first.playId === state.playId && first.entryId === state.entryId,
        'native clock must advance on the same play and entry');
      return state.position > first.position + .14 && { first, current: state };
    }, `actual ${quality} clock advances`);
  }
  async function capture(label, value) {
    step = label;
    const stage = `ux_${String(++count).padStart(2, '0')}_${label}`;
    assert(visible() && cover()?.isConnected, 'same actual search results route');
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const localSource = bar()?.querySelector('[data-quality-source="local"]');
    const qualityTrigger = bar()?.querySelector('.quality-control button[aria-label="音质"]');
    await emit(stage, { observed: value, qualitySource: localSource?.textContent.trim() ?? null,
      localSourceTag: localSource?.tagName ?? null, qualityTrigger: !!qualityTrigger,
      qualityTriggerEnabled: !!qualityTrigger && !qualityTrigger.disabled });
    await wait(() => window.__searchDownloadStageRecorded === stage, `native receipt for ${stage}`);
  }
  try {
    ({ epoch } = await wait(async () => {
      const account = await invoke('account_snapshot');
      return account.status === 'authenticated' && buttonReady(one('[aria-label="搜索音乐"]')) && account;
    }, 'synthetic account and actual desktop search ready'));
    await control('publish');
    step = 'search';
    const field = one('[aria-label="搜索音乐"]');
    field.value = query;
    field.dispatchEvent(new Event('input', { bubbles: true }));
    field.form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await wait(() => visible() && buttonReady(cover())?.getAttribute('aria-label') === '播放 合成下载歌曲', 'playable search row');
    const row = cover();
    row.click();
    step = 'local_load';
    const local = await started('lossless');
    await capture('published_local', local);
    await wait(() => buttonReady(cover())?.getAttribute('aria-label') === '暂停 合成下载歌曲', 'same row pause ready');
    cover().click();
    const paused = await wait(async () => {
      const state = await snapshot();
      return state.state === 'paused' && synced(state) && state;
    }, 'same search row toggles to paused');
    assert(paused.playId === local.current.playId && paused.entryId === local.current.entryId &&
      paused.queueRevision === local.current.queueRevision, 'same row paused without replacing local playback');
    await capture('same_local_paused', { current: paused });
    (await wait(() => buttonReady(bar()?.querySelector('[aria-label="停止播放"]')), 'stop control ready')).click();
    await wait(async () => (await snapshot()).state === 'idle', 'private local decoder stopped');
    await control('corrupt');
    await wait(() => visible() && cover() === row && buttonReady(cover())?.getAttribute('aria-label') === '播放 合成下载歌曲', 'same row fallback ready');
    cover().click();
    step = 'fallback_load';
    await capture('corrupt_online_fallback', await started('standard', local.current.playId));
    (await wait(() => buttonReady(bar()?.querySelector('[aria-label="停止播放"]')), 'fallback stop control ready')).click();
    await wait(async () => (await snapshot()).state === 'idle', 'fallback decoder stopped');
    await emit('complete', { ok: true, stageCount: count, realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', { step, count, message: String(error), last, row: cover()?.outerHTML });
  }
})();
