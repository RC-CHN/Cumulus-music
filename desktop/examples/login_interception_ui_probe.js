(async () => {
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const el = (selector) => document.querySelector(selector);
  const legacy = window.__interceptionLegacy === true;
  const guide = () => el('[data-classic-remove-guide]');
  const report = (stage, data = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'auth-proof', payload: { stage, ...data },
  });
  const wait = async (check, name) => {
    const end = Date.now() + 30000;
    while (Date.now() < end) {
      const result = await check();
      if (result) return result;
      await sleep(50);
    }
    throw Error(name);
  };
  const assert = (check, name) => { if (!check) throw Error(name); };
  const account = () => window.__TAURI_INTERNALS__.invoke('account_snapshot');
  async function stage(name, settle = 900) {
    await sleep(name === 'interception_expired' && legacy ? 0 : 350);
    window.__authAck = '';
    await report(name, {
      text: el('.welcome')?.textContent || '',
      signedIn: el('.account-button')?.textContent.includes('邮箱验证账号'),
      guide: guide()?.open ?? false,
    });
    await wait(() => window.__authAck === name, 'native_ack');
    if (settle) await sleep(settle);
  }
  function search(keyword) {
    const input = el('.searchbox input');
    input.value = keyword;
    input.dispatchEvent(new Event('input', { bubbles: true }));
    el('.searchbox').dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
  }
  try {
    await wait(() => el('.account-button')?.textContent.includes('邮箱验证账号') && !el('.loading-state'), 'restored');
    await stage('interception_restored');
    search('fixture-first');
    await wait(() => document.body.textContent.includes('请求未通过登录校验'), 'retained');
    assert(el('.account-button')?.textContent.includes('邮箱验证账号'), 'account_was_lost');
    assert(!guide(), 'action one must not open the recovery guide');
    await stage('interception_retained');
    await sleep(5200);
    search('fixture-second');
    await wait(() => el('.welcome')?.textContent.includes('请重新确认登录'), 'reauth_notice');
    if (legacy) assert(!guide(), 'classic guide must not appear with the initial invalidation');
    await stage('interception_expired', legacy ? 220 : 900);
    if (!legacy) {
      assert(!guide(), 'new benefit cohort must not open the classic guide');
      const button = [...document.querySelectorAll('.welcome button')]
        .find((node) => node.textContent.trim() === '重新登录');
      button.click();
      await wait(() => el('.login-methods'), 'login_entry');
      await stage('interception_login');
      await report('complete', { ok: true });
      return;
    }

    await wait(() => guide()?.open && guide().querySelector('.qr-frame svg'), 'automatic classic QR');
    assert(guide().querySelector('h2')?.textContent.trim() === '请重新确认登录', 'official title missing');
    assert(guide().querySelector('.classic-remove-guide-reason')?.textContent.trim() ===
      '登录状态已失效，请重新登录', 'official reason missing');
    assert(!guide().querySelector('.login-methods,.login-accounts'), 'classic guide has no unrelated methods');
    const epoch = (await account()).epoch;
    await stage('interception_login', 220);
    const retry = await wait(() => [...guide().querySelectorAll('button')].find(
      (button) => button.textContent.trim() === '点击重试' && !button.disabled,
    ), 'explicit retry after expired classic ticket');
    retry.click();
    await wait(() => guide()?.open && guide().querySelector('.qr-frame svg') &&
      !guide().textContent.includes('二维码已失效'), 'single new synthetic QR after retry');
    await stage('interception_guide_retry', 220);
    const beforeClose = await window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
      event: 'auth-proof', payload: { stage: 'interception_before_close' },
    });
    void beforeClose;
    await wait(() => window.__authAck === 'interception_before_close', 'close checkpoint');
    guide().querySelector('[aria-label="关闭登录"]').click();
    await wait(() => !guide(), 'classic guide closed');
    const snapshot = await account();
    assert(snapshot.epoch === epoch && snapshot.status === 'expired' && !snapshot.classicRemoveGuide,
      'close neither changes account nor leaves the automatic offer reusable');
    await sleep(1250);
    await stage('interception_guide_closed', 220);
    await report('complete', { ok: true, pollsBeforeClose: window.__classicPollsBeforeClose });
  } catch (error) {
    await report('failed', { ok: false, error: error instanceof Error ? error.message : 'unknown' });
  }
})();
