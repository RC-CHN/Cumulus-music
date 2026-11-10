// The official catalogue's applied check follows the native PEQ, not detail focus.
(async () => {
  const invoke = (command, args) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, extra = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...extra } });
  const one = (selector) => document.querySelector(selector);
  const launcher = () => one('.playerbar .audio-effects-trigger');
  const shell = () => one('.audio-effects-panel[open]');
  const cards = (id) => [...(shell()?.querySelectorAll(`[data-effect-id="${id}"]`) ?? [])];
  const action = (name) => shell()?.querySelector(`[data-peq-action="${name}"]`);
  const ready = (node) => node?.isConnected && !node.disabled && node;
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (ok, label) => { if (!ok) throw Error(label); };
  let stage = 'startup', epoch, layout, wanted, last;
  async function wait(read, label) {
    for (let attempt = 0; attempt < 230; attempt++) {
      const result = await read();
      if (result) return result;
      await sleep(65);
    }
    throw Error(label);
  }
  function click(node) {
    assert(ready(node), 'control is present and enabled');
    node.focus({ preventScroll: true });
    node.click();
  }
  const snapshot = async () => (last = await invoke('player_snapshot', { epoch }));
  function matches(applied) {
    const nodes = cards(300);
    if (nodes.length !== wanted || cards(301).length !== 1) return false;
    const caption = applied === 'bypassed_for_source' ? '已启用，当前音源暂不处理' : '已启用';
    return nodes.every((node) => {
      const badge = node.querySelector('.effect-card-applied-badge');
      return node.dataset.effectApplied === applied && (applied ? badge?.textContent.trim() === caption : !badge);
    }) && !cards(301)[0].dataset.effectApplied;
  }
  async function capture(name, expected, checked = wanted, nativeApplied = checked > 0) {
    stage = name;
    const state = await snapshot();
    assert(state.appliedEffectId === (nativeApplied ? 300 : undefined), 'public snapshot has only the applied catalogue ID');
    assert(state.parametricProcessing === expected, 'PCM status matches independently');
    const markers = [...(shell()?.querySelectorAll('[data-effect-applied]') ?? [])];
    assert(markers.length === checked, 'only expected cards have an applied mark');
    await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));
    const receipt = `ux_${name}`;
    await emit(receipt, { captureHandshake: true, layout, markers: markers.map((node) => [Number(node.dataset.effectId), node.dataset.effectApplied]),
      viewed: [...(shell()?.querySelectorAll('[data-effect-id][aria-expanded="true"]') ?? [])].map((node) => Number(node.dataset.effectId)) });
    await wait(() => window.__ordinaryPeqStageRecorded === receipt, 'native snapshot/capture acknowledgement');
  }
  async function play(id, quality) {
    const selected = await invoke('play_selection', { epoch, selection: { kind: 'songs', ids: [id], index: 0, quality } });
    return wait(async () => {
      const state = await snapshot();
      return state.state === 'playing' && state.track?.id === id && state.playId === selected.playId && state.position > .2 && state;
    }, `native fixture playback for ${id}`);
  }
  let online = true;
  Object.defineProperty(navigator, 'onLine', { configurable: true, get: () => online });
  function connectivity(next) {
    online = next;
    window.dispatchEvent(new Event(next ? 'online' : 'offline'));
  }
  try {
    ({ epoch } = await wait(async () => {
      const account = await invoke('account_snapshot');
      return account.status === 'authenticated' && ready(launcher()) && account;
    }, 'native synthetic account'));
    layout = (await invoke('pc_presentation', { epoch })).effectCatalogueNew ? 'modern' : 'legacy';
    wanted = layout === 'legacy' ? 1 : 2;
    stage = 'first-track';
    const first = await play('1', 'vivid');
    click(launcher());
    await wait(() => shell()?.dataset.audioEffectLayout === layout && matches(undefined), 'initial directory with no applied mark');
    const original = cards(300)[0];
    click(original);
    await wait(() => original.getAttribute('aria-expanded') === 'true' && ready(action('prepare')) && matches(undefined),
      'merely inspecting the first card does not enable it');
    await capture('01_viewed_only', false, 0);
    click(action('prepare'));
    await wait(() => ready(action('activate')), 'prepared synthetic PEQ');
    click(action('activate'));
    await wait(async () => matches('configured') && (await snapshot()).parametricProcessing, 'native activation sets the exact card');
    assert(last.playId === first.playId, 'applying does not reload playback');
    click(cards(301)[0]);
    await wait(() => cards(301)[0].getAttribute('aria-expanded') === 'true' && matches('configured') &&
      shell()?.querySelector('.effect-directory-detail')?.textContent.includes('此音效详情暂不可用'),
      'viewing another row does not move the applied check');
    await capture('02_other_viewed', true);

    stage = 'reopen';
    click(shell()?.querySelector('[aria-label="关闭音效"]'));
    await wait(() => !shell() && document.activeElement === launcher(), 'dialog closes to its real owner');
    click(launcher());
    await wait(() => matches('configured') && !shell().querySelector('[data-effect-id][aria-expanded="true"]'),
      'fresh drawer retains the confirmed mark without a viewed row');
    await capture('03_reopened', true);

    connectivity(false);
    await wait(() => shell()?.textContent.includes('当前离线') && !cards(300).length, 'offline unmounts catalogue');
    await capture('04_offline', true, 0, true);
    connectivity(true);
    await wait(() => matches('configured'), 'online remount recovers the current mark');

    stage = 'source-bypass';
    const dry = await play('2', 'standard');
    await wait(() => matches('bypassed_for_source'), 'dry source retains the mark with an explicit nonprocessing label');
    assert(dry.actualQuality === 'vivid' && !dry.parametricProcessing, 'authorized source, not requested quality, triggered bypass');
    await capture('05_source_bypass', false);

    stage = 'source-restore';
    const wet = await play('3', 'standard');
    await wait(async () => matches('configured') && (await snapshot()).parametricProcessing, 'later source restores the same selected effect');
    assert(wet.actualQuality === 'dolby' && last.playId === wet.playId, 'no extra media load on restoration');
    await capture('06_source_restored', true);

    stage = 'stop';
    click(cards(300)[0]);
    await wait(() => ready(action('pause')), 'current registration management remains available');
    const stopped = await invoke('player_action', { epoch, action: { kind: 'stop' } });
    assert(stopped.state === 'idle' && stopped.appliedEffectId === undefined, 'ordinary Stop clears the public mark');
    await wait(() => matches(undefined) && cards(300)[0].getAttribute('aria-expanded') === 'true' && ready(action('resume')),
      'stop leaves the viewed row open but clears the applied mark');
    await capture('07_stopped', false, 0);
    click(action('resume'));
    await wait(async () => {
      const state = await snapshot();
      return matches('configured') && state.state === 'idle' && !state.parametricProcessing;
    }, 'explicit restore configures the same row without starting music or claiming PCM');
    await capture('08_restored_idle', false);
    await emit('complete', { ok: true, layout });
  } catch (error) {
    await emit('failed', { step: stage, message: String(error), last, panel: shell()?.textContent.trim().slice(0, 500) });
  }
})();
