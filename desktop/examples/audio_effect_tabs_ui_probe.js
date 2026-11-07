// One real dialog, production tab/IPC paths and a real paused null decoder.
(async () => {
  const invoke = (command, args) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, extra = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...extra } });
  const one = (selector) => document.querySelector(selector);
  const shell = () => one('.audio-effects-panel[open]');
  const tab = (key) => shell()?.querySelector(`[data-audio-effect-tab="${key}"]`);
  const pane = (key) => shell()?.querySelector(`[data-audio-effect-pane="${key}"]:not([hidden])`);
  const launcher = () => one('.playerbar .audio-effects-trigger');
  const trigger = () => one('.player-track-actions [aria-label="当前歌曲更多操作"]');
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (value, message) => { if (!value) throw Error(message); };
  const ready = (node) => node?.isConnected && !node.disabled && node;
  const keys = ['catalogue', 'devices', 'equalizer'];
  const labels = ['音效', '设备适配', '均衡器'];
  async function wait(read, message) {
    for (let attempt = 0; attempt < 230; attempt++) {
      const value = await read();
      if (value) return value;
      await sleep(65);
    }
    throw Error(message);
  }
  function click(node) {
    assert(ready(node), 'click requires a visible enabled control');
    node.focus({ preventScroll: true });
    node.click();
  }
  function key(name) {
    document.activeElement.dispatchEvent(new KeyboardEvent('keydown', {
      key: name, bubbles: true, cancelable: true,
    }));
  }
  let epoch, step = 'startup', stageCount = 0, baseline, last;
  const snapshot = async () => (last = await invoke('player_snapshot', { epoch }));
  let online = true;
  Object.defineProperty(navigator, 'onLine', { configurable: true, get: () => online });
  function connectivity(next) {
    online = next;
    window.dispatchEvent(new Event(online ? 'online' : 'offline'));
  }
  function selected(key) {
    const active = tab(key);
    const content = pane(key);
    return active?.getAttribute('aria-selected') === 'true' && active.tabIndex === 0 &&
      active.getAttribute('aria-controls') === content?.id &&
      content.getAttribute('aria-labelledby') === active.id &&
      document.activeElement === active;
  }
  async function capture(label, active) {
    step = label;
    assert(selected(active), 'selected tab and focus agree');
    assert(document.querySelectorAll('dialog[open]').length === 1 &&
      shell().querySelectorAll('[role="tabpanel"]:not([hidden])').length === 1,
      'tabs share one modal and expose one panel');
    assert(keys.every((key, index) => tab(key).textContent.trim() === labels[index] &&
      (key === active || tab(key).tabIndex === -1 && tab(key).getAttribute('aria-selected') === 'false')),
      'all three official tabs and roving tabindex');
    const box = shell().getBoundingClientRect();
    assert(box.left >= 0 && box.top >= 0 && box.right <= innerWidth && box.bottom <= innerHeight,
      'dialog remains inside the short window');
    const state = await snapshot();
    assert(state.state === 'paused' && state.playId === baseline.playId &&
      state.entryId === baseline.entryId && state.queueRevision === baseline.queueRevision &&
      state.position === baseline.position && state.equalizer.revision === baseline.equalizer.revision,
      'navigation never changes playback or the native effect revision');
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await sleep(120);
    const stage = `ux_${String(++stageCount).padStart(2, '0')}_${label}`;
    await emit(stage, { active, online, stageCount, observedPosition: state.position,
      connectedTabs: [...shell().querySelectorAll('[role="tab"]')].map((node) => node.textContent.trim()) });
    await wait(() => window.__audioEffectTabRecorded === stage, `native receipt for ${label}`);
  }
  async function close(owner) {
    click(shell()?.querySelector('[aria-label="关闭音效"]'));
    await wait(() => !one('.audio-effects-panel') && document.activeElement === owner(),
      'one close returns to the actual opening control');
  }
  try {
    ({ epoch } = await wait(async () => {
      const account = await invoke('account_snapshot');
      return account.status === 'authenticated' && ready(launcher()) &&
        ready(one('.track-row .track-play')) && !one('.page-enter-active,.page-leave-active') && account;
    }, 'authenticated current window, player and daily row'));
    step = 'native-playing';
    click(one('.track-row .track-play'));
    await wait(async () => {
      const state = await snapshot();
      return state.state === 'playing' && state.playId && state.position > .25;
    }, 'real native null playback clock');
    click(await wait(() => ready(one('.playerbar [aria-label="暂停"]')), 'pause button'));
    let observed;
    baseline = await wait(async () => {
      const state = await snapshot();
      if (state.state !== 'paused' || state.nativeSeeking || !Number.isFinite(state.position)) return false;
      if (observed?.playId === state.playId && observed.position === state.position) return state;
      observed = state;
      return false;
    }, 'settled paused clock baseline');
    await emit('resize', { width: 760, height: 540 });
    await wait(() => innerWidth === 760 && innerHeight === 540, 'actual compact viewport');

    // Open offline first. Catalogue must not mount or make a request.
    connectivity(false);
    click(launcher());
    await wait(() => selected('catalogue') && pane('catalogue').textContent.includes('当前离线'),
      'offline first open does not mount the online catalogue');
    await capture('offline_default', 'catalogue');
    key('End');
    await wait(() => selected('equalizer') &&
      pane('equalizer').querySelectorAll('.equalizer-band input:not(:disabled)').length === 10,
      'ten real local equalizer controls are available offline');
    await capture('offline_equalizer', 'equalizer');
    key('Home');
    await wait(() => selected('catalogue'), 'Home selects the first tab');
    connectivity(true);
    await wait(() => selected('catalogue') && pane('catalogue').textContent.includes('合成空间音效'),
      'online catalogue mounts and reads its first group');
    await capture('online_catalogue', 'catalogue');
    key('ArrowRight');
    await wait(() => selected('devices') && pane('devices').textContent.includes('合成耳机品牌'),
      'device tab separately loads the native brand list');
    await capture('device_brands', 'devices');
    const brand = pane('devices').querySelector('select');
    brand.value = '4100';
    brand.dispatchEvent(new Event('change', { bubbles: true }));
    await wait(() => pane('devices').textContent.includes('合成耳机甲'), 'exact selected brand models');
    await capture('device_models', 'devices');
    click(tab('equalizer'));
    await wait(() => selected('equalizer') && pane('equalizer').querySelector('.equalizer-controls'),
      'mouse selects the same third tab in the same modal');
    await capture('online_equalizer', 'equalizer');
    await close(launcher);
    await captureClosedShortcut();

    step = 'current-track-menu';
    click(await wait(() => ready(trigger()), 'current track menu trigger'));
    const menu = await wait(() => ready(one('[data-menu-key="effects"]')), 'current track effects action');
    click(menu);
    await wait(() => selected('catalogue') && pane('catalogue').textContent.includes('合成空间音效'),
      'menu opens same default route, not the previous Equalizer tab');
    await capture('menu_default', 'catalogue');
    await close(trigger);
    await emit('complete', { ok: true, stageCount, observedPosition: baseline.position,
      closedFocus: document.activeElement?.getAttribute('aria-label') });
  } catch (error) {
    await emit('failed', { step, stageCount, message: String(error), last,
      focus: document.activeElement?.outerHTML.slice(0, 400),
      panel: shell()?.textContent.trim().slice(0, 500) });
  }
  async function captureClosedShortcut() {
    step = 'explicit-eq-shortcut';
    const shortcut = () => one('.playerbar [aria-label="均衡器"]');
    click(await wait(() => ready(shortcut()), 'explicit separate equalizer shortcut'));
    const direct = await wait(() => one('dialog.equalizer-dialog[open]'), 'direct equalizer remains available');
    assert(!shell() && direct.querySelectorAll('.equalizer-band input').length === 10,
      'shortcut goes directly to real EQ without reading catalogue');
    click(direct.querySelector('[aria-label="关闭均衡器"]'));
    await wait(() => !one('dialog.equalizer-dialog') && document.activeElement === shortcut(),
      'direct shortcut retains its own focus return');
  }
})();
