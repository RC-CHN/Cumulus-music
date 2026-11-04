(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const all = (selector, root = document) => [...root.querySelectorAll(selector)];
  const ready = (node) => !!(node && !node.disabled && node.getClientRects().length &&
    !node.closest('[inert]') && !node.closest('.page-leave-active'));
  const byText = (text, root = document) =>
    all('button', root).find((node) => ready(node) && node.textContent.trim() === text);
  const assert = (valid, label) => { if (!valid) throw Error(label); };
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  async function wait(test, label) {
    for (let i = 0; i < 160; i++) {
      if (test()) return;
      await sleep(60);
    }
    throw Error(label);
  }
  function click(node) {
    assert(ready(node), 'available control');
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    node.click();
  }
  const emit = (stage, fields = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...fields },
  });
  const list = () => one('[data-user-blacklist]');
  const target = () => one('[data-blacklist-user="73"]', list());
  const remove = () => one('[data-blacklist-remove]', target());
  const confirm = () => one('dialog.profile-blacklist-confirm[open]');
  const unblock = () => one('[data-profile-unblock]');
  const menu = () => one('[data-profile-blacklist-menu]');
  let stageCount = 0;
  async function stage(name) {
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await sleep(100);
    return emit(`ux_${String(++stageCount).padStart(2, '0')}_${name}`, {
      shown: list() ? all('[data-blacklist-user]', list()).length : null,
      visible: document.visibilityState === 'visible',
      target: !!target(),
      confirm: !!confirm(),
      pending: !!one('.blacklist-pending', list()),
    });
  }
  try {
    await wait(() => ready(one('[aria-label="打开设置"]')), 'app bootstrap');
    click(one('[aria-label="打开设置"]'));
    await wait(() => ready(one('[data-open-user-blacklist]')), 'privacy blacklist entry');
    one('[data-open-user-blacklist]').scrollIntoView({ block: 'center' });
    await stage('privacy_entry');
    click(one('[data-open-user-blacklist]'));
    await wait(() => !!one('[role="alert"]', list()), 'initial list failure');
    await stage('first_error');
    click(byText('重试', list()));
    await wait(target, 'account list loaded');
    assert(all('[data-blacklist-user]', list()).length < 46, 'blacklist grid mounts bounded rows');
    assert(one('[data-blacklist-user="100"] .blacklist-profile').disabled, 'closed account cannot open');
    await stage('list');

    click(one('.blacklist-profile', target()));
    await wait(() => ready(unblock()), 'direct unblocking on profile');
    assert(!one('[data-profile-follow]'), 'own blacklist suppresses follow action');
    await stage('blocked_profile');
    click(unblock());
    await wait(() => ready(menu()) && !unblock(), 'profile unblocked without a second dialog');
    await stage('unblocked_profile');
    const opener = menu();
    opener.focus();
    opener.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    await wait(() => document.activeElement === one('[role="menuitem"]') && !!one('[role="menuitem"]'),
      'keyboard opens the profile more menu and focuses the action');
    await stage('profile_menu');
    click(one('[role="menuitem"]'));
    await wait(() => ready(unblock()) && !one('[role="menuitem"]'), 'adding applies official blocked profile branch');
    await stage('added');

    click(one('[aria-label="返回上一页"]'));
    await wait(() => ready(remove()), 'list back reads latest membership');
    click(remove());
    await wait(() => confirm() && document.activeElement === byText('取消', confirm()), 'list removal defaults to cancel');
    const affirmative = byText('确定', confirm());
    const bounds = affirmative.getBoundingClientRect();
    assert(affirmative.contains(document.elementFromPoint(bounds.x + bounds.width / 2,
      bounds.y + bounds.height / 2)), 'confirmation is not covered');
    await stage('confirm');
    click(byText('取消', confirm()));
    await wait(() => !confirm() && document.activeElement === remove(), 'cancel restores row focus');
    await stage('cancelled');
    click(remove());
    await wait(confirm, 'second explicit removal confirmation');
    click(byText('确定', confirm()));
    await wait(() => one('.blacklist-pending', list()) && target(), 'unknown delete preserves list and exposes reconciliation');
    assert(remove().disabled, 'unknown delete cannot be replayed');
    await stage('pending');
    click(byText('核对黑名单', list()));
    await wait(() => !target() && !one('.blacklist-pending', list()) &&
      list().textContent.includes('已解除黑名单'), 'list-only reconciliation settles');
    await stage('reconciled');

    click(one('[aria-label="返回设置"]', list()));
    await wait(() => ready(one('[data-open-user-blacklist]')) && !list() &&
      document.activeElement === one('[data-open-user-blacklist]'), 'return focus to privacy settings');
    await stage('settings_back');
    await emit('complete', { ok: true, stageCount, realMusicRequests: 0, remoteWrites: 0 });
  } catch (error) {
    await emit('failed', { ok: false, error: String(error), stageCount,
      text: (list() || one('.main-content'))?.innerText.slice(0, 1000) });
  }
})();
