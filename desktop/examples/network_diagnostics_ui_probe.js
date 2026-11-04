(async () => {
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => Array.from(document.querySelectorAll(selector));
  const assert = (condition, message) => { if (!condition) throw Error(message); };
  const emit = (stage, data = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'diagnostics-proof', payload: { stage, ...data },
  });
  async function wait(condition, message) {
    for (let i = 0; i < 120; ++i) {
      if (condition()) return;
      await sleep(90);
    }
    throw Error(message);
  }
  async function capture(stage) {
    await sleep(350);
    await emit(stage, { width: innerWidth, height: innerHeight });
    await sleep(700);
  }
  function click(element) {
    assert(element && !element.disabled, 'control not available');
    element.focus({ preventScroll: true });
    element.click();
  }
  try {
    const gear = () => one('[aria-label="打开设置"]');
    await wait(() => gear() && !gear().disabled, 'settings is available');
    click(gear());
    const about = () => one('[aria-labelledby="settings-about"]');
    await wait(() => about()?.textContent.includes('当前版本'), 'About and version render');
    assert(one('.settings-page')?.textContent.includes('尚未登录网易云音乐'), 'proof must remain signed out');
    const opener = Array.from(about().querySelectorAll('button'))
      .find((button) => button.textContent.trim() === '网络诊断');
    assert(opener, 'explicit network diagnostics entry');
    opener.scrollIntoView({ block: 'end' });
    assert(!one('.settings-network-diagnostics'), 'diagnostics are not opened automatically');
    await capture('settings');
    click(opener);
    const dialog = () => one('.settings-network-diagnostics[open]');
    const rows = () => all('.network-diagnostics-results dd').map((node) => node.textContent.trim());
    await wait(() => dialog() && rows().length === 5, 'local diagnostics results');
    assert(rows()[0] === '可用' && rows()[4] === '未检查', 'loopback available and external service not contacted');
    assert(dialog().textContent.includes('不上传结果'), 'scope stated in the dialog');
    assert(!dialog().textContent.includes('127.0.0.1'), 'no raw local endpoint');
    await capture('diagnostics');
    click(all('.settings-network-diagnostics footer button').find((button) => button.textContent.trim() === '重新诊断'));
    await wait(() => rows().length === 5, 'explicit native rerun');
    dialog().dispatchEvent(new Event('cancel', { cancelable: true }));
    await wait(() => !dialog() && document.activeElement === opener, 'Escape and focus restoration');
    await sleep(200);
    assert(!dialog(), 'no automatic reopen after close');
    await emit('complete', { ok: true, signedOut: true });
  } catch (error) {
    await emit('failed', { ok: false, error: String(error), active: document.activeElement?.outerHTML.slice(0, 300) });
  }
})();
