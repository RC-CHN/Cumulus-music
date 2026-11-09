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
  const emit = (stage, more = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'ux-proof', payload: {stage, ...more},
  });
  function click(node) {
    assert(node && !node.disabled, 'target available');
    node.focus({preventScroll: true});
    node.click();
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
    await emit(stage, {trackId: p.trackId, playId: p.playId, markerPercent, markerVisible: mark().getBoundingClientRect().width > 0});
    await wait(() => window.__chorusQueueRecorded === stage, `native stage receipt: ${stage}`);
    return p.playId;
  }
  try {
    await wait(() => q('.daily-track-actions .primary:not(:disabled)'), 'signed-in synthetic Daily ready');
    assert(document.querySelectorAll('.track-row').length === 2, 'exact two fixture songs');
    click(q('.daily-track-actions .primary'));
    const a = await capture('ux_01_chorus_a', '1', 4);
    click(q('.playerbar [aria-label="下一首"]'));
    const b = await capture('ux_02_chorus_b', '2', 13, a);
    click(q('.playerbar [aria-label="上一首"]'));
    await capture('ux_03_chorus_a_again', '1', 4, b);
    await emit('complete', {ok: true, stageCount: 3, realMusicRequests: 0, accountWrites: 0});
  } catch (cause) {
    await emit('failed', {ok: false, error: String(cause), trackId: player()?.dataset.trackId,
      markerPercent: mark()?.style.left, position: player()?.dataset.position, state: player()?.dataset.state});
  }
})();
