// Production canvas and native palette completion across a same-artwork voice switch.
(async () => {
  const mode = window.__scenePaletteCase;
  const nativeInvoke = window.__TAURI_INTERNALS__.invoke.bind(window.__TAURI_INTERNALS__);
  const calls = { reads: 0, samples: 0, cancels: 0 };
  let releaseCanvas, canvasHeld = false, sampledSize, epoch, count = 0, nonce = 0;
  let core, original;
  const invoke = (...args) => nativeInvoke(...args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const one = (selector, root = document) => root?.querySelector(selector);
  const visible = (node) => node && !node.disabled && !node.closest('[inert]') && node.getClientRects().length;
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (value, label) => { if (!value) throw Error(label); };
  async function wait(read, label) {
    const end = performance.now() + 18000;
    while (performance.now() < end) {
      const value = await read();
      if (value) return value;
      await sleep(25);
    }
    throw Error(label);
  }
  async function click(get, label) {
    const node = await wait(() => { const n = get(); return visible(n) && n; }, label);
    node.focus({ preventScroll: true });
    node.click();
  }
  const card = () => one('[data-podcast-scene-fm] .scene-fm-card');
  const title = () => one('[data-scene-fm-title]', card());
  const player = () => invoke('player_snapshot', { epoch });
  const scene = async () => (await invoke('scene_fm_view', { epoch, refresh: false })).value;
  const bg = () => {
    const node = card();
    return node && { kind: node.dataset.sceneFmBackgroundKind,
      pending: node.dataset.sceneFmPalettePending, image: getComputedStyle(node).backgroundImage };
  };
  async function stats() {
    const current = ++nonce;
    await emit('palette_poll', { nonce: current });
    return wait(() => window.__paletteStatus?.[0] === current && window.__paletteStatus[1],
      'native media observation received');
  }
  async function capture(name, extra = {}) {
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const state = await player(), q = await invoke('player_queue', { epoch }), selected = await scene();
    const stage = `ux_${String(++count).padStart(2, '0')}_${name}`;
    window.__podcastProgressStage = '';
    await emit(stage, { actual: document.visibilityState === 'visible', case: mode,
      background: bg(), ipc: { ...calls }, sampledSize, ...extra,
      scene: { id: selected.episode?.id, cover: selected.episode?.coverUrl },
      state: { state: state.state, voice: state.track?.voiceId, playId: state.playId,
        position: state.position },
      queue: { revision: q.revision, current: q.current,
        voices: q.tracks.map((entry) => [entry.entryId, entry.track.voiceId]) },
    });
    await wait(() => window.__podcastProgressStage === stage, 'native capture acknowledged');
    return state;
  }
  try {
    await wait(async () => {
      const account = await invoke('account_snapshot');
      if (account.status !== 'authenticated') return false;
      epoch = account.epoch;
      return visible(one('input[aria-label="搜索音乐"]'));
    }, 'isolated signed-in shell');
    const provides = one('#app').__vue_app__._context.provides;
    core = provides[Reflect.ownKeys(provides).find((key) => typeof key === 'symbol' && key.description === 'core-client')];
    assert(core, 'real typed frontend core adapter is available');
    original = { read: core.sceneFmPaletteRead, sample: core.sceneFmPaletteSample,
      cancel: core.sceneFmPaletteCancel };
    core.sceneFmPaletteRead = (...args) => { calls.reads++; return original.read.apply(core, args); };
    core.sceneFmPaletteCancel = (...args) => { calls.cancels++; return original.cancel.apply(core, args); };
    core.sceneFmPaletteSample = async (...args) => {
      calls.samples++;
      sampledSize = { width: args[2], height: args[3], bytes: args[4].length };
      if (mode === 'pending_canvas' && !canvasHeld) {
        canvasHeld = true;
        await new Promise((resolve) => { releaseCanvas = resolve; });
      }
      return original.sample.apply(core, args);
    };
    await click(() => one('input[aria-label="搜索音乐"]'), 'open search chart');
    await click(() => one('button[aria-label="播客榜：更多"]'), 'enter classic podcast home');
    await wait(() => card() && bg()?.pending === 'true', 'palette work starts');
    await wait(async () => mode === 'pending_canvas' ? canvasHeld : (await stats()).held,
      'actual selected palette phase is held');
    const firstStats = await stats();
    const expectedRequests = mode === 'pending_header' ? 1 : 2;
    assert(firstStats.urls.length === expectedRequests && calls.reads === 1, 'single original flight');
    if (mode === 'pending_canvas') {
      assert(sampledSize.width === 16 && sampledSize.height === 8 && sampledSize.bytes === 512,
        'the real Blob image and browser canvas produced bounded RGBA before submission');
    }
    await capture('pending_first');
    await click(() => one('button[aria-label="播放随心听FM"]'), 'play actual first episode');
    await wait(async () => {
      const s = await player();
      return s.state === 'playing' && s.track?.voiceId === '601' && s.position > .1;
    }, 'first native decoder advances');
    await click(() => one('button[aria-label="下一首"]'), 'select second episode');
    await wait(async () => {
      const s = await player();
      return s.state === 'playing' && s.track?.voiceId === '602' && title()?.textContent.includes('602');
    }, 'second native decoder and real card agree');
    const selected = await scene();
    let oldVoiceRejected = false;
    try {
      await invoke('scene_fm_palette_read', { epoch, revision: selected.revision,
        voiceId: '601', pixelRatio: 1 });
    } catch (cause) {
      oldVoiceRejected = cause?.code === 'stale';
    }
    assert(oldVoiceRejected, 'new read for old voice still fails native admission');
    const switched = await capture('pending_second', { oldVoiceRejected });
    const heldStats = await stats();
    assert(calls.reads === 1 && calls.cancels === 0 && heldStats.urls.length === expectedRequests &&
      bg().pending === 'true', 'same artwork retains its original in-flight work without a retry');
    if (mode === 'pending_canvas') releaseCanvas();
    else await emit('palette_release');
    const expected = mode === 'pending_header'
      ? 'linear-gradient(245deg,rgb(71,125,179)0%,rgb(71,143,179)48%,rgb(71,125,179)100%)'
      : 'linear-gradient(245deg,rgb(179,107,71)0%,rgb(179,89,71)48%,rgb(179,107,71)100%)';
    await wait(() => bg()?.pending === 'false' && bg().kind === 'gradient' &&
      bg().image.replace(/\s+/g, '') === expected, 'original work finishes for current same-URL voice');
    const completed = await capture('palette_complete');
    assert(completed.playId === switched.playId && completed.position >= switched.position,
      'palette does not reopen or rewind current audio');
    await wait(async () => (await player()).position > completed.position + .15, 'audio keeps advancing');
    await capture('playing_stable');
    assert(calls.reads === 1 && calls.samples === (mode === 'pending_header' ? 0 : 1) &&
      calls.cancels === 0, 'one read and at most one actual sample, no cancellation');
    await emit('complete', { ok: true, case: mode, stageCount: count, ipc: calls,
      sampledSize, oldVoiceRejected, realMusicRequests: 0, realAccountWrites: 0 });
  } catch (cause) {
    await emit('failed', { ok: false, case: mode, error: String(cause), stageCount: count,
      ipc: calls, background: bg(), page: one('.main-content')?.innerText.slice(0, 1200) });
  } finally {
    if (original) {
      core.sceneFmPaletteRead = original.read;
      core.sceneFmPaletteSample = original.sample;
      core.sceneFmPaletteCancel = original.cancel;
    }
  }
})();
