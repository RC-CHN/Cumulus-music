// Real PlayerBar, Rust diagnosis and Tauri settings routing; only the OS endpoint is sealed.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, extra = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...extra },
  });
  const bar = () => one('.playerbar');
  const trigger = () => bar()?.querySelector('.volume-trigger');
  const slider = () => bar()?.querySelector('.volume-popover input[type=range]');
  const alert = () => one('[data-volume-diagnosis]');
  const heading = () => one('#settings-audio-output');
  const daily = () => [...document.querySelectorAll('.nav-item')].find((node) => node.textContent.trim() === '每日推荐');
  const enabled = (node) => node?.isConnected && !node.matches(':disabled') &&
    !node.closest('[inert]') && node.getClientRects().length > 0;
  const playback = () => invoke('player_snapshot', { epoch });
  const playAll = () => one('.track-actions [data-recommendation-play]');
  let epoch, baseline, last = 'startup', count = 0;
  function assert(condition, message) { if (!condition) throw Error(message); }
  async function wait(test, message) {
    last = message;
    for (let i = 0; i < 180; i++) {
      const value = await test();
      if (value) return value;
      await sleep(65);
    }
    throw Error(message);
  }
  async function click(lookup, label) {
    await wait(() => {
      const node = lookup();
      if (!enabled(node)) return false;
      node.focus({ preventScroll: true });
      node.click();
      return true;
    }, label);
  }
  async function endpoint(mode) {
    window.__systemMasterVolumeFixtureAck = null;
    await emit('system-master-volume-fixture', { mode });
    await wait(() => window.__systemMasterVolumeFixtureAck === mode, 'endpoint: ' + mode);
  }
  function key(node, value) {
    node.dispatchEvent(new KeyboardEvent('keydown', { key: value, bubbles: true, cancelable: true }));
  }
  async function adjust(from, to) {
    const button = await wait(() => enabled(trigger()) && trigger(), 'enabled volume');
    button.focus({ preventScroll: true });
    key(button, 'ArrowUp');
    const input = await wait(() => enabled(slider()) && slider(), 'opened software range');
    assert(Number(input.value) === from, 'captured open volume: ' + input.value);
    input.value = String(to);
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await wait(async () => (await playback()).volume === to && Number(slider()?.value) === to &&
      slider()?.closest('[aria-busy]')?.getAttribute('aria-busy') === 'false', 'volume acknowledged: ' + to);
    key(input, 'Escape');
    await wait(() => trigger()?.getAttribute('aria-expanded') === 'false' && document.activeElement === trigger(),
      'Escape closes software popover and restores focus');
  }
  async function capture(label, visible, expectedVolume) {
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await sleep(300); // Let transition/GTK compositor settle before host captures the frame.
    const native = await playback();
    assert(native.state === 'paused' && native.playId === baseline.playId &&
      native.entryId === baseline.entryId && native.queueRevision === baseline.queueRevision &&
      native.position === baseline.position && native.volume === expectedVolume,
      'diagnosis leaves paused decoder/queue/clock unchanged');
    const box = alert()?.getBoundingClientRect();
    assert(!!box === visible && (!box || (box.left >= 0 && box.right <= innerWidth &&
      box.top >= 0 && box.bottom <= innerHeight)), 'expected diagnostic visibility and no clipped alert');
    if (visible) assert(alert().textContent.includes('当前系统设置可能影响播放体验') &&
      alert().textContent.includes('去查看') && document.activeElement === trigger(),
      'official prompt text with no stolen focus');
    const stage = 'ux_' + String(++count).padStart(2, '0') + '_' + label;
    window.__systemProbeStageRecorded = null;
    await emit(stage, { captureHandshake: true, label, visible, volume: native.volume, playId: native.playId,
      entryId: native.entryId, queueRevision: native.queueRevision, position: native.position,
      focused: document.activeElement?.id || document.activeElement?.getAttribute('aria-label'),
      outputPercent: one('[data-system-master-percent]')?.textContent.trim() ?? null });
    await wait(() => window.__systemProbeStageRecorded === stage, 'native evidence saved: ' + stage);
  }
  try {
    const account = await wait(async () => {
      const value = await invoke('account_snapshot');
      return value.status === 'authenticated' && enabled(daily()) && value;
    }, 'isolated account ready');
    epoch = account.epoch;
    await click(daily, 'open Daily');
    await wait(() => {
      const encoded = playAll()?.getAttribute('data-recommendation-play');
      const source = encoded && JSON.parse(encoded)[0];
      return one('.track-row[data-track-id="1"]') && typeof source === 'string' &&
        source.startsWith(epoch + ':daily:') && enabled(playAll());
    }, 'account-bound Daily receipt');
    await click(playAll, 'play sealed Daily');
    await wait(async () => {
      const value = await playback();
      return value.state === 'playing' && value.track?.id === '1' && value.position > .2 &&
        bar()?.dataset.playId === value.playId;
    }, 'real null decoder started');
    await click(() => bar()?.querySelector('[aria-label="暂停"]'), 'pause');
    await invoke('player_action', { epoch, action: { kind: 'volume', value: 60 } });
    baseline = await wait(async () => {
      const value = await playback();
      return value.state === 'paused' && value.volume === 60 &&
        trigger()?.title?.includes('60%') && value;
    }, 'paused and initial 60 percent');

    await adjust(60, 79);
    await capture('below_threshold', false, 79);
    if (window.__volumeDiagnosisUnsupported) {
      await adjust(79, 80);
      await capture('unsupported_after_increase', false, 80);
    } else {
      await endpoint('muted_spatial_off');
      await adjust(79, 80);
      await capture('muted', false, 80);
      await endpoint('thirty');
      await adjust(80, 86);
      await capture('at_system_threshold', false, 86);
      await endpoint('almost_thirty');
      await adjust(86, 82);
      await capture('decrease', false, 82);
      await adjust(82, 85);
      await wait(alert, 'raw 29.99 percent offers diagnosis');
      await capture('low_system_volume', true, 85);
      const presented = performance.now();
      await wait(() => !alert(), 'diagnosis times out');
      assert(performance.now() - presented >= 3800, 'diagnosis is not prematurely closed');
      await capture('automatic_close', false, 85);

      await endpoint('spatial_unavailable');
      await adjust(85, 87);
      await capture('spatial_unavailable', false, 87);

      await endpoint('recovered');
      await adjust(87, 90);
      await wait(alert, 'active spatial sound offers diagnosis with normal master volume');
      await capture('spatial_active', true, 90);
      await click(() => alert()?.querySelector('.volume-diagnosis-action'), 'explicit open audio output');
      await wait(() => !alert() && document.activeElement === heading() &&
        one('[data-system-master-percent]')?.textContent.trim() === '84%' &&
        one('[data-system-spatial-sound] [role=status]')?.textContent.trim() === '已开启（可能影响声音效果）',
        'output settings heading focused, normal master volume and active spatial sound displayed');
      await capture('settings', false, 90);

      await click(daily, 'return from Settings');
      await wait(() => !heading(), 'left output settings');
      await adjust(90, 92);
      await capture('remembered_cooldown', false, 92);
    }
    await emit('complete', { ok: true, stageCount: count, realOsWrites: 0,
      realMusicRequests: 0, realAccountWrites: 0 });
  } catch (cause) {
    await emit('failed', { ok: false, last, count, error: String(cause), stack: cause?.stack,
      native: epoch && await playback().catch(() => null), displayedVolume: slider()?.value ?? null,
      focus: document.activeElement?.outerHTML.slice(0, 400), alert: alert()?.textContent.trim(),
      outputPercent: one('[data-system-master-percent]')?.textContent.trim() ?? null });
  }
})();
