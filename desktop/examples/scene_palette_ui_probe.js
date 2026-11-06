// Real production palette IPC, native-supplied PNG, local browser canvas and Rust quantization.
(async () => {
  const one = (s, root = document) => root?.querySelector(s);
  const visible = (node) => node && !node.disabled && !node.closest('[inert]') &&
    node.getClientRects().length > 0;
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (v, label) => { if (!v) throw Error(label); };
  const search = () => one('input[aria-label="搜索音乐"]');
  const navigationReady = () => visible(one('button[aria-label="打开设置"]'));
  const classicHome = () => one('.podcast-discovery .podcast-top[data-layout="classic"]');
  const card = () => one('[data-podcast-scene-fm] .scene-fm-card');
  const normalize = (css) => css.replace(/\s+/g, '');
  const expected = (r, g, b, x, y, z) => normalize(
    `linear-gradient(245deg, rgb(${r}, ${g}, ${b}) 0%, rgb(${x}, ${y}, ${z}) 48%, rgb(${r}, ${g}, ${b}) 100%)`);
  const blue = expected(71, 125, 179, 71, 143, 179);
  const brown = expected(179, 107, 71, 179, 89, 71);
  const red = expected(179, 71, 71, 179, 71, 89);
  let epoch, count = 0;
  const mode = window.__scenePaletteCase;
  async function wait(fn, label) {
    const end = performance.now() + 18000;
    while (performance.now() < end) {
      const value = await fn();
      if (value) return value;
      await sleep(45);
    }
    throw Error(label);
  }
  async function click(get, label) {
    const node = await wait(() => { const n = get(); return visible(n) && n; }, label);
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    node.click();
  }
  function bg() {
    const n = card();
    return n && {
      kind: n.dataset.sceneFmBackgroundKind, pending: n.dataset.sceneFmPalettePending,
      image: getComputedStyle(n).backgroundImage, color: getComputedStyle(n).backgroundColor,
    };
  }
  async function palette(expectedImage) {
    return wait(() => {
      const value = bg();
      if (!value || value.pending !== 'false') return false;
      if (expectedImage) return normalize(value.image) === expectedImage && value.kind === 'gradient';
      return value.image === 'none' && value.kind === 'theme_fallback';
    }, 'official palette result is applied to the rendered card');
  }
  const state = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  async function capture(name) {
    if (window.__CUMULUS_PROOF_INVOKE__) {
      const selector = one('.lyrics-panel') ? '.lyrics-panel' : '.page-view';
      await wait(() => {
        const p = one(selector);
        return p && getComputedStyle(p).opacity === '1' &&
          !p.getAnimations().some((a) => a.pending || a.playState === 'running');
      }, 'page compositor settles');
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      await sleep(120);
    }
    const s = await state();
    const q = await queue();
    const stage = `ux_${String(++count).padStart(2, '0')}_${name}`;
    window.__podcastProgressStage = '';
    await emit(stage, {
      actual: document.visibilityState === 'visible', case: mode, background: bg(),
      source: one('[data-scene-fm-title]', card())?.textContent.trim(),
      theme: document.documentElement.dataset.theme, voiceSourceVisible: !!one('.vinyl-source'),
      state: { state: s.state, voice: s.track?.voiceId, playId: s.playId, position: s.position },
      queue: { revision: q.revision, current: q.current,
        voices: q.tracks.map((entry) => [entry.entryId, entry.track.voiceId]) },
    });
    await wait(() => window.__podcastProgressStage === stage, 'native capture acknowledged');
    return { s, q };
  }
  try {
    await wait(async () => {
      const a = await invoke('account_snapshot');
      if (a.status !== 'authenticated') return false;
      epoch = a.epoch;
      // Authentication alone enables search before the navigation guard clears.
      return visible(search()) && navigationReady();
    }, 'signed-in isolated shell can navigate');
    await click(search, 'open search chart');
    await click(() => navigationReady() && search()?.getAttribute('aria-expanded') === 'true' &&
      one('button[aria-label="播客榜：更多"]', one('#search-discovery-panel:not(.menu-leave-active)')),
    'active podcast chart More');
    await wait(classicHome, 'More opens the classic podcast home');
    await wait(card, 'classic Scene FM card');
    const title = () => one('[data-scene-fm-title]', card());
    if (mode !== 'late') {
      await palette(mode === 'sample' ? brown : mode === 'only_tb' ? null : blue);
      const idle = await capture('palette_idle');
      assert(idle.s.state === 'idle' && idle.q.tracks.length === 0, 'palette cannot create playback');
    } else {
      await wait(() => bg()?.pending === 'true', 'first source palette is deliberately pending');
    }
    await click(() => one('button[aria-label="播放随心听FM"]'), 'play synthetic scene');
    const loaded = await wait(async () => {
      const s = await state();
      return s.state === 'playing' && s.track?.voiceId === '601' && s.position > .2 && s;
    }, 'native decoder advances');
    if (mode === 'late') {
      await click(() => one('button[aria-label="下一首"]'), 'switch to the next real scene episode');
      await wait(async () => {
        const s = await state();
        return s.state === 'playing' && s.track?.voiceId === '602' && title()?.textContent.includes('602');
      }, 'second voice is selected and rendered');
      await palette(red);
      const fresh = await capture('new_source');
      await sleep(1800);
      await palette(red);
      const late = await capture('old_source_discarded');
      assert(late.s.playId === fresh.s.playId && late.q.current === fresh.q.current,
        'late artwork cannot replace the decoder or scene selection');
    } else {
      await palette(mode === 'sample' ? brown : mode === 'only_tb' ? null : blue);
      const playing = await capture('playing_palette_stable');
      assert(playing.s.playId === loaded.playId && playing.q.tracks.map((v) => v.track.voiceId).join(',') === '601,602',
        'palette keeps loaded complete queue');
      await click(title, 'open current voice detail');
      await wait(() => visible(one('.lyrics-panel')), 'official voice playback detail');
      assert(!one('.vinyl-source'), 'official VoiceTitle has no source row');
      const details = await capture('voice_detail');
      assert(details.s.playId === playing.s.playId && details.q.current === playing.q.current,
        'voice detail preserves current playback');
    }
    await emit('complete', { ok: true, case: mode, stageCount: count });
  } catch (e) {
    await emit('failed', { ok: false, case: mode, error: String(e), stageCount: count,
      navigationReady: !!navigationReady(), searchExpanded: search()?.getAttribute('aria-expanded'),
      classicHome: !!classicHome(),
      page: one('.main-content')?.innerText.slice(0, 1300), background: bg() });
  }
})();
