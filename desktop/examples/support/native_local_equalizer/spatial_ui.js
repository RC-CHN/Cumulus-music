// The sealed host opens only the fixed local FLAC; all user controls and
// observations below use the rendered main UI and production Tauri commands.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (payload) => invoke('plugin:event|emit', { event: 'local-spatial-proof', payload });
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (value, message) => { if (!value) throw new Error(message); };
  const bar = () => one('.playerbar');
  const dialog = () => one('.equalizer-dialog');
  const control = (label) => one(`[aria-label="${label}"]`);
  const spatial = () => one('.equalizer-dialog [data-spatial-status]');
  const launcher = () => one('.playerbar .equalizer-toggle');
  const master = () => control('启用均衡器和增强音效');
  const transport = () => one('.playerbar .play-toggle');
  const tone = ['低音增益', '高音增益'];
  const sliders = ['环绕深度', '环绕强度'];
  let last = 'account';
  let epoch;
  async function wait(read, label) {
    last = label;
    const deadline = performance.now() + 11000;
    while (performance.now() < deadline) {
      const value = await read();
      if (value) return value;
      await sleep(35);
    }
    throw new Error(label);
  }
  function visible(node) {
    if (!node?.isConnected || node.disabled || node.closest('[inert]')) return false;
    const r = node.getBoundingClientRect();
    return r.width > 0 && r.height > 0 &&
      node.contains(document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2));
  }
  function reveal(node) {
    if (!visible(node)) node?.scrollIntoView({ block: 'nearest', behavior: 'instant' });
  }
  function click(node) {
    reveal(node);
    assert(visible(node), 'expected visible control: ' + node?.outerHTML.slice(0, 120));
    node.focus({ preventScroll: true });
    node.click();
  }
  function set(label, value, event = 'change') {
    const node = control(label);
    reveal(node);
    assert(visible(node), 'visible slider ' + label);
    node.value = String(value);
    const accepted = Number(node.value);
    node.dispatchEvent(new Event(event, { bubbles: true }));
    return accepted;
  }
  function sliderValues(labels) {
    return labels.map((label) => Number(control(label)?.value));
  }
  function samePlay(a, b) {
    return a.playId === b.playId && a.entryId === b.entryId && a.queueRevision === b.queueRevision;
  }
  function snapshot() { return invoke('player_snapshot', { epoch }); }
  async function openFile(ordinal) {
    await emit({ open: ordinal });
    const ack = await wait(() => window.__spatialOpenAck?.ordinal === ordinal &&
      window.__spatialOpenAck, 'trusted native open ' + ordinal);
    assert(ack.ok, ack.error);
  }
  async function openEq() {
    await wait(() => visible(launcher()), 'enabled EQ launcher');
    click(launcher());
    await wait(() => dialog()?.open && control(sliders[1]), 'opened EQ dialog');
    reveal(control(sliders[1]));
    await wait(() => visible(control(sliders[0])) && visible(control(sliders[1])),
      'visible spatial controls');
    assert(sliders.every((label, i) => {
      const input = control(label);
      return input.min === String(i === 0 ? -10 : 0) && input.max === '10' && input.step === '1';
    }), 'official integer slider geometry');
  }
  async function closeEq() {
    click(control('关闭均衡器'));
    await wait(() => !dialog(), 'closed EQ dialog');
  }
  async function playback(read, label) {
    return wait(async () => {
      const view = await snapshot();
      assert(!view.error, 'native player error: ' + view.error);
      return view.state === bar()?.dataset.state && read(view) && view;
    }, label);
  }
  async function pause(view) {
    assert(transport()?.getAttribute('aria-label') === '暂停', 'visible Pause');
    click(transport());
    return playback((now) => samePlay(now, view) && now.state === 'paused' && !now.nativeSeeking,
      'accepted real Pause');
  }
  async function equalizer(revision, values, status) {
    return wait(async () => {
      const view = await snapshot();
      return view.equalizer.revision !== revision &&
        view.equalizer.spatial_status === status &&
        dialog()?.dataset.draftState === 'settled' &&
        JSON.stringify(sliderValues(sliders)) === JSON.stringify(values) &&
        (status === 'off' || spatial()?.dataset.spatialStatus === status) && view;
    }, 'committed spatial controls: ' + status);
  }
  function paintSample(selector) {
    const node = one(selector);
    assert(node?.isConnected && !node.closest('[inert]'), 'connected capture padding');
    const bounds = node.getBoundingClientRect();
    const pad = Number.parseFloat(getComputedStyle(node).paddingLeft);
    const x = bounds.left + Math.min(5, pad / 2), y = bounds.top + bounds.height / 2;
    assert(pad >= 4 && document.elementFromPoint(x, y) === node, 'uncovered capture padding');
    let color;
    for (let current = node; current; current = current.parentElement) {
      const style = getComputedStyle(current);
      assert(style.opacity === '1' && style.visibility === 'visible', 'visible paint ancestry');
      if (color) continue;
      assert(style.backgroundImage === 'none', 'solid capture padding');
      const rgb = /^rgb\((\d+),\s*(\d+),\s*(\d+)\)$/.exec(style.backgroundColor);
      if (rgb) color = 'rgb(' + rgb.slice(1).join(', ') + ')';
      else assert(style.backgroundColor === 'transparent' ||
        /^rgba\(\d+,\s*\d+,\s*\d+,\s*0\)$/.test(style.backgroundColor), 'nonopaque padding');
    }
    assert(color && color !== 'rgb(0, 0, 0)', 'nonblack opaque background');
    return { x, y, color, source: selector };
  }
  async function stage(name, view, extra = {}) {
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    if (dialog()) {
      const r = dialog().getBoundingClientRect();
      assert(dialog().scrollWidth <= dialog().clientWidth + 1 &&
        r.top >= 0 && r.bottom <= innerHeight + 1, 'EQ dialog stays in viewport');
    }
    const capture = window.__spatialCapture ? {
      screenX, screenY, ratio: devicePixelRatio,
      paintSamples: (dialog() ? ['.equalizer-dialog'] : ['.sidebar-caption', '.playerbar'])
        .map(paintSample),
    } : {};
    await emit({ stage: name, epoch, player: view, ...capture, ...extra, dom: {
      state: bar()?.dataset.state, status: spatial()?.dataset.spatialStatus ?? null,
      sliders: dialog() ? sliderValues(sliders) : null,
      tone: dialog() ? sliderValues(tone) : null,
      master: master()?.getAttribute('aria-checked') ?? null,
    }});
    await wait(() => window.__spatialStageAck === name, 'durable stage ' + name);
  }
  try {
    const account = await wait(async () => {
      const value = await invoke('account_snapshot');
      return value.epoch != null && value.status !== 'authenticated' && value;
    }, 'isolated guest epoch');
    epoch = account.epoch;
    await wait(() => visible(launcher()), 'ready local player');

    await openFile(1);
    const stock = await playback((view) => view.state === 'playing' && view.position > .08,
      'first stock source advances');
    const stockPaused = await pause(stock);
    await openEq();
    const before = await snapshot();
    set(sliders[0], 3, 'input');
    await sleep(180);
    const preview = await snapshot();
    assert(preview.equalizer.revision === before.equalizer.revision &&
      !preview.equalizer.settings.presence, 'uncommitted drag does not write settings');
    set(sliders[0], 3);
    set(sliders[1], 5);
    const saved = await equalizer(before.equalizer.revision, [3, 5], 'unavailable');
    assert(samePlay(saved, stockPaused) && saved.state === 'paused' &&
      !saved.equalizer.settings.enabled && saved.equalizer.settings.gains.every((gain) => gain === 0),
    'spatial-only stock save does not restart or enable GEQ');
    await stage('ux_01_stock_saved', saved);
    await closeEq();

    await openFile(2);
    const playing = await playback((view) => view.state === 'playing' &&
      view.position > .1 && view.playId !== stock.playId &&
      view.equalizer.spatial_status === 'processing' && launcher()?.dataset.equalizerState === 'processing',
    'native clock shows real spatial-only PCM processing after explicit open');
    const paused = await pause(playing);
    await openEq();
    set(sliders[0], 6);
    set(sliders[1], 8);
    const hot = await equalizer(paused.equalizer.revision, [6, 8], 'configured');
    assert(samePlay(hot, paused) && hot.state === 'paused', 'hot apply preserves paused native play');
    await stage('ux_02_hot_paused', hot);

    click(master());
    const off = await equalizer(hot.equalizer.revision, [6, 8], 'bypassed');
    assert(samePlay(off, hot) && off.equalizer.settings.master_bypassed, 'master retains saved sliders');
    await stage('ux_03_master_off', off);
    click(master());
    const on = await equalizer(off.equalizer.revision, [6, 8], 'configured');
    assert(samePlay(on, off) && !on.equalizer.settings.master_bypassed, 'master re-applies same graph');
    await stage('ux_04_master_on', on);
    await closeEq();

    const requested = .5;
    const step = Number(control('播放进度')?.step);
    const accepted = set('播放进度', requested);
    assert(step > 0 && Number.isFinite(step) && Math.abs(accepted - requested) <= step / 2 + 1e-9,
      'browser range rounds the requested seek within half its actual step');
    const sought = await playback((view) => samePlay(view, on) && view.state === 'paused' &&
      !view.nativeSeeking && Math.abs(view.position - accepted) < .06, 'paused native replacement clock');
    await stage('ux_05_paused_seek', sought, { seekTarget: { requested, accepted, step } });
    assert(transport()?.getAttribute('aria-label') === '播放', 'visible Resume');
    click(transport());
    await playback((view) => samePlay(view, sought) && view.state === 'playing' &&
      view.position > .65 && view.equalizer.spatial_status === 'processing', 'resumed native spatial PCM');
    const ended = await playback((view) => samePlay(view, sought) && view.state === 'ended' &&
      Math.abs(view.position - 88207 / 44100) < .02, 'real native EOF at original displayed source duration');
    await stage('ux_06_ended', ended);

    await openEq();
    set(tone[0], 4);
    set(tone[1], -2);
    const all = await wait(async () => {
      const view = await snapshot();
      return view.equalizer.settings.bass_db === 4 && view.equalizer.settings.treble_db === -2 &&
        dialog()?.dataset.draftState === 'settled' && view;
    }, 'native preference has all four enhancements before reset');
    click([...dialog().querySelectorAll('button')].find((node) => node.textContent.trim() === '重置增强音效'));
    const reset = await equalizer(all.equalizer.revision, [0, 0], 'off');
    assert(samePlay(reset, ended) && tone.every((_, i) => sliderValues(tone)[i] === 0) &&
      !reset.equalizer.settings.bass_db && !reset.equalizer.settings.treble_db,
    'one enhanced reset clears tone and spatial, keeps completed source');
    await stage('ux_07_reset', reset);
    await emit({ stage: 'complete', ok: true, stageCount: 7, epoch,
      startedSpatial: playing.equalizer.spatial_status === 'processing' });
  } catch (error) {
    await emit({ stage: 'failed', ok: false, last, error: String(error), epoch });
  }
})();
