// Exercise the shared header switch in both official catalogue layouts.
(async () => {
  const invoke = (command, args) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, extra = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...extra } });
  const one = (selector) => document.querySelector(selector);
  const shell = () => one('.audio-effects-panel[open]');
  const launcher = () => one('.playerbar .audio-effects-trigger');
  const tab = (key) => shell()?.querySelector(`[data-audio-effect-tab="${key}"]`);
  const header = () => shell()?.querySelector('.audio-effects-headphone-master');
  const pane = (key) => shell()?.querySelector(`[data-audio-effect-pane="${key}"]:not([hidden])`);
  const button = (root, label) => [...(root?.querySelectorAll('button') ?? [])].find(
    (node) => node.getAttribute('aria-label') === label || node.textContent.trim() === label);
  const deviceControls = () => pane('devices')?.querySelector('[data-device-tuning]');
  const ready = (node) => node?.isConnected && !node.disabled && node;
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (ok, label) => { if (!ok) throw Error(label); };
  let epoch, layout, stage = 'startup', last;
  async function wait(read, label) {
    for (let i = 0; i < 240; i++) {
      const value = await read();
      if (value) return value;
      await sleep(65);
    }
    throw Error(label);
  }
  function click(node) {
    assert(ready(node), 'expected enabled control');
    node.focus({ preventScroll: true });
    node.click();
  }
  function choose(node, value) {
    node.value = String(value);
    assert(node.value === String(value), `option ${value} exists`);
    node.dispatchEvent(new Event('change', { bubbles: true }));
  }
  const snapshot = async () => (last = await invoke('player_snapshot', { epoch }));
  async function play(id, quality) {
    const requested = await invoke('play_selection', { epoch, selection: { kind:'songs', ids:[id], index:0, quality } });
    return wait(async () => {
      const state = await snapshot();
      return state.state === 'playing' && state.playId === requested.playId &&
        state.track?.id === id && state.position > .25 && state;
    }, `native fixture playback for ${id}`);
  }
  async function tabTo(key) {
    click(tab(key));
    await wait(() => tab(key)?.getAttribute('aria-selected') === 'true' && pane(key) &&
      document.activeElement === tab(key), `selected tab ${key}`);
  }
  function headerIs(checked) {
    return header()?.getAttribute('role') === 'switch' &&
      header().getAttribute('aria-label') === '耳机调音' &&
      header().getAttribute('aria-checked') === String(checked) &&
      header().getAttribute('aria-disabled') !== 'true';
  }
  async function capture(name) {
    stage = name;
    await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));
    const receipt = `ux_${name}`;
    await emit(receipt, {captureHandshake:true, layout,
      checked: header()?.getAttribute('aria-checked') ?? null,
      activeTab: shell()?.querySelector('[role="tab"][aria-selected="true"]')?.dataset.audioEffectTab,
      eqMaster: !!shell()?.querySelector('.local-equalizer-master.audio-effects-master'),
      insideComparison: !!deviceControls()?.querySelector('[data-comparison-receipt]'),
    });
    await wait(() => window.__headphoneSourceStageRecorded === receipt, `recorded ${name}`);
  }
  async function toggle(wantOn, samePlay) {
    const control = header();
    const before = await snapshot();
    click(control);
    assert(control.getAttribute('aria-checked') === String(!wantOn), 'no optimistic toggle');
    const after = await wait(async () => {
      const state = await snapshot();
      return state.playId === samePlay && state.entryId === before.entryId &&
        state.position > before.position && state.deviceParametric?.status === (wantOn ? 'configured' : 'paused_locally') &&
        state.deviceParametric.comparison?.receipt !== before.deviceParametric.comparison.receipt &&
        state.parametricProcessing === wantOn && headerIs(wantOn) && state;
    }, 'same playback observes the new comparison receipt and native PCM state');
    assert(document.activeElement === control && header() === control, 'local operation preserves switch focus and identity');
    return after;
  }

  try {
    ({ epoch } = await wait(async () => {
      const account = await invoke('account_snapshot');
      return account.status === 'authenticated' && ready(launcher()) && account;
    }, 'synthetic account ready'));
    layout = (await invoke('pc_presentation', {epoch})).effectCatalogueNew ? 'modern' : 'legacy';
    stage = 'playing';
    const first = await play('71','vivid');
    assert(first.actualQuality === 'standard', 'native actual quality starts on a compatible source');
    click(launcher());
    await wait(() => shell()?.dataset.audioEffectLayout === layout && pane('catalogue')?.querySelector('[data-effect-group]'),
      'the assigned catalogue is ready');
    assert(!header(), 'no switch without a verified device');
    await capture('01_no_headphone');

    stage = 'apply';
    await tabTo('devices');
    const brand = await wait(() => pane('devices')?.querySelector('select option[value="4100"]')?.closest('select'), 'brand');
    choose(brand,4100);
    const model = await wait(() => pane('devices')?.querySelector('select option[value="4102"]')?.closest('select'), 'model');
    choose(model,4102);
    click(await wait(() => ready(button(pane('devices'),'检查调音材料')), 'check material'));
    click(await wait(() => ready(button(pane('devices'),'启用耳机调音')), 'apply material'));
    const applied = await wait(async () => {
      const state = await snapshot();
      return state.deviceParametric?.status === 'configured' && state.parametricProcessing && headerIs(true) && state.deviceParametric;
    }, 'confirmed real native headphone processing');
    const remembered = await invoke('effect_device_remembered', {epoch});
    assert(!deviceControls()?.querySelector('[data-comparison-receipt]'), 'device page does not duplicate the shared comparison');
    await capture('02_enabled_in_devices');

    await tabTo('catalogue');
    await wait(() => pane('catalogue')?.querySelector('[data-effect-group]'), 'catalogue remount completes before comparison');
    const sameHeader = header();
    await capture('03_enabled_in_catalogue');
    await toggle(false, first.playId);
    await capture('04_paused_in_catalogue');
    await tabTo('equalizer');
    assert(header() === sameHeader && headerIs(false), 'one header survives tab changes');
    await toggle(true, first.playId);
    await capture('05_resumed_in_equalizer');

    stage = 'bypass';
    const stale = last.deviceParametric.comparison;
    assert(document.activeElement === header(), 'focused header before dry source');
    const dry = await play('72','standard');
    assert(dry.actualQuality === 'vivid', 'native authorized quality rather than request triggers bypass');
    await wait(() => !header() && document.activeElement === tab('equalizer'), 'dry source hides comparison and returns focus');
    let rejected = false;
    try {
      await invoke('effect_device_compare', { epoch, comparison:{
        ...stale, brand:applied.brand, model:applied.model, activationId:applied.activationId, paused:true,
      } });
    } catch (issue) { rejected = issue?.code === 'stale'; }
    assert(rejected, 'a previous source receipt is rejected natively');
    await capture('06_source_bypass');

    stage = 'reopen';
    const restored = await play('73','standard');
    await wait(async () => headerIs(true) && (await snapshot()).parametricProcessing, 'compatible source returns current switch');
    assert(restored.actualQuality === 'dolby' && last.deviceParametric.activationId === applied.activationId, 'native source policy retains same device');
    await toggle(false, restored.playId);
    click(button(shell(),'关闭音效'));
    await wait(() => !shell() && document.activeElement === launcher(), 'focus returns to launcher');
    click(launcher());
    await wait(() => pane('catalogue')?.querySelector('[data-effect-group]'), 'reopened catalogue');
    assert(layout === 'modern' ? !header() : headerIs(false), 'only legacy exposes an already disabled initial device choice');
    await tabTo('devices');
    await wait(() => deviceControls()?.dataset.deviceTuning === 'paused_locally', 'paused device remains available');
    if (layout === 'modern') {
      assert(!header(), 'modern mount does not use the unsupported recommendation toggle');
      await capture('07_reopened_paused');
      click(button(deviceControls(),'继续当前调音'));
      await wait(async () => headerIs(true) && (await snapshot()).parametricProcessing, 'existing device control restores; header then takes ownership');
      assert(document.activeElement === header(), 'focus follows the device comparison into the header');
    } else {
      assert(!deviceControls()?.querySelector('[data-comparison-receipt]'), 'legacy shares its one enabled control');
      await capture('07_reopened_paused');
      await toggle(true, restored.playId);
    }
    assert(last.playId === restored.playId && last.deviceParametric.activationId === applied.activationId &&
      (await invoke('effect_device_remembered', {epoch})).revision === remembered.revision, 'no new playback or preference write');
    await capture('08_reopened_resumed');
    await emit('complete', {ok:true,layout});
  } catch (error) {
    await emit('failed', {step:stage,message:String(error),last,focus:document.activeElement?.outerHTML.slice(0,260),
      panel:shell()?.textContent.trim().slice(0,500)});
  }
})();
