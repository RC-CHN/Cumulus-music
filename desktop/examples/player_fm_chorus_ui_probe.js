// One closed FM source-switch scenario. All playback commands use production IPC.
(async () => {
  const q = selector => document.querySelector(selector);
  const bar = () => q('.playerbar');
  const mark = () => q('.seek-chorus');
  const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
  const frame = () => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, more = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: {stage, ...more},
  });
  const ratio = seconds => seconds * 100 / 30;
  const assert = (value, reason) => { if (!value) throw Error(reason); };
  const wait = async (probe, reason) => {
    const end = performance.now() + 16000;
    while (performance.now() < end) {
      const value = await probe();
      if (value) return value;
      await delay(35);
    }
    throw Error(reason);
  };
  const percent = () => mark() ? parseFloat(mark().style.left) : null;
  const near = (a, b) => a != null && Math.abs(a - b) < .01;
  const sourceA = {kind: 'personal'};
  const sourceB = {kind: 'familiar', source_id: '["41","42"]'};
  let observer;
  let last;

  async function observe(stage, seconds, previousPlay, parked = false) {
    last = stage;
    await wait(() => {
      const player = bar()?.dataset;
      return player?.fm === 'true' && player.trackId === '1' && player.state === (parked ? 'paused' : 'playing')
        && player.playId && player.playId !== previousPlay && Number(player.position) > .12
        && (seconds == null ? !mark() : near(percent(), ratio(seconds)));
    }, stage + ': native playing clock and scoped chorus marker');
    await frame();
    const player = bar().dataset;
    const node = mark();
    const rail = q('.trial-seek-track')?.getBoundingClientRect();
    assert(rail?.width > 0 && document.visibilityState === 'visible', 'actual visible layered seek rail');
    const samples = [];
    let thumbSample = null;
    if (seconds == null) {
      assert(!node, 'old marker is absent from the real DOM while Familiar response is held');
    } else {
      const rect = node?.getBoundingClientRect();
      assert(rect?.width === 4 && rect.height === 4 && near(percent(), ratio(seconds)), 'visible marker geometry');
      const x = rect.left + 2, y = rect.top + 2;
      const input = q('.seek-rail input');
      assert(document.elementFromPoint(x, y) === input && !input.disabled && input.tabIndex === 0,
        'painted chorus point does not steal the range pointer or tab target');
      const color = getComputedStyle(node).backgroundColor;
      if (parked) {
        assert(document.activeElement === input && node.classList.contains('reached'), 'focused thumb overlaps reached marker');
        thumbSample = {center: {x, y}, outsideRail: {x, y: y - 3}, markerColor: color};
      } else samples.push({x, y, color, under: {x: x + 5, y}});
    }
    await emit(stage, {
      playId: player.playId, position: Number(player.position), trackId: player.trackId,
      markerPercent: percent(), markerVisible: Boolean(node), paintSamples: samples,
      missingSample: seconds == null ? {x: rail.left + rail.width * ratio(4) / 100, y: rail.top + 2} : null,
      thumbSample, screenX, screenY, ratio: devicePixelRatio,
    });
    await wait(() => window.__fmChorusRecorded === stage, 'native snapshot and actual screenshot: ' + stage);
    return player.playId;
  }

  try {
    const account = await wait(async () => {
      const current = await invoke('account_snapshot');
      return current.status === 'authenticated' && current;
    }, 'sealed synthetic account restored');
    const start = source => invoke('fm_command', {
      epoch: account.epoch, source, command: {kind: 'start', quality: 'standard'},
    });
    await start(sourceA);
    const a = await observe('ux_01_fm_personal', 4);
    let stale = false;
    observer = new MutationObserver(() => {
      if (bar()?.dataset.playId && bar().dataset.playId !== a && near(percent(), ratio(4))) stale = true;
    });
    observer.observe(bar(), {attributes: true, childList: true, subtree: true});
    await start(sourceB);
    const b = await observe('ux_02_fm_familiar_held', null, a);
    assert(!stale, 'previous source marker was painted into the Familiar play');
    observer.disconnect();
    await emit('fm-chorus-release');
    assert(await observe('ux_03_fm_familiar', 13, a) === b, 'answer updates marker without replacing the decoder');
    await start(sourceA);
    const again = await observe('ux_04_fm_personal_again', 4, b);
    assert(again !== a, 'returning to a cached source still starts a new native playback');
    await invoke('player_action', {epoch: account.epoch, action: {kind: 'pause', paused: true}});
    await invoke('player_action', {epoch: account.epoch, action: {kind: 'seek', seconds: 4}});
    const input = q('.seek-rail input');
    input.focus({preventScroll: true});
    await wait(() => Number(bar().dataset.position) === 4 && document.activeElement === input
      && getComputedStyle(q('.seek-time-preview')).opacity === '1', 'native parked thumb and visible keyboard time preview');
    assert(await observe('ux_05_fm_thumb_priority', 4, undefined, true) === again,
      'seek and keyboard focus keep the selected decoder');
    await emit('complete', {ok: true, stageCount: 5, accountWrites: 0, realMusicRequests: 0});
  } catch (cause) {
    await emit('failed', {ok: false, last, error: String(cause), state: bar()?.dataset.state,
      playId: bar()?.dataset.playId, markerPercent: percent(), position: bar()?.dataset.position});
  } finally {
    observer?.disconnect();
  }
})();
