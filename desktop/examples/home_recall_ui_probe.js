// Production Home and footer in a sealed WebView; only the service/PCM are synthetic.
(async () => {
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, rest = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...rest },
  });
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const paint = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const assert = (ok, message) => { if (!ok) throw Error(message); };
  const play = () => document.querySelector('.playerbar .play-toggle');
  const guide = () => document.querySelector('[data-home-recall-guide]');
  async function wait(predicate, label, timeout = 12000) {
    const start = performance.now();
    do {
      const result = await predicate();
      if (result) return result;
      await sleep(32);
    } while (performance.now() - start < timeout);
    throw Error(label);
  }
  try {
    const { epoch } = await wait(async () => {
      const value = await invoke('account_snapshot');
      return value.status === 'authenticated' && value;
    }, 'isolated verified synthetic account');
    const player = () => invoke('player_snapshot', { epoch });
    const queue = () => invoke('player_queue', { epoch });
    const home = await wait(() => [...document.querySelectorAll('.sidebar button')]
      .find((button) => button.textContent.trim() === '首页' && !button.disabled), 'Home button available');
    home.focus({ preventScroll: true });
    home.click();
    await wait(guide, 'prefill guide rendered');
    const firstSeen = performance.now();
    await wait(() => {
      const section = document.querySelector('.discovery-home [data-home-section="playlists"]');
      const bubble = guide();
      return section?.getBoundingClientRect().height > 0 && bubble &&
        Number(getComputedStyle(bubble).opacity) >= .95 &&
        !(section.closest('.page-view')?.getAnimations() ?? []).some((animation) =>
          animation.pending || animation.playState === 'running');
    }, 'homepage content and guide painted together');
    await paint();
    const ready = await player();
    const prefill = await queue();
    const rows = prefill.tracks.map(({ track }) => track.id);
    assert(rows.join(',') === Array.from({ length: 12 }, (_, i) => String(911 + i)).join(','),
      'native queue used twelve HOT rows in presentation order');
    assert(ready.state === 'idle' && ready.track.id === '911' &&
      ready.entryId === prefill.current && ready.playId == null && ready.position == null,
      'prefill selected metadata but did not load/play audio');
    assert(guide()?.textContent.trim() === '为你专属推荐，一键听听看~' &&
      !play()?.disabled && play()?.getAttribute('aria-label') === '播放', 'footer pause glyph and prompt');
    const guideRect = guide().getBoundingClientRect(), playRect = play().getBoundingClientRect();
    assert(guideRect.height > 0 && guideRect.bottom < playRect.top &&
      Math.abs((guideRect.left + guideRect.right - playRect.left - playRect.right) / 2) < 2,
      'guide is painted above and centered on the actual Play button');
    assert(document.activeElement !== play() && document.activeElement !== guide(), 'guide stole focus');
    await emit('ux_recall_ready', { rows, ready, guide: guide().textContent.trim() });
    assert(guide()?.isConnected, 'visible guide capture missed its display window');
    // The first screenshot can take time; measure the production guide from
    // its own appearance rather than adding another delay after capture.
    await sleep(Math.max(0, 900 - (performance.now() - firstSeen)));
    assert(guide()?.isConnected, 'guide closed too soon');
    await wait(() => !guide(), 'guide closed automatically', 3500);
    const visibleMs = performance.now() - firstSeen;
    assert(visibleMs > 2700 && visibleMs < 3850, 'guide did not last approximately three seconds');
    await paint();
    const closed = await player();
    assert(closed.state === 'idle' && closed.queueRevision === ready.queueRevision &&
      closed.entryId === ready.entryId && closed.playId == null, 'closing prompt changed transport');
    assert(play()?.getAttribute('aria-label') === '播放' && !guide(), 'closed guide and idle footer rendered');
    await emit('ux_recall_closed', { closed, visibleMs });

    // In screenshot mode emit returns only after the host has written the
    // actual closed-state PNG. Never start Play while that frame is pending.
    const beforePlay = await player();
    assert(beforePlay.state === 'idle' && beforePlay.playId == null &&
      beforePlay.entryId === closed.entryId && beforePlay.queueRevision === closed.queueRevision,
      'playback changed while the closed frame was awaiting capture');
    const playButton = play();
    playButton.focus({ preventScroll: true });
    playButton.click();
    const playing = await wait(async () => {
      const view = await player();
      return view.state === 'playing' && view.track?.id === '911' &&
        view.position > 0.25 && view;
    }, 'explicit footer Play advanced actual null audio clock');
    assert(playing.entryId === ready.entryId && playing.playId &&
      playing.queueRevision === ready.queueRevision, 'Play opened the exact prefetched selection');
    assert(JSON.stringify((await queue()).tracks) === JSON.stringify(prefill.tracks), 'Play reordered rows');
    await wait(() => play()?.getAttribute('aria-label') === '暂停', 'playing footer rendered');
    await paint();
    await emit('ux_recall_playing', { playing, beforePlay });
    await emit('complete', { ok: true, realAccount: false });
  } catch (error) {
    await emit('failed', { ok: false, error: String(error), stack: error?.stack,
      visibility: document.visibilityState,
      footer: play() && { disabled: play().disabled, label: play().getAttribute('aria-label') },
      home: !!document.querySelector('.discovery-home'),
    });
  }
})();
