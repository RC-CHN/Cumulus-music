// Production Vue controls, native account/queue snapshots and null-output libmpv.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const visible = (node) => node && !node.disabled && !node.closest('[inert]') &&
    node.getClientRects().length > 0;
  const button = (label) => one(`button[aria-label="${label}"]`);
  const card = () => one('[data-podcast-scene-fm]');
  const preference = () => one('input[data-scene-fm-autoplay]');
  const cover = (label) => one(`button[aria-label="${label}随心听FM"]`, card());
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const assert = (value, label) => { if (!value) throw Error(label); };
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  async function wait(predicate, label) {
    const end = performance.now() + 15000;
    while (performance.now() < end) {
      const value = await predicate();
      if (value) return value;
      await sleep(55);
    }
    throw Error(label);
  }
  async function click(getNode, label) {
    const node = await wait(() => {
      const current = getNode();
      return visible(current) && current;
    }, label);
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    node.click();
  }

  const kind = window.__podcastSceneHomeCase;
  let epoch, stageCount = 0;
  const player = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  const saved = async () => (await invoke('scene_fm_autoplay_preference', { epoch })).value;
  async function capture(name) {
    if (window.__CUMULUS_PROOF_INVOKE__) {
      // The X11 capture reads the compositor, not the freshly updated DOM.
      const target = name === 'title_resumes_detail' ? '.lyrics-panel' : '.page-view';
      await wait(() => {
        const page = one(target);
        return page?.isConnected && getComputedStyle(page).opacity === '1' &&
          !page.getAnimations().some((animation) => animation.pending || animation.playState === 'running');
      }, 'route or detail transition has painted');
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      await sleep(120);
    }
    const state = await player();
    const rows = await queue();
    const stage = `ux_${String(++stageCount).padStart(2, '0')}_${name}`;
    window.__podcastProgressStage = '';
    await emit(stage, {
      actual: document.visibilityState === 'visible', case: kind,
      layout: one('.podcast-top')?.dataset.layout, hasCard: !!card(),
      hasStandalone: !!one('.main-content .scene-fm-page') && !card(),
      scenes: [...(card()?.querySelectorAll('[data-scene-id]') ?? [])].map((n) => n.dataset.sceneId),
      checked: preference()?.checked, lyricsOpen: !!one('.lyrics-panel:not(.lyrics-panel-leave-active)'),
      state: { state: state.state, voice: state.track?.voiceId, song: state.track?.id,
        playId: state.playId, position: state.position, mode: state.timeline?.mode },
      queue: { revision: rows.revision, current: rows.current,
        entries: rows.tracks.map((item) => [item.entryId, item.track.voiceId]) },
    });
    await wait(() => window.__podcastProgressStage === stage, `saved ${stage}`);
    return { state, rows };
  }
  async function openHome(layout) {
    await click(() => one('input[aria-label="搜索音乐"]'), 'search field');
    await click(() => button('播客榜：更多'), 'podcast chart More');
    await wait(() => one('.podcast-discovery .podcast-top')?.dataset.layout === layout,
      `podcast homepage ${layout}`);
  }
  async function idle() {
    const state = await player();
    assert(state.state === 'idle' && !state.playId && (await queue()).tracks.length === 0,
      'no native audio or queue was created');
  }
  async function stableIdle() {
    // Observe the already rendered visit through at least one delayed actor turn.
    await sleep(620);
    await idle();
  }
  async function playing() {
    const loaded = await wait(async () => {
      const state = await player();
      return state.state === 'playing' && state.track?.voiceId === '601' &&
        state.track.id === '1601' && state.position > .22 && state.timeline?.mode === 'full' &&
        visible(cover('暂停')) && state;
    }, 'first synthetic Scene FM voice advances on the native clock');
    const rows = await queue();
    assert(rows.tracks.map((item) => item.track.voiceId).join(',') === '601,602' &&
      rows.current === rows.tracks[0].entryId && !rows.fm && loaded.playId,
      'scene keeps the complete ordered podcast queue');
  }

  try {
    await wait(async () => {
      const account = await invoke('account_snapshot');
      if (account.status !== 'authenticated') return false;
      epoch = account.epoch;
      return visible(button('打开设置'));
    }, 'synthetic signed-in shell');
    assert(!card(), 'boot does not mount the podcast card');
    await click(() => button('打开设置'), 'open real Settings');
    await wait(() => visible(preference()), 'Settings playback checkbox');
    assert(preference().closest('[aria-labelledby="settings-playback"]'), 'control in Settings / Play');
    const desired = kind !== 'manual';
    // Exercise an actual UI preference change even if an inherited default
    // changes. Neither setting transition is allowed to start audio here.
    if (preference().checked === desired) {
      await click(preference, 'toggle initial local Scene FM setting');
      await wait(async () => visible(preference()) && await saved() === !desired,
        'initial preference saved');
    }
    await click(preference, 'save local Scene FM setting');
    await wait(async () => visible(preference()) && preference().checked === desired &&
      await saved() === desired, 'desired preference saved and rendered');
    await stableIdle();
    await capture('settings');

    await openHome(kind === 'updated' ? 'updated' : 'classic');
    if (kind === 'updated') {
      await wait(() => one('.podcast-top[data-layout="updated"] [data-preview="book"]'),
        'updated audiobook preview is rendered');
      assert(!card(), 't1 does not mount the Scene FM card');
      await stableIdle();
      await capture('updated_no_scene');
    } else {
      await wait(card, 'classic inline Scene FM card');
      if (kind === 'retry') {
        const retry = () => [...(card()?.querySelectorAll('button') ?? [])]
          .find((node) => node.textContent.trim() === '刷新找回' || node.getAttribute('aria-label') === '刷新找回');
        await wait(() => visible(retry()), 'directory failure exposes explicit retry');
        await idle();
        await capture('directory_error');
        await click(retry, 'retry from the same classic homepage visit');
      }
      if (kind === 'manual') {
        await wait(() => visible(cover('播放')) && one('[data-scene-id="OP_1"]', card()),
          'manual card renders its directory');
        await stableIdle();
        await capture('classic_opted_out');
        await click(() => cover('播放'), 'explicit inline Scene FM Play');
      }
      await playing();
      await capture(kind === 'automatic' ? 'first_automatic' : kind === 'manual' ? 'manual_play' : 'retry_automatic');
      if (kind === 'automatic') {
        await click(() => cover('暂停'), 'pause through inline Scene FM card');
        await wait(async () => (await player()).state === 'paused' && visible(cover('播放')),
          'native pause and card transport agree');
        const paused = await capture('paused');
        await click(() => button('打开设置'), 'unmount the podcast homepage');
        await wait(() => visible(preference()) && preference().checked && !card(),
          'Settings reopens with saved true and card gone');
        await openHome('classic');
        await wait(() => visible(cover('播放')) && one('[data-scene-id="OP_1"]', card()),
          'same window remounts the classic card');
        await sleep(620);
        const remount = await capture('remount_no_replay');
        assert(remount.state.state === 'paused' && remount.state.playId === paused.state.playId &&
          remount.rows.revision === paused.rows.revision && remount.rows.current === paused.rows.current &&
          Math.abs(remount.state.position - paused.state.position) < .10,
          'autoplay stays consumed for the same window/account; native clock and queue stay paused');
        await click(() => one('[data-scene-fm-title]', card()), 'open the current voice from its inline title');
        await wait(async () => {
          const state = await player();
          return visible(one('.lyrics-panel:not(.lyrics-panel-leave-active)')) && state.state === 'playing' &&
            state.playId === remount.state.playId && state.track?.voiceId === '601' &&
            state.position > remount.state.position + .1;
        }, 'title resumes the same paused decoder and opens the actual playback detail');
        assert(!one('.vinyl-source', one('.lyrics-panel:not(.lyrics-panel-leave-active)')),
          'official voice detail omits provenance instead of showing unknown');
        const detail = one('.lyrics-panel:not(.lyrics-panel-leave-active)');
        assert(one('[data-voice-detail]', detail) &&
          one('[data-voice-release]', detail)?.textContent.trim().startsWith('发布时间：') &&
          !one('.vinyl-credits[aria-label="歌手"], .vinyl-album', detail),
          'voice detail renders host/release metadata instead of song artist and album credits');
        const opened = await capture('title_resumes_detail');
        assert(opened.state.playId === remount.state.playId &&
          opened.rows.revision === remount.rows.revision && opened.rows.current === remount.rows.current &&
          opened.state.position >= remount.state.position,
          'title navigation keeps the loaded voice and queue identity');
      }
    }
    await emit('complete', { ok: true, case: kind, stageCount });
  } catch (error) {
    await emit('failed', { ok: false, case: kind, error: String(error), stageCount,
      focused: document.activeElement?.getAttribute('aria-label'),
      page: one('.main-content')?.innerText.slice(0, 1200) });
  }
})();
