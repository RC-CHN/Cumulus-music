// Production Vue in the synthetic ux_ui_probe host. Only QR/captcha results are injected.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const assert = (value, message) => { if (!value) throw Error(message); };
  const wait = async (check, label) => {
    for (let i = 0; i < 150; i++) {
      if (check()) return;
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
    throw Error(label);
  };
  const emit = (stage, data = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  try {
    const account = () => one('.account-button');
    await wait(() => account() && !account().disabled && one('#app').__vue_app__, 'synthetic account ready');
    const provides = one('#app').__vue_app__._context.provides;
    const core = provides[Reflect.ownKeys(provides).find((key) => key.description === 'core-client')];
    let nextPoll = 'challenge', opens = 0, reply;
    core.beginQr = async () => ({
      operationId: 'synthetic-qr', status: 'waiting_scan', modules: [[true]], pollAfterMs: 100,
    });
    core.pollQr = async () => nextPoll;
    core.cancelQr = async () => {};
    core.qrIdentityPending = async () => false;
    core.solveLoginCaptcha = () => {
      opens++;
      return new Promise((resolve, reject) => { reply = { resolve, reject }; });
    };
    account().click();
    if (account().title !== '点击登录') {
      await wait(() => one('.account-add') && !one('.account-add').disabled, 'synthetic add account');
      one('.account-add').click();
    }
    const frame = () => one('.qr-frame');
    const action = () => [...document.querySelectorAll('button')].find((button) =>
      /^(打开安全验证|请在验证窗口中完成操作…)$/.test(button.textContent.trim()));
    async function start(expectedOpens) {
      await wait(() => action(), 'QR challenge action');
      action().focus();
      action().click();
      action().click();
      assert(opens === expectedOpens, 'a pending challenge cannot open twice');
    }
    await start(1);
    reply.reject({ code: 'state', message: '合成验证取消' });
    await wait(() => one('.login-status')?.textContent.includes('合成验证取消'), 'browser cancellation visible');
    assert(document.activeElement === action(), 'cancelling keeps focus on the retry action');
    await start(2);
    nextPoll = 'waiting_scan';
    reply.resolve({ kind: 'qr', value: 'waiting_scan' });
    await wait(() => !action() && frame()?.querySelector('svg'), 'successful verification returns to QR');
    assert(document.activeElement === frame(), 'successful verification moves owned focus to QR');
    nextPoll = 'challenge';
    await start(3);
    const other = one('.login-methods button:not(:disabled)');
    other.focus();
    nextPoll = 'waiting_confirmation';
    reply.resolve({ kind: 'qr', value: 'waiting_confirmation' });
    await wait(() => one('.qr-confirmation') && !action(), 'scan confirmation after verification');
    assert(document.activeElement === other, 'late verification does not steal focus from another control');
    await emit('complete', { ok: true, qrCaptchaFocusVerified: true, syntheticQrAndCaptcha: true, opens });
  } catch (error) {
    await emit('failed', { message: String(error) });
  }
})();
