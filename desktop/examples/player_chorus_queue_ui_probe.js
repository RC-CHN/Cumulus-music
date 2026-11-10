// Opt-in actual WebView and libmpv proof; the native sealed fixture counts requests.
(async () => {
  const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
  const q = s => document.querySelector(s);
  const player = () => q('.playerbar');
  const mark = () => q('.seek-chorus');
  const assert = (ok, label) => { if (!ok) throw Error(label); };
  const wait = async (condition, label) => {
    const limit = performance.now() + 16000;
    while (!condition()) {
      assert(performance.now() < limit, label);
      await pause(35);
    }
  };
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, more = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: {stage, ...more},
  });
  function click(node) {
    assert(node && !node.disabled, 'target available');
    node.focus({preventScroll: true});
    node.click();
  }
  async function chooseTheme(theme) {
    click(q('[aria-label="外观主题"]'));
    await wait(() => q('.action-menu'), 'theme chooser visible');
    const label = theme === 'dark' ? '深色' : '浅色';
    click([...document.querySelectorAll('.action-menu button')].find(button => button.textContent.trim() === label));
    await wait(() => document.documentElement.dataset.theme === theme && !q('.action-menu'), 'native theme applied');
  }
  async function capture(stage, id, seconds, previous) {
    const ratio = 100 * seconds / 30;
    await wait(() => player()?.dataset.trackId === id
      && player().dataset.state === 'playing'
      && player().dataset.playId !== previous
      && Number(player().dataset.position) > .12
      && Math.abs(parseFloat(mark()?.style.left) - ratio) < .01, `native track ${id}, distinct marker and clock`);
    const p = player().dataset;
    const markerPercent = parseFloat(mark().style.left);
    const rect = mark().getBoundingClientRect();
    const x = rect.left + rect.width / 2, y = rect.top + rect.height / 2;
    assert(document.elementFromPoint(x, y) === q('.seek-rail input'), 'ordinary marker keeps the seek target');
    const color = getComputedStyle(mark()).backgroundColor;
    const before = document.documentElement.dataset.theme === 'dark'
      ? 'rgba(255, 255, 255, 0.3)' : 'rgba(40, 50, 72, 0.3)';
    assert(rect.width === 4 && rect.height === 4, 'four pixel standard marker');
    assert(color === (mark().classList.contains('reached') ? 'rgb(255, 255, 255)' : before),
      'official default theme marker color');
    await emit(stage, {
      trackId: p.trackId, playId: p.playId, markerPercent, markerVisible: rect.width > 0,
      markerColor: color, theme: document.documentElement.dataset.theme,
      screenX, screenY, ratio: devicePixelRatio,
      paintSamples: [{x, y, color, under: {x: x + 5, y}}],
    });
    await wait(() => window.__chorusQueueRecorded === stage, `native stage receipt: ${stage}`);
    return p.playId;
  }
  try {
    await wait(() => q('.daily-track-actions .primary:not(:disabled)'), 'signed-in synthetic Daily ready');
    assert(document.querySelectorAll('.track-row').length === 2, 'exact two fixture songs');
    await chooseTheme('light');
    click(q('.daily-track-actions .primary'));
    const a = await capture('ux_01_chorus_a', '1', 4);
    const input = q('.seek-rail input');
    const {epoch} = await invoke('account_snapshot');
    await invoke('player_action', {epoch, action: {kind: 'pause', paused: true}});
    await invoke('player_action', {epoch, action: {kind: 'seek', seconds: 1}});
    await wait(() => player()?.dataset.state === 'paused' && Number(player().dataset.position) === 1,
      'native clock is parked below the chorus');
    input.value = '5';
    input.dispatchEvent(new Event('input', {bubbles:true}));
    await wait(() => mark()?.classList.contains('reached'), 'uncommitted scrub uses its own clock');
    assert(getComputedStyle(mark()).backgroundColor === 'rgb(255, 255, 255)' &&
      Number(player().dataset.position) === 1, 'white highlight did not issue a native seek');
    input.dispatchEvent(new PointerEvent('pointercancel', {bubbles:true}));
    await wait(() => !mark()?.classList.contains('reached'), 'cancel restores the native clock');
    await invoke('player_action', {epoch, action: {kind: 'pause', paused: false}});
    await wait(() => player()?.dataset.state === 'playing', 'same playback resumed before changing track');
    await chooseTheme('dark');
    click(q('.playerbar [aria-label="下一首"]'));
    const b = await capture('ux_02_chorus_b', '2', 13, a);
    click(q('.playerbar [aria-label="上一首"]'));
    await capture('ux_03_chorus_a_again', '1', 4, b);
    await emit('complete', {ok: true, stageCount: 3, scrubClockVerified: true, realMusicRequests: 0, accountWrites: 0});
  } catch (cause) {
    await emit('failed', {ok: false, error: String(cause), trackId: player()?.dataset.trackId,
      markerPercent: mark()?.style.left, position: player()?.dataset.position, state: player()?.dataset.state});
  }
})();
