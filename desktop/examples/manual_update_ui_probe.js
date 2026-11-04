(async () => {
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const one = (selector) => document.querySelector(selector);
  const dialog = () => one('.settings-update-check[open]');
  const title = () => dialog()?.querySelector('h2')?.textContent;
  const action = (label) => Array.from(dialog()?.querySelectorAll('button') || [])
    .find((node) => node.textContent.trim() === label);
  const assert = (condition, message) => { if (!condition) throw Error(message); };
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, details = {}) => invoke('plugin:event|emit', {
    event: 'update-proof', payload: { stage, ...details },
  });
  async function wait(predicate, message) {
    for (let i = 0; i < 100; ++i) {
      if (predicate()) return;
      await sleep(90);
    }
    throw Error(message);
  }
  async function capture(stage) {
    await sleep(330);
    await emit(stage, { width: innerWidth, height: innerHeight });
    await sleep(700);
  }
  function click(node) {
    assert(node && !node.disabled, 'control not available');
    node.focus({ preventScroll: true });
    node.click();
  }
  async function deniedWithoutOffer() {
    try {
      await invoke('open_update_release_page', { url: 'https://private.invalid/untrusted' });
    } catch (error) {
      assert(error === 'no_offer', 'open is denied without a verified offer');
      return;
    }
    throw Error('open without verified offer unexpectedly succeeded');
  }
  try {
    const gear = () => one('[aria-label="打开设置"]');
    await wait(() => gear() && !gear().disabled, 'settings available');
    click(gear());
    const about = () => one('[aria-labelledby="settings-about"]');
    await wait(() => about()?.textContent.includes('当前版本'), 'About shows current version');
    const opener = Array.from(about().querySelectorAll('button'))
      .find((node) => node.textContent.trim() === '检查更新');
    assert(opener && !dialog(), 'manual entry without auto-open');
    opener.scrollIntoView({ block: 'end' });
    await deniedWithoutOffer();
    await capture('settings');

    click(opener);
    await wait(() => dialog()?.textContent.includes('最新公开稳定版'), 'current stable release');
    assert(!action('查看发布页'), 'no handoff for current release');
    await capture('current');
    click(action('知道了'));
    await wait(() => !dialog() && document.activeElement === opener, 'current acknowledgement returns focus');

    await emit('mode', { mode: 2 });
    click(opener);
    await wait(() => dialog()?.textContent.includes('无法连接 GitHub'), 'network error');
    await capture('network_error');
    await emit('mode', { mode: 1 });
    click(action('重试'));
    await wait(() => title() === '发现新版本' && action('查看发布页'), 'explicit retry offers new release');
    assert(dialog().textContent.includes('999.1.0') && !dialog().textContent.includes('PRIVATE'),
      'new version only, no raw metadata');
    await capture('available');
    await emit('browser', { fail: true });
    click(action('查看发布页'));
    await wait(() => dialog()?.textContent.includes('无法打开系统浏览器'), 'native browser failure allows retry');
    await emit('browser', { fail: false });
    click(action('查看发布页'));
    await wait(() => !dialog() && document.activeElement === opener, 'successful browser handoff returns focus');
    await deniedWithoutOffer();

    await emit('mode', { mode: 4 });
    click(opener);
    await wait(() => dialog()?.textContent.includes('暂未返回'), 'no published release is distinct');
    await capture('absent');
    click(action('知道了'));
    await wait(() => !dialog(), 'absent dismissal');

    await emit('mode', { mode: 5 });
    click(opener);
    await wait(() => dialog()?.textContent.includes('发布信息不完整'), 'incomplete package metadata');
    assert(!action('查看发布页'), 'no handoff for malformed release');
    click(action('知道了'));
    await wait(() => !dialog(), 'malformed dismissal');

    await emit('mode', { mode: 6 });
    click(opener);
    await wait(() => action('关闭'), 'pending check can be dismissed');
    click(action('关闭'));
    await wait(() => !dialog() && document.activeElement === opener, 'pending check dismissal returns focus');
    await sleep(1000);
    assert(!dialog(), 'late reply does not reopen dialog');
    await emit('complete', { ok: true, realNetwork: false });
  } catch (error) {
    await emit('failed', { ok: false, error: String(error), active: document.activeElement?.outerHTML.slice(0, 250) });
  }
})();
