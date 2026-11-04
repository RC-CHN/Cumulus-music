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

    if (window.__cumulusPeqPlayerStop) {
      const selectedEffect = { epoch, id: 300 };
      if (window.__cumulusPeqResume) {
        const active = await invoke('effect_peq_status', selectedEffect);
        assert(!active.canResume && !action('resume'), 'bound effect must not offer another resume');
        let duplicate;
        try {
          await invoke('effect_peq_activate', { ...selectedEffect, attempt: active.release, resumeExisting: true });
        } catch (issue) { duplicate = issue; }
        assert(duplicate?.code === 'busy', 'native layer rejects a duplicate resume of the active binding');
      }
      stage = 'player-stop';
      const stopped = await invoke('player_action', { epoch, action: { kind: 'stop' } });
      assert(stopped.state === 'idle' && !stopped.boundParametric, 'ordinary stop clears the local PEQ');
      await wait(() => phase('recovery_required') && enabled(action('stop')) &&
        controls().textContent.includes('仍有登记待'), 'visible PEQ details update after ordinary stop');
      await capture('03_player_stopped');
      if (window.__cumulusPeqResume) {
        stage = 'resume-existing';
        const detached = await invoke('effect_peq_status', selectedEffect);
        assert(detached.canResume && detached.registration === 'registered', 'only an acknowledged lease can resume');
        click(await wait(() => enabled(action('resume')), 'resume the existing registration'));
        await wait(() => phase('configured') && !action('resume'), 'same registration configured again');
        let stale;
        try {
          await invoke('effect_peq_activate', { ...selectedEffect, attempt: detached.release, resumeExisting: true });
        } catch (issue) { stale = issue; }
        assert(stale?.code === 'stale', 'a spent resume receipt cannot be reused');
        await invoke('play_selection', { epoch, selection: {
          kind: 'songs', ids: ['1'], index: 0, quality: 'standard',
        } });
        await wait(async () => {
          const current = await player();
          return current.state === 'playing' && current.boundParametric === 'configured' && current.position > 0.35;
        }, 'real playback advances through the resumed PEQ');
        await capture('04_resumed_registration');
        await emit('complete', { ok: true, stageCount: 4, resumedRegistrationVerified: true });
        return;
      }
      click(action('stop'));
      await wait(() => phase('recovery_required') && enabled(action('stop')) &&
        action('stop').textContent.includes('未确认'), 'explicit cleanup response is uncertain');
      click(action('stop'));
      click(await wait(() => enabled(action('confirm-stop')), 'confirm exact cleanup retry'));
      await wait(() => phase('idle') && !one('[data-effect-registration="300"]'),
        'explicit cleanup releases the local registration');
      await capture('04_player_stop_released');
      await emit('complete', { ok: true, stageCount: 4, playerStopStatusVerified: true });
      return;
    }

    stage = 'direct-management';
    click(button(one('.effect-directory[open]'), '关闭音效目录'));
    await wait(() => !one('.effect-directory[open]'), 'initial directory closes');
    const eq = one('.equalizer-dialog[open]');
    const master = button(eq, '启用均衡器和低高音增强');
    const separate = await wait(() => eq?.querySelector('.equalizer-independent[data-parametric-state="configured"]'),
      'equalizer explains the independently configured PEQ');
    assert(master?.textContent.includes('均衡器与低高音') && separate.textContent.includes('不受上方开关控制'),
      'the local bypass is not presented as stopping PEQ');
    click(master);
    await wait(async () => {
      const state = await player();
      return master.getAttribute('aria-checked') === 'false' && state.equalizer.settings.master_bypassed &&
        state.boundParametric === 'configured' && state.state === 'playing' && state.playId === baseline.playId;
    }, 'local bypass retains native PEQ and the playing entry');
    click(master);
    await wait(async () => {
      const state = await player();
      return master.getAttribute('aria-checked') === 'true' && !state.equalizer.settings.master_bypassed &&
        state.boundParametric === 'configured' && state.playId === baseline.playId;
    }, 'local bypass restoration retains native PEQ');
    click(button(separate, '管理独立参数均衡'));
    const fromPanel = await wait(() => one('.effect-directory[open]'), 'EQ panel opens management directly');
    assert(button(fromPanel, '浏览音效目录') && !fromPanel.querySelector('[aria-label="音效分类"]'),
      'EQ panel opens local management without browsing the catalogue');
    assert(await wait(() => enabled(fromPanel.querySelector('[data-effect-registration="300"]')),
      'EQ panel management shows the active registration'));
    click(button(fromPanel, '关闭音效目录'));
    await wait(() => !one('.effect-directory[open]'), 'EQ panel management closes');
    click(button(one('.equalizer-dialog[open]'), '关闭均衡器'));
    await wait(() => !one('.equalizer-dialog[open]'), 'initial equalizer closes');
    click(await wait(() => enabled(one('.equalizer-toggle[aria-label="管理独立参数均衡"]')),
      'bound footer management shortcut'));
    const local = await wait(() => one('.effect-directory[open]'), 'footer opens management directly');
    assert(button(local, '浏览音效目录') && !local.querySelector('[aria-label="音效分类"]'),
      'management opens without starting catalogue navigation');
    click(await wait(() => enabled(local.querySelector('[data-effect-registration="300"]')),
      'bound registration appears in local management'));
    await wait(() => phase('configured') && enabled(action('stop')), 'reopened local controls retain native binding');

    stage = 'late-stop';
    const provides = one('#app')?.__vue_app__?._context.provides;
    const core = provides?.[Reflect.ownKeys(provides).find((key) => key.description === 'core-client')];
    assert(core, 'production CoreClient is available');
    const stopNative = core.effectPeqStop;
    let resumeStop, startedStop = false;
    const holdStop = new Promise((resolve) => { resumeStop = resolve; });
    core.effectPeqStop = async (...args) => {
      startedStop = true;
      await holdStop;
      return stopNative(...args);
    };
    const registration = () => enabled(one('[data-effect-registration="300"]'));
    try {
      click(action('stop'));
      await wait(() => startedStop, 'stop was initiated before closing management');
      click(button(one('.effect-directory[open]'), '关闭音效目录'));
      await wait(() => !one('.effect-directory[open]'), 'starting directory closes');
      click(button(one('.equalizer-dialog[open]'), '音效目录'));
      const beforeStop = await wait(registration, 'new directory reads the registered state');
      await wait(() => one('.effect-directory[open] .effect-directory-card'), 'new catalogue load completes');
      assert(!beforeStop.textContent.includes('停止待核对'), 'stop has not finished yet');
      resumeStop();
      await wait(() => registration()?.textContent.includes('停止待核对'),
        'already open directory reflects a late uncertain stop');
    } finally {
      resumeStop();
      core.effectPeqStop = stopNative;
    }
    click(await wait(registration, 'durable registration shown independently of selection'));
    await wait(() => phase('recovery_required') && enabled(action('stop')) && enabled(action('status')),
      'open recovery without catalogue detail');
    assert(!action('prepare') && !action('activate'), 'recovery-only view never prepares or activates');
    await wait(() => !one('.equalizer-toggle')?.dataset.parametricState &&
      !one('.equalizer-toggle').textContent.includes('PEQ') && !one('.equalizer-independent'),
      'footer and EQ panel clear the detached native component');
    await capture('03_stop_uncertain');
    const detached = await player();
    await wait(async () => {
      const state = await player();
      return state.state === 'playing' && state.playId === baseline.playId && state.position > detached.position + 0.25;
    }, 'ordinary native playback continues after detach');

    if (window.__cumulusPeqExitUncertain) {
      await emit('complete', { ok: true, stageCount: 3, leftStopUncertain: true, lateStopVerified: true });
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
    await emit('complete', { ok: true, stageCount: 5, sameNativePlayback: true, directManagementVerified: true, scopedBypassVerified: true, lateStopVerified: true });
  } catch (error) {
    await emit('failed', { step: stage, message: String(error), stack: error?.stack,
      phase: controls()?.getAttribute('data-peq-phase') });
  }
})();
