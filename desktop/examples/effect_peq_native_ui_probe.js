// Production Vue + native Tauri effects IPC; the Rust host closes all origins.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (condition, message) => { if (!condition) throw new Error(message); };
  const wait = async (read, message) => {
    for (let index = 0; index < 150; index++) {
      const value = await read();
      if (value) return value;
      await sleep(65);
    }
    throw new Error(message);
  };
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const button = (root, name) => [...(root?.querySelectorAll('button') ?? [])].find(
    (node) => node.getAttribute('aria-label') === name || node.textContent.trim() === name,
  );
  const enabled = (node) => node && !node.disabled && node.getAttribute('aria-disabled') !== 'true' && node;
  const click = (node) => {
    assert(enabled(node), 'enabled control expected');
    node.focus({ preventScroll: true });
    node.click();
  };
  const controls = () => one('.effect-parametric');
  const action = (name) => controls()?.querySelector(`button[data-peq-action="${name}"]`);
  const phase = (name) => controls()?.getAttribute('data-peq-phase') === name;
  async function capture(name) {
    await emit(`ux_${name}`, {captureHandshake: true});
    await sleep(650);
  }

  let stage = 'startup';
  try {
    const account = await wait(async () => {
      const value = await invoke('account_snapshot');
      return value.status === 'authenticated' && value;
    }, 'synthetic account did not restore');
    const { epoch } = account;
    const player = () => invoke('player_snapshot', { epoch });
    await invoke('play_selection', { epoch, selection: {
      kind: 'songs', ids: ['1'], index: 0, quality: 'standard',
    } });
    const baseline = await wait(async () => {
      const state = await player();
      return state.state === 'playing' && state.position > 0.1 && state;
    }, 'native fixture WAV did not advance');
    click(await wait(() => enabled(one('.equalizer-toggle')), 'EQ launcher'));
    click(button(await wait(() => one('.equalizer-dialog[open]'), 'EQ dialog'), '音效目录'));
    click(await wait(() => enabled(one('.effect-directory[open] .effect-directory-card')), 'PEQ card'));
    await wait(() => phase('idle') && enabled(action('prepare')), 'initial PEQ status');
    assert(!enabled(action('activate')), 'activation requires a prepared receipt');
    stage = 'prepare';
    click(action('prepare'));
    await wait(() => phase('prepared') && enabled(action('activate')), 'prepared official material');
    await capture('01_prepared');

    stage = 'activate';
    click(action('activate'));
    await wait(() => phase('configured') && enabled(action('stop')), 'native configured receipt');
    await wait(() => one('.equalizer-toggle')?.dataset.parametricState === 'configured' &&
      one('.equalizer-toggle').textContent.includes('PEQ'), 'footer shows the native bound component');
    const started = await player();
    await wait(async () => {
      const state = await player();
      return state.state === 'playing' && state.playId === baseline.playId && state.position > started.position + 0.35;
    }, 'same native playback advances through PEQ');
    await capture('02_configured');

    stage = 'stop';
    click(action('stop'));
    await wait(() => phase('recovery_required') && enabled(action('stop')) && enabled(action('status')),
      'lost stop acknowledgement remains unresolved');
    await wait(() => !one('.equalizer-toggle')?.dataset.parametricState &&
      !one('.equalizer-toggle').textContent.includes('PEQ'), 'footer clears the detached native component');
    await capture('03_stop_uncertain');
    const detached = await player();
    await wait(async () => {
      const state = await player();
      return state.state === 'playing' && state.playId === baseline.playId && state.position > detached.position + 0.25;
    }, 'ordinary native playback continues after detach');

    stage = 'reopen_directory';
    click(button(one('.effect-directory[open]'), '关闭音效目录'));
    await wait(() => !one('.effect-directory[open]'), 'effect directory closes');
    click(button(await wait(() => one('.equalizer-dialog[open]'), 'EQ dialog remains open'), '音效目录'));
    const recovery = await wait(() => enabled(one('[data-effect-registration="300"]')),
      'durable registration shown independently of selection');
    click(recovery);
    await wait(() => phase('recovery_required') && enabled(action('stop')), 'open recovery without catalogue detail');
    assert(!action('prepare') && !action('activate'), 'recovery-only view never prepares or activates');

    if (window.__cumulusPeqExitUncertain) {
      await emit('complete', { ok: true, stageCount: 3, leftStopUncertain: true });
      return;
    }

    stage = 'reconcile';
    click(action('status'));
    await sleep(0);
    await wait(() => phase('recovery_required') && enabled(action('status')) && enabled(action('stop')), 'local reconciliation does not settle lost acknowledgement');
    click(action('stop'));
    click(await wait(() => enabled(action('cancel-stop')), 'explicit retry confirmation'));
    await wait(() => !action('confirm-stop') && enabled(action('stop')), 'cancel retry keeps uncertain state');
    await capture('04_reconciled');

    stage = 'confirm';
    click(action('stop'));
    click(await wait(() => enabled(action('confirm-stop')), 'explicit retry confirmation reopened'));
    await wait(() => phase('idle') && !enabled(action('stop')) && !one('[data-effect-registration="300"]'),
      'acknowledged stop clears recovery and local list');
    const final = await player();
    assert(final.playId === baseline.playId && final.position > baseline.position, 'binding does not replace the ordinary queue');
    await capture('05_released');
    await emit('complete', { ok: true, stageCount: 5, sameNativePlayback: true });
  } catch (error) {
    await emit('failed', { step: stage, message: String(error), stack: error?.stack,
      phase: controls()?.getAttribute('data-peq-phase') });
  }
})();
