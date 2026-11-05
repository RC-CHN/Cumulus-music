// Native Episode metadata → real program title/profile navigation and image error fallback.
(async () => {
  const one = (s, root = document) => root?.querySelector(s);
  const visible = (n) => n && n.getClientRects().length && !n.disabled && !n.closest('[inert]');
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (v, message) => { if (!v) throw Error(message); };
  const card = () => one('[data-podcast-scene-fm]');
  let epoch, playId, count = 0, last = 'startup';
  const player = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  async function wait(test, label) {
    const end = performance.now() + 18000;
    while (performance.now() < end) {
      const value = await test();
      if (value) return value;
      await sleep(55);
    }
    throw Error(label);
  }
  async function click(get, label) {
    last = label;
    const node = await wait(() => {
      const current = get();
      return visible(current) && current;
    }, label);
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    node.click();
  }
  function fallback(selector) {
    const node = one(selector);
    return node?.dataset.coverState === 'error' &&
      !!one('[data-cover-placeholder]', node) && !one('img', node);
  }
  function metadata() {
    const root = one('.lyrics-panel');
    return root && {
      creator: one('[data-voice-creator]', root)?.textContent.trim(),
      release: one('[data-voice-release] time', root)?.textContent.trim(),
      songCredits: !!one('.vinyl-credits,.vinyl-album,.vinyl-source', root),
    };
  }
  async function capture(name) {
    last = name;
    const root = one('.lyrics-panel') ? '.lyrics-panel' : '.page-view';
    if (window.__CUMULUS_PROOF_INVOKE__) {
      await wait(() => {
        const n = one(root);
        return n && getComputedStyle(n).opacity === '1' &&
          !n.getAnimations().some((a) => a.pending || a.playState === 'running');
      }, 'compositor settles for the current page');
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      await sleep(120);
    }
    const state = await player();
    const rows = await queue();
    assert(!JSON.stringify(state).includes('synthetic-private-field'), 'native DTO strips unrelated creator fields');
    const stage = `ux_${String(++count).padStart(2, '0')}_${name}`;
    window.__podcastProgressStage = '';
    await emit(stage, {
      actual: document.visibilityState === 'visible',
      metadata: metadata(), profile: one('.playlist-creator-page h1')?.textContent.trim(),
      covers: {
        card: one('.scene-fm-cover')?.dataset.coverState,
        bar: one('.player-cover')?.dataset.coverState,
        detail: one('.vinyl-record')?.dataset.coverState,
      },
      state: {
        state: state.state, playId: state.playId, voice: state.track?.voiceId,
        position: state.position, voiceDetail: state.voiceDetail,
      },
      queue: { revision: rows.revision, current: rows.current,
        voices: rows.tracks.map((entry) => [entry.entryId, entry.track.voiceId]) },
    });
    await wait(() => window.__podcastProgressStage === stage, 'native stage acknowledgement');
  }
  async function detail() {
    await wait(() => {
      const m = metadata();
      return m?.creator === '合成主播' && m.release === '2023-11-15' && !m.songCredits;
    }, 'official program header shows user creator/date, not song artist/album/source');
    await wait(() => fallback('.vinyl-record'), 'detail cover uses local error art');
    const state = await player();
    assert(state.playId === playId && state.track.voiceId === '601', 'metadata display preserves current decoder');
    const native = state.voiceDetail;
    assert(native?.id === '601' && native.creator?.id === '55' &&
      native.creator.name === '合成主播' && native.releaseTime === 1700049600000 &&
      Object.keys(native).sort().join(',') === 'creator,id,releaseTime' &&
      Object.keys(native.creator).sort().join(',') === 'id,name',
      'current native episode metadata is exactly whitelisted');
  }
  try {
    await wait(async () => {
      const a = await invoke('account_snapshot');
      if (a.status !== 'authenticated') return false;
      epoch = a.epoch;
      return visible(one('input[aria-label="搜索音乐"]'));
    }, 'isolated signed-in shell');
    await click(() => one('input[aria-label="搜索音乐"]'), 'search chart');
    await click(() => one('button[aria-label="播客榜：更多"]'), 'classic podcast homepage');
    await wait(() => card() && one('[data-scene-fm-title]', card()), 'actual scene episode');
    await wait(() => fallback('.scene-fm-cover'), 'blocked public cover changes to local placeholder');
    await capture('card_placeholder');
    await click(() => one('.scene-fm-cover[aria-label="播放随心听FM"]'), 'play current program');
    const loaded = await wait(async () => {
      const s = await player();
      return s.state === 'playing' && s.track?.voiceId === '601' && s.position > .2 && s;
    }, 'actual libmpv voice playback');
    playId = loaded.playId;
    await wait(() => fallback('.player-cover'), 'playback-bar image failure uses placeholder');
    await capture('bar_placeholder');
    await click(() => one('[data-scene-fm-title]', card()), 'open real voice detail');
    await detail();
    await capture('voice_metadata');
    await click(() => one('[data-voice-creator]'), 'creator opens a user profile');
    await wait(() => one('.playlist-creator-page h1')?.textContent.trim() === '合成主播主页' &&
      !one('.lyrics-panel'), 'native user profile 55');
    assert((await player()).playId === playId, 'profile navigation does not replace audio');
    await capture('creator_profile');
    await click(() => one('button[aria-label="返回上一页"]'), 'back to original podcast page');
    await wait(() => card() && one('[data-scene-fm-title]', card()), 'scene page restored');
    await click(() => one('.player-cover'), 'reopen same program details from playback bar');
    await detail();
    await capture('detail_reopened');
    await emit('complete', { ok: true, stageCount: count, realMusicRequests: 0,
      realAccountWrites: 0, physicalAudioVerified: false });
  } catch (error) {
    await emit('failed', { ok: false, last, error: String(error), stageCount: count,
      page: one('.main-content')?.innerText.slice(0, 1800), metadata: metadata() });
  }
})();
