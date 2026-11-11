// Native WebView with closed native account/assignment/list fixtures.
(async () => {
  const mode = window.__memberWelfareMode;
  window.__welfareAck = {};
  let id = 0;
  const one = (selector, root = document) => root.querySelector(selector);
  const account = () => one('.topbar .account-button');
  const menu = () => one('[role=menu][aria-label="账号菜单"]:not([inert])');
  const entry = () => menu()?.querySelector('[data-menu-key="welfare"]');
  const dialog = () => one('.member-welfare-dialog[open]:not([inert])');
  const current = () => one('dialog.account-dialog[open]:not([inert])');
  const login = () => one('dialog.login-dialog[open]:not([inert])');
  let accountDialogDismissed = false;
  const assert = (condition, reason) => { if (!condition) throw Error(reason); };
  const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const wait = async (predicate, reason) => {
    for (let attempt = 0; attempt < 100; attempt++) {
      const value = await predicate();
      if (value) return value;
      await delay(45);
    }
    throw Error(reason);
  };
  const send = (stage, details = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, mode, ...details },
  });
  const snap = async (stage) => {
    await delay(260);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await send(`ux_${stage}`);
  };
  async function fixture(action) {
    const request = ++id;
    await send('member-welfare-fixture', { action, id: request });
    return wait(() => window.__welfareAck[request], `fixture acknowledgement for ${action}`);
  }
  const trace = async () => (await fixture('status')).trace;
  const click = (node) => {
    assert(node && !node.disabled, 'missing or disabled action');
    node.focus({ preventScroll: true });
    node.click();
  };
  function escapeMenu() {
    document.activeElement.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
  }
  function closeDialog() {
    dialog().dispatchEvent(new Event('cancel', { cancelable: true }));
  }
  async function openMenu() {
    click(account());
    await wait(menu, 'assigned account menu opens');
  }
  async function closeMenu() {
    escapeMenu();
    await wait(() => !menu() && document.activeElement === account(), 'Escape restores account focus');
  }
  async function openList(prefix) {
    const action = await wait(entry, 'nonempty response reveals the welfare entry');
    assert(action.textContent.includes('查看合成活动') && action.textContent.includes('2 条未读'),
      'menu uses the server right-side tip and unread count');
    click(action);
    await wait(dialog, 'welfare dialog opens after menu selection');
    assert(!menu() && dialog().textContent.includes(`${prefix}活动`), 'selected snapshot and menu dismissed');
    assert(dialog().querySelectorAll('li').length === 2 &&
      dialog().querySelectorAll('.member-welfare-unread').length === 1, 'cards and unread indicator');
    assert(!dialog().querySelector('a, img, iframe, [href], [src]') &&
      dialog().querySelectorAll('button').length === 1 &&
      !dialog().innerHTML.includes('must-not-leak') &&
      !dialog().textContent.includes('立即领取'), 'no remote links, images, tickets or action affordances');
    assert(document.activeElement?.getAttribute('aria-label') === '关闭福利活动中心',
      'opening focuses close control');
  }
  try {
    await wait(() => account() && !account().disabled, 'account bootstrap settled');
    if (mode === 'guest' || mode === 'control') {
      if (mode === 'control') await wait(async () => (await trace()).length === 1, 'non-treatment experiment read');
      assert(!account().hasAttribute('aria-haspopup'), 'guest/control use their existing account action');
      const accountDialog = mode === 'guest' ? login : current;
      click(account());
      const opened = await wait(accountDialog, 'existing login or switch-account action');
      assert(!menu() && !entry() && !dialog(), 'no welfare exposure');
      await snap(mode);
      // Keep the open-dialog sample, then release the modal and its login
      // flow before asking the native host to shut down.
      opened.dispatchEvent(new Event('cancel', { cancelable: true }));
      await wait(() => !opened.isConnected && document.activeElement === account(),
        'dismissal removes the dialog and returns focus to the account button');
      accountDialogDismissed = true;
    } else {
      await wait(() => account()?.getAttribute('aria-haspopup') === 'menu', 'unique treatment assignment');
      assert((await trace()).length === 1 && !menu() && !dialog(), 'no welfare list prefetch before menu open');
      await openMenu();
      if (mode === 'reopen') {
        await openList('原账号');
        await snap('reopened');
        closeDialog();
        await wait(() => !dialog() && document.activeElement === account(), 'reopened dialog returns focus');
      } else {
        await wait(entry, 'welfare activity appears');
        assert(document.activeElement?.dataset.menuKey === 'member', 'first account menu action receives focus');
        await snap('assigned_menu');
        await openList('原账号');
        await snap('assigned_dialog');
        closeDialog();
        await wait(() => !dialog() && document.activeElement === account(), 'dialog Escape restores nickname focus');
        for (const [behavior, count] of [['empty', 3], ['failure', 4]]) {
          await fixture(behavior);
          await openMenu();
          await wait(async () => (await trace()).length === count, `native ${behavior} list read`);
          await snap(behavior);
          assert(!entry() && !dialog(), `${behavior} must not expose an old or empty list`);
          if (behavior === 'failure') {
            document.body.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }));
            await wait(() => !menu(), 'outside click dismisses the account menu');
          } else await closeMenu();
        }
        await fixture('hold');
        await openMenu();
        await wait(async () => (await trace()).length === 5, 'native old-owner list waits');
        await fixture('switch');
        await wait(() => account()?.textContent.includes('合成账号 3') && !account().disabled &&
          account().getAttribute('aria-haspopup') === 'menu' && !menu(), 'account change closes old menu');
        await openMenu();
        await openList('新账号');
        await fixture('release');
        await wait(async () => (await fixture('status')).oldReplyReturned, 'late old owner result returned');
        assert(dialog().textContent.includes('新账号活动') &&
          !document.body.textContent.includes('已离开账号的迟到'), 'late result cannot overwrite the new account');
        await snap('switched');
        click(one('[aria-label="关闭福利活动中心"]', dialog()));
        await wait(() => !dialog() && document.activeElement === account(), 'close button returns current focus');
      }
    }
    await send('complete', { ok: true, realMusicRequests: 0, remoteAccountWrites: 0,
      accountDialogDismissed });
  } catch (error) {
    await send('failed', { ok: false, error: String(error), stack: String(error.stack) });
  }
})();
