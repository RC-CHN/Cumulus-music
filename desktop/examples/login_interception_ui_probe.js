(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const el = (s) => document.querySelector(s);
  const report = (stage, data = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', { event: 'auth-proof', payload: { stage, ...data } });
  const wait = async (fn, name) => { const end = Date.now() + 30000; while (Date.now() < end) { if (fn()) return; await sleep(100); } throw Error(name); };
  async function stage(name) { await sleep(400); window.__authAck = ''; await report(name, { text: el('.welcome')?.textContent || '', signedIn: el('.account-button')?.textContent.includes('邮箱验证账号') }); await wait(() => window.__authAck === name, 'native_ack'); await sleep(900); }
  async function search(keyword) { const n = el('.searchbox input'); n.value = keyword; n.dispatchEvent(new Event('input', { bubbles: true })); el('.searchbox').dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })); }
  try {
    await wait(() => el('.account-button')?.textContent.includes('邮箱验证账号') && !el('.loading-state'), 'restored');
    await stage('interception_restored');
    await search('fixture-first');
    await wait(() => document.body.textContent.includes('请求未通过登录校验'), 'retained');
    if (!el('.account-button')?.textContent.includes('邮箱验证账号')) throw Error('account_was_lost');
    await stage('interception_retained');
    await sleep(5200);
    await search('fixture-second');
    await wait(() => el('.welcome')?.textContent.includes('请重新确认登录'), 'reauth_notice');
    await stage('interception_expired');
    const button = [...document.querySelectorAll('.welcome button')].find((n) => n.textContent.trim() === '重新登录');
    button.click(); await wait(() => el('.login-methods'), 'login_entry');
    await stage('interception_login');
    await report('complete', { ok: true });
  } catch (e) { await report('failed', { ok: false, error: e instanceof Error ? e.message : 'unknown' }); }
})();
