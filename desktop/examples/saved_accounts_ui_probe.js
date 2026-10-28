// Production UI + native account removal, isolated synthetic credentials only.
(async () => {
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const el = selector => document.querySelector(selector);
  const all = selector => [...document.querySelectorAll(selector)];
  const assert = (ok, message) => { if (!ok) throw Error(message); };
  const report = (stage, data = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', { event: 'saved-account-proof', payload: { stage, ...data } });
  const wait = async (fn, label) => { const end = Date.now() + 30000; while (Date.now() < end) { if (fn()) return; await sleep(100); } throw Error(label); };
  const click = async node => {
    assert(node, 'missing control');
    await wait(() => node.isConnected && !node.disabled, 'disabled control');
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true }); node.click(); await sleep(150);
  };
  const text = value => all('button').find(node => node.textContent.trim() === value);
  const rows = () => all('.saved-account');
  const remove = () => el('[aria-label="移除账号 邮箱验证账号"]');
  const focus = node => assert(document.activeElement === node, 'confirmation focus lost');
  const title = value => {
    assert(el('.account-dialog h2').textContent === value, 'wrong step title');
    assert(el('.account-dialog').getAttribute('aria-labelledby') === el('.account-dialog h2').id, 'dialog label must describe current step');
  };
  async function stage(name) {
    await sleep(400);
    const dialog = el('dialog'), rect = dialog?.getBoundingClientRect();
    assert(document.documentElement.scrollWidth <= innerWidth && (!dialog || (dialog.scrollWidth <= dialog.clientWidth + 1 && rect.top >= 0 && rect.bottom <= innerHeight + 1)), 'layout overflow');
    if (dialog) for (const node of dialog.querySelectorAll('.account-confirm button')) {
      const box = node.getBoundingClientRect();
      assert(box.top >= rect.top && box.bottom <= rect.bottom && box.left >= rect.left && box.right <= rect.right, 'confirmation action clipped');
    }
    window.__savedAck = '';
    await report(name, { overflow: false, width: innerWidth, height: innerHeight, theme: document.documentElement.dataset.theme, focused: document.activeElement?.textContent.trim(), title: dialog?.querySelector('h2')?.textContent });
    await wait(() => window.__savedAck === name, 'native acknowledgement'); await sleep(900);
  }
  async function theme(value) {
    await click(el('[aria-label="外观主题"]')); await wait(() => el('.action-menu'), 'appearance menu');
    await click(text(value === 'dark' ? '深色' : '浅色'));
    await wait(() => !el('.action-menu') && document.documentElement.dataset.theme === value, 'theme');
  }
  async function open() {
    await click(el('.account-button'));
    await wait(() => el('.account-add') && !el('.account-dialog [role="status"]'), 'accounts');
  }
  async function logout(name, cancelEvent = false) {
    await click(el('.account-logout')); await wait(() => text('确认退出'), 'logout confirmation');
    await stage(name); focus(text('取消')); title('退出登录');
    if (cancelEvent) { el('.account-dialog').dispatchEvent(new Event('cancel', { cancelable: true })); await sleep(150); }
    else await click(text('取消'));
    focus(el('.account-logout')); title('切换账号');
    assert(rows().length === 2, 'cancel logout changed saved accounts');
  }
  try {
    await wait(() => el('.account-button')?.textContent.includes('二次验证账号') && !el('.account-button').disabled, 'current account');
    if (window.__savedReopen) {
      assert(document.documentElement.dataset.theme === 'dark', 'theme did not persist in the isolated WebView');
      await stage('saved_reopen_main'); await open();
      assert(rows().length === 1 && !all('.saved-remove').length, 'removed account returned');
      await stage('saved_reopen_accounts');
    } else {
      await theme('light'); await open();
      assert(rows().length === 2 && all('.saved-remove').length === 1, 'current account removal exposed');
      await stage('saved_list');
      await logout('saved_logout_light');
      await click(remove()); focus(text('取消')); title('确认移除账号'); await click(text('取消'));
      assert(rows().length === 2, 'cancel removed account'); focus(remove()); await stage('saved_cancelled');
      await click(el('[aria-label="关闭账号窗口"]')); await wait(() => !el('.account-dialog'), 'close accounts');
      focus(el('.account-button')); await theme('dark'); await report('resize');
      await wait(() => innerWidth === 760, 'small window'); await open(); await stage('saved_dark_small');
      await logout('saved_logout_dark', true);
      await click(remove()); focus(text('取消')); title('确认移除账号');
      await stage('saved_confirm'); await click(text('确认移除'));
      await wait(() => rows().some(row => row.textContent.includes('凭据清理未完成')), 'pending cleanup');
      assert(rows().find(row => row.textContent.includes('邮箱验证账号')).disabled, 'pending account switchable');
      focus(remove()); await stage('saved_pending');
      await click(text('继续移除')); focus(text('取消')); await click(text('确认移除'));
      await wait(() => rows().length === 1 && !el('.account-dialog [role="alert"]'), 'cleanup retry');
      focus(el('[aria-label="关闭账号窗口"]')); await stage('saved_done');
    }
    await report('complete', { ok: true, reopened: window.__savedReopen, confirmationFocus: true, explicitCancel: true, dialogCancel: true });
  } catch (error) { await report('failed', { ok: false, error: String(error), focused: document.activeElement?.outerHTML.slice(0, 400) }); }
})();
