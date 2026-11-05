// Actual Vue and native IPC; only the final default-endpoint response is synthetic.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  function assert(value, message) { if (!value) throw Error(message); }
  async function wait(test, message) {
    for (let step = 0; step < 150; step++) {
      const value = await test();
      if (value) return value;
      await sleep(75);
    }
    throw Error(message);
  }
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, extra = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...extra },
  });
  const bar = () => one('.playerbar');
  const diagnosis = () => one('[data-system-master-volume]');
  const volume = () => one('[data-system-master-percent]')?.textContent.trim() ?? null;
  const ready = () => one('.audio-output')?.getAttribute('aria-busy') === 'false';
  const refresh = () => one('[aria-label="刷新音频输出设备"]');
  const daily = () => all('.nav-item').find((node) => node.textContent.trim() === '每日推荐');
  let count = 0, playId, pausedAt, last = 'startup';
  const enabled = (node) => node?.isConnected && !node.matches(':disabled') &&
    !node.closest('[inert]') && node.getClientRects().length > 0;
  const playAll = () => one('.track-actions [data-recommendation-play]');
  const describe = (node) => node ? { connected: node.isConnected,
    disabled: node.matches(':disabled'), visible: node.getClientRects().length > 0,
    label: node.getAttribute('aria-label'), text: node.textContent.trim().slice(0, 100) } : null;
  function click(node, label) {
    assert(enabled(node), 'enabled control ' + label + ': ' + node?.outerHTML);
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    node.click();
  }
  async function activate(lookup, label) {
    // Resolve and activate in the same DOM turn. Waiting for a previous query
    // then querying again allowed a route update to remove that control.
    last = label;
    await wait(() => {
      const node = lookup();
      if (!enabled(node)) return false;
      click(node, label);
      return true;
    }, 'connected enabled control: ' + label);
  }
  async function openSettings() {
    await activate(() => one('[aria-label="打开设置"]'), 'open Settings');
    await wait(() => ready(), 'native settings ready');
  }
  async function capture(label) {
    last = label;
    one('.audio-output').scrollIntoView({ block:'center' });
    await sleep(300);
    assert(bar().dataset.playId === playId && bar().dataset.state === 'paused' &&
      Math.abs(Number(bar().dataset.position) - pausedAt) < 0.2, 'diagnostics never changed the player');
    assert(document.documentElement.scrollWidth <= innerWidth, 'no viewport overflow');
    const stage = 'ux_' + String(++count).padStart(2, '0') + '_' + label;
    window.__systemProbeStageRecorded = null;
    await emit(stage, {
      visible: !!diagnosis(), percent: volume(), playId, pausedAt,
      focused: document.activeElement?.getAttribute('aria-label'),
      text: diagnosis()?.textContent.trim() ?? '',
      width:innerWidth, theme:document.documentElement.dataset.theme,
    });
    await wait(() => window.__systemProbeStageRecorded === stage, 'native evidence saved for ' + stage);
  }
  async function readFixture(mode, expected) {
    window.__systemMasterVolumeFixtureAck = null;
    await emit('system-master-volume-fixture', { mode });
    await wait(() => window.__systemMasterVolumeFixtureAck === mode, 'synthetic native endpoint swapped');
    await activate(refresh, 'refresh endpoint: ' + mode);
    await wait(() => ready() && volume() === expected, 'fresh native result: ' + mode);
    await wait(() => document.activeElement === refresh(), 'refresh focus retained');
  }
  try {
    const account = await wait(async () => {
      const current = await invoke('account_snapshot');
      return current.status === 'authenticated' && enabled(daily()) &&
        enabled(one('[aria-label="打开设置"]')) && current;
    }, 'ready after synthetic account recovery');
    await activate(daily, 'open synthetic Daily');
    last = 'wait for current Daily receipt';
    await wait(() => one('.track-row[data-track-id="1"]') &&
      playAll()?.getAttribute('data-recommendation-play') && enabled(playAll()),
      'synthetic Daily rows and authoritative play receipt');
    await activate(playAll, 'play confirmed Daily receipt');
    last = 'wait for native Daily playback';
    const playing = await wait(async () => {
      const current = await invoke('player_snapshot', { epoch: account.epoch });
      return current.state === 'playing' && current.track?.id === '1' && current.position > .2 &&
        bar()?.dataset.playId === current.playId && bar().dataset.state === 'playing' && current;
    }, 'actual libmpv playing the first authorized synthetic song');
    await activate(() => bar()?.querySelector('[aria-label="暂停"]'), 'pause native Daily playback');
    await wait(async () => {
      const current = await invoke('player_snapshot', { epoch: account.epoch });
      return current.state === 'paused' && current.playId === playing.playId &&
        bar()?.dataset.playId === current.playId && bar().dataset.state === 'paused';
    }, 'actual native pause');
    playId = playing.playId;
    pausedAt = Number(bar().dataset.position);
    await openSettings();
    if (window.__systemMasterVolumeUnsupported) {
      assert(!diagnosis(), 'normal Linux command hides the Windows diagnostic');
      assert((await invoke('system_master_volume')).kind === 'unsupported', 'native unsupported result');
      await capture('unsupported');
    } else {
      assert(volume() === '37%' && diagnosis().textContent.includes('低于30%可能影响收听体验'),
        'official threshold hint and normalized scalar');
      assert(one('.audio-output select').compareDocumentPosition(diagnosis()) & Node.DOCUMENT_POSITION_FOLLOWING,
        'diagnosis follows the output selection');
      await capture('initial');
      await readFixture('muted', '0%');
      await capture('muted');
      await readFixture('missing', null);
      assert(diagnosis().textContent.includes('暂时无法读取系统音量'), 'failure is not reported as zero');
      await capture('unavailable');
      await activate(() => one('input[name="settings-theme"][value="dark"]'), 'select dark theme');
      await wait(() => document.documentElement.dataset.theme === 'dark', 'dark theme');
      await emit('resize', {width:760, height:540});
      await sleep(150);
      if (innerWidth !== 760 || innerHeight !== 540) {
        // Compare CSS viewport sizes even if the desktop is running at a
        // different window scaling. The production window still owns resize.
        await emit('resize', {width:760 * 760 / innerWidth, height:540 * 540 / innerHeight});
      }
      await wait(() => innerWidth === 760, 'compact settings viewport');
      await readFixture('recovered', '84%');
      await capture('recovered');
      await activate(daily, 'leave Settings for Daily');
      await wait(() => !one('.settings-page'), 'left settings');
      await openSettings();
      assert(volume() === '84%', 'remount performs a fresh native read');
      await capture('reopened');
    }
    await emit('complete', { ok:true, stageCount:count, realOsWrites:0, realMusicRequests:0, realAccountWrites:0 });
  } catch (cause) {
    await emit('failed', { ok:false, last, count, error:String(cause), stack:cause instanceof Error ? cause.stack : null,
      controls: { daily:describe(daily()), settings:describe(one('[aria-label="打开设置"]')),
        playAll:describe(playAll()), pause:describe(bar()?.querySelector('[aria-label="暂停"]')) },
      percent:volume(), state:bar()?.dataset.state });
  }
})();
