// No adapter monkey-patching. All settings and audio controls call registered
// production Tauri commands from the actual Vue component.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  function assert(value, message) {
    if (!value) throw Error(message);
  }
  async function wait(test, message) {
    for (let attempt = 0; attempt < 180; attempt++) {
      if (await test()) return;
      await sleep(60);
    }
    throw Error(message);
  }
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, value = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...value } });
  const phase = window.__CUMULUS_POWER_PROOF_PHASE__;
  const bar = () => one('.playerbar');
  const status = () => one('[data-playback-power-status]')?.textContent.trim() ?? '';
  const error = () => one('[data-playback-power-error]')?.textContent.trim() ?? '';
  const option = () => one('input[name="playback-keep-awake"]');
  const refresh = () => one('[aria-label="重新读取播放设置"]');
  const playbackReady = () => one('.playback-startup')?.getAttribute('aria-busy') === 'false';
  function click(node) {
    assert(node && !node.matches(':disabled'), 'enabled control: ' + node?.outerHTML);
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    node.click();
  }
  let last = 'startup', stageCount = 0, sequence = 0;
  window.__powerProbeReplies = Object.create(null);
  async function requests() {
    const seq = ++sequence;
    await emit('inspect-power', { sequence: seq });
    await wait(() => Object.hasOwn(window.__powerProbeReplies, seq), 'native power observation');
    const value = window.__powerProbeReplies[seq];
    delete window.__powerProbeReplies[seq];
    return value;
  }
  async function expectRequests(before, added) {
    const expected = [...before, ...added];
    await wait(async () => (await requests()).length >= expected.length, 'required native request');
    const actual = await requests();
    assert(actual.join() === expected.join(),
      'unexpected power trace: ' + JSON.stringify({ actual, expected }));
  }
  async function fixture(mode) {
    window.__powerFixtureAck = null;
    await emit(mode);
    await wait(() => window.__powerFixtureAck === mode, 'fixture acknowledged');
  }
  async function capture(label) {
    last = label;
    one('.playback-startup').scrollIntoView({ block: 'center' });
    await sleep(260);
    assert(document.documentElement.scrollWidth <= innerWidth, 'no horizontal overflow');
    assert(one('.settings-scroll').getBoundingClientRect().bottom <=
      bar().getBoundingClientRect().top + 1, 'settings do not overlap the player');
    const stage = 'ux_' + String(++stageCount).padStart(2, '0') + '_' + label;
    window.__powerStageAck = null;
    await emit(stage, {
      phase, width: innerWidth, height: innerHeight, theme: document.documentElement.dataset.theme,
      visible: !!option(), checked: option()?.checked ?? null, status: status(), error: error(),
      focus: document.activeElement?.getAttribute('name') || document.activeElement?.getAttribute('aria-label'),
    });
    await wait(() => window.__powerStageAck === stage, 'native evidence saved for ' + stage);
  }
  async function toggle(enabled, request, label) {
    const before = await requests();
    assert(option()?.checked === !enabled, 'toggle begins with confirmed value');
    click(option());
    await wait(() => playbackReady() && option()?.checked === enabled && status() === '设置已更新',
      'native preference commit');
    await wait(() => document.activeElement === option(), 'checkbox focus restored');
    await expectRequests(before, [request]);
    await capture(label);
  }
  async function transport(button, state, request, label) {
    const before = await requests();
    const position = Number(bar().dataset.position);
    click(bar().querySelector('[aria-label="' + button + '"]'));
    await wait(() => bar().dataset.state === state &&
      (state !== 'playing' || Number(bar().dataset.position) > position + 0.15),
      'actual native player moved to ' + state);
    await expectRequests(before, request ? [request] : []);
    await capture(label);
  }
  try {
    await wait(() => bar()?.dataset.state === 'paused' && bar().dataset.trackId === '2',
      'native libmpv restored synthetic track paused');
    const account = await invoke('account_snapshot');
    assert(account.status === 'authenticated', 'sealed synthetic session');
    click(one('[aria-label="打开设置"]'));
    await wait(() => playbackReady() && one('[name="startup-auto-play"]'), 'settings ready');
    const initial = await requests();
    assert(initial.every((request) => request === 'clear'), 'paused startup never acquires');
    if (phase === 'initial' || phase === 'reopen') {
      assert(initial.length >= 1, 'first observation explicitly clears');
      assert(option()?.closest('label').textContent.trim() === '音乐播放时不自动休眠或熄屏',
        'official checkbox label');
      const inputOrder = all('.playback-startup fieldset input').map((node) => node.name);
      // Official Play.jsx:376,407,443,453; the sealed fixture has no handoff control.
      const expectedOrder = ['startup-auto-play', 'playback-keep-awake',
        'scene-fm-autoplay', 'startup-remember-position'];
      assert(inputOrder.join() === expectedOrder.join(), 'official relative placement: ' +
        JSON.stringify({ actual: inputOrder, expected: expectedOrder }));
      assert(option()?.checked === (phase === 'reopen'), 'default or SQLite persistence');
    }
    if (phase === 'initial') {
      await capture('paused_default');
      await toggle(true, 'clear', 'enabled_while_paused');
      await transport('播放', 'playing', 'keep', 'playing_acquires');
      const beforeFailure = await requests();
      await fixture('reject-write');
      click(option());
      await wait(() => playbackReady() && error() && option().disabled, 'native storage write failed');
      assert(option().checked && !status(), 'failed write preserves confirmed checkbox');
      await wait(() => document.activeElement === refresh(), 'failure focuses refresh');
      await expectRequests(beforeFailure, []);
      await capture('write_failure_keeps_hold');
      await fixture('allow-write');
      click(refresh());
      await wait(() => playbackReady() && !error() && !option().disabled, 'explicit read recovers');
      await toggle(false, 'clear', 'disabled_while_playing');
      await toggle(true, 'keep', 'enabled_while_playing');
      await transport('暂停', 'paused', 'clear', 'pause_releases');
      await transport('播放', 'playing', 'keep', 'second_resume_acquires');
      await transport('停止播放', 'idle', 'clear', 'stop_releases');
    } else if (phase === 'reopen') {
      await capture('reopened_checked_without_hold');
      await transport('播放', 'playing', 'keep', 'reopened_playing_acquires');
      await transport('停止播放', 'idle', 'clear', 'reopened_stop_releases');
    } else {
      assert(!option(), 'control group or unsupported host must hide the setting');
      const enabled = phase === 'control';
      const read = await invoke('playback_power', { epoch: account.epoch });
      const refused = await invoke('set_playback_power', { epoch: account.epoch, enabled: false });
      assert(!read.available && !refused.available && read.enabled === enabled && refused.enabled === enabled,
        'hidden capability and attempted write independently fenced by native code');
      await capture('hidden');
      await transport('播放', 'playing', enabled ? 'keep' : null, 'hidden_playing');
      await transport('停止播放', 'idle', enabled ? 'clear' : null, 'hidden_stop');
    }
    await emit('complete', { ok: true, phase, stageCount, realMusicRequests: 0, realAccountWrites: 0, realOsPowerCalls: 0 });
  } catch (cause) {
    await emit('failed', {
      ok: false, phase, last, stageCount, error: String(cause), state: bar()?.dataset.state,
      position: bar()?.dataset.position, powerStatus: status(), powerError: error(),
    });
  }
})();
