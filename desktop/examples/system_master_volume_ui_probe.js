// Actual Vue and native IPC; only the final default-endpoint response is synthetic.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  function assert(value, message) { if (!value) throw Error(message); }
  async function wait(test, message) {
    for (let step = 0; step < 150; step++) {
      if (test()) return;
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
  function click(node) {
    assert(node && !node.matches(':disabled'), 'enabled control: ' + node?.outerHTML);
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    node.click();
  }
  async function openSettings() {
    click(one('[aria-label="打开设置"]'));
    await wait(() => ready(), 'native settings ready');
  }
  async function capture(label) {
    last = label;
    one('.audio-output').scrollIntoView({ block:'center' });
    await sleep(300);
    assert(bar().dataset.playId === playId && bar().dataset.state === 'paused' &&
      Math.abs(Number(bar().dataset.position) - pausedAt) < 0.2, 'diagnostics never changed the player');
    assert(document.documentElement.scrollWidth <= innerWidth, 'no viewport overflow');
    await emit('ux_' + String(++count).padStart(2, '0') + '_' + label, {
      visible: !!diagnosis(), percent: volume(), playId, pausedAt,
      focused: document.activeElement?.getAttribute('aria-label'),
      text: diagnosis()?.textContent.trim() ?? '',
      width:innerWidth, theme:document.documentElement.dataset.theme,
    });
  }
  async function readFixture(mode, expected) {
    window.__systemMasterVolumeFixtureAck = null;
    await emit('system-master-volume-fixture', { mode });
    await wait(() => window.__systemMasterVolumeFixtureAck === mode, 'synthetic native endpoint swapped');
    click(refresh());
    await wait(() => ready() && volume() === expected, 'fresh native result: ' + mode);
    await wait(() => document.activeElement === refresh(), 'refresh focus retained');
  }
  try {
    await wait(() => daily() && one('[aria-label="打开设置"]') && !one('[aria-label="打开设置"]').disabled,
      'ready after synthetic account recovery');
    click(daily());
    await wait(() => all('button').some((node) => node.textContent.trim() === '播放全部' && !node.disabled),
      'synthetic Daily tracks');
    click(all('button').find((node) => node.textContent.trim() === '播放全部' && !node.disabled));
    await wait(() => bar().dataset.state === 'playing' && Number(bar().dataset.position) > 0.2,
      'actual libmpv playing');
    click(bar().querySelector('[aria-label="暂停"]'));
    await wait(() => bar().dataset.state === 'paused', 'actual native pause');
    playId = bar().dataset.playId;
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
      click(one('input[name="settings-theme"][value="dark"]'));
      await wait(() => document.documentElement.dataset.theme === 'dark', 'dark theme');
      await emit('resize', {width:760, height:540});
      await wait(() => innerWidth === 760, 'compact settings viewport');
      await readFixture('recovered', '84%');
      await capture('recovered');
      click(daily());
      await wait(() => !one('.settings-page'), 'left settings');
      await openSettings();
      assert(volume() === '84%', 'remount performs a fresh native read');
      await capture('reopened');
    }
    await emit('complete', { ok:true, stageCount:count, realOsWrites:0, realMusicRequests:0, realAccountWrites:0 });
  } catch (cause) {
    await emit('failed', { ok:false, last, count, error:String(cause), percent:volume(), state:bar()?.dataset.state });
  }
})();
