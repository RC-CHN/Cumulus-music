// Headphone setting and feedback use the real WebView, IPC, and libmpv.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (condition, message) => { if (!condition) throw new Error(message); };
  const wait = async (check, message) => {
    for (let i = 0; i < 160; i++) {
      const result = await check();
      if (result) return result;
      await sleep(65);
    }
    throw new Error(message);
  };
  const invoke = (command, args) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, details = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...details },
  });
  const findButton = (root, label) => [...(root?.querySelectorAll('button') ?? [])].find(
    (node) => node.getAttribute('aria-label') === label || node.textContent.trim() === label,
  );
  const click = (node, label) => {
    assert(node && !node.disabled, `missing control: ${label}`);
    node.focus({ preventScroll: true });
    node.click();
  };
  const eq = () => one('.equalizer-dialog[open]');
  const dialog = () => one('.effect-directory[open]');
  const launcher = () => one('.playerbar .equalizer-toggle');
  const tuning = () => one('[data-device-tuning]');
  const material = () => one('.effect-device-material:not([data-device-tuning])');
  const selects = () => [...(dialog()?.querySelectorAll('select') ?? [])];
  const choose = (node, value) => {
    node.value = String(value);
    assert(node.value === String(value), `option ${value} not found`);
    node.dispatchEvent(new Event('change', { bubbles: true }));
  };
  let sequence = 0;
  async function stats() {
    const sent = ++sequence;
    await emit('effect-source-native-stats', { sequence: sent });
    const result = await wait(() => window.__headphoneSourceStats?.sequence === sent &&
      window.__headphoneSourceStats.stats, 'native PCM status receipt');
    return result;
  }
  async function capture(stage) {
    await sleep(100);
    await emit(stage, { captureHandshake: true });
    await wait(() => window.__headphoneSourceStageRecorded === stage, `recorded ${stage}`);
  }
  try {
    const account = await wait(async () => {
      const current = await invoke('account_snapshot', {});
      return current.status === 'authenticated' && current;
    }, 'isolated account ready');
    const player = () => invoke('player_snapshot', { epoch: account.epoch });
    await wait(async () => (await player()).state === 'idle' && launcher() && !launcher().disabled,
      'initial native player is ready');
    click(launcher(), '均衡器');
    await wait(eq, 'equalizer opens');
    click(findButton(eq(), '耳机音效'), '耳机音效');
    const browse = await wait(() => findButton(dialog(), '浏览耳机音效'), 'local management');
    click(browse, '浏览耳机音效');
    await wait(() => selects()[0]?.textContent.includes('合成耳机品牌'), 'headphone brands');
    choose(selects()[0], 4100);
    await wait(() => selects()[1]?.textContent.includes('合成耳机乙'), 'headphone models');
    choose(selects()[1], 4102);
    click(await wait(() => findButton(material(), '检查调音材料'), 'selected headphone'), '检查调音材料');
    const enable = await wait(() => findButton(material(), '启用耳机调音'), 'verified material');
    click(enable, '启用耳机调音');
    const configured = await wait(async () => {
      const view = await player();
      return view.state === 'idle' && view.deviceParametric?.status === 'configured' &&
        tuning()?.dataset.deviceTuning === 'configured' && view.deviceParametric;
    }, 'real native headphone is configured but not yet processing');
    assert((await stats()).processedSamples === 0, 'idle configuration is not PCM processing');
    const remembered = await invoke('effect_device_remembered', { epoch: account.epoch });
    click(findButton(dialog(), '关闭耳机音效管理'), '关闭耳机音效管理');
    await wait(() => !dialog(), 'headphone directory closes');
    click(findButton(eq(), '关闭均衡器'), '关闭均衡器');
    await wait(() => !eq(), 'main playback footer');

    const stages = [
      ['ux_01_fallback_standard', '71', 'vivid', 'standard', false, 1, 0],
      ['ux_02_vivid_bypassed', '72', 'standard', 'vivid', true, 1, 1],
      ['ux_03_consecutive_dolby_restored', '73', 'standard', 'dolby', false, 2, 1],
      ['ux_04_sky_bypassed', '74', 'standard', 'sky', true, 2, 2],
      ['ux_05_jymaster_restored', '75', 'standard', 'jymaster', false, 3, 2],
    ];
    for (const [index, [stage, id, requested, actual, bypassed, attaches, bypasses]] of stages.entries()) {
      const playback = await invoke('play_selection', { epoch: account.epoch, selection: {
        kind: 'songs', ids: [id], index: 0, quality: requested,
      } });
      const status = bypassed ? 'bypassed_for_source' : 'configured';
      await wait(async () => {
        const view = await player();
        return view.state === 'playing' && view.track?.id === id && view.position > .3 &&
          view.playId === playback.playId && view.actualQuality === actual &&
          view.deviceParametric?.status === status &&
          view.deviceParametric.activationId === configured.activationId &&
          !!view.deviceParametric.comparison === !bypassed &&
          view.parametricProcessing === !bypassed &&
          launcher()?.dataset.headphoneState === status &&
          launcher()?.title.includes(bypassed ? '当前音源已旁路耳机调音' : '耳机调音正在处理音频');
      }, `${stage}: native player and footer agree`);
      await wait(async () => {
        const native = await stats();
        return native.nativeAttached && native.decoderLoads === index + 1 &&
          native.attaches === attaches && native.bypasses === bypasses && native.detaches === 0 &&
          native.revision === native.observedRevision && native.processedSamples > 0 &&
          native.failedBlocks === 0 && (bypassed ? native.bypassRevision === native.revision :
            native.bypassRevision === null);
      }, `${stage}: actual node acknowledged the exact configuration revision`);
      click(launcher(), '管理耳机音效');
      await wait(() => tuning()?.dataset.deviceTuning === status &&
        tuning().textContent.includes(bypassed ? '当前音源已旁路耳机调音' : '耳机调音正在处理音频') &&
        !!findButton(tuning(), '暂时停用调音') === !bypassed, `${stage}: usable management state`);
      await capture(stage);
      click(findButton(dialog(), '关闭耳机音效管理'), '关闭耳机音效管理');
      await wait(() => !dialog() && document.activeElement === launcher(), 'focus returns to footer');
    }
    const after = await invoke('effect_device_remembered', { epoch: account.epoch });
    assert(after.revision === remembered.revision, 'auto bypass did not alter saved preference');
    await emit('complete', { ok: true, stageCount: stages.length });
  } catch (error) {
    await emit('failed', { message: String(error), body: document.body.innerText.slice(-2400),
      nativeStats: window.__headphoneSourceStats });
  }
})();
