(async () => {
  const one = (s, root = document) => root.querySelector(s);
  const all = (s, root = document) => [...root.querySelectorAll(s)];
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (test, error) => { if (!test) throw Error(error); };
  const btn = (name) => all('button').find((node) => node.getAttribute('aria-label') === name);
  const emit = (stage, data = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  async function wait(test, label) {
    for (let i = 0; i < 165; i++) {
      const value = test();
      if (value) return value;
      await sleep(70);
    }
    throw Error(label);
  }
  function click(node) {
    assert(node && !node.disabled, 'missing enabled control');
    node.focus({ preventScroll: true });
    node.click();
  }
  let count = 0;
  async function capture(name) {
    await wait(() => !one('.page-view.page-enter-active, .page-view.page-leave-active'), 'route transition');
    await sleep(260);
    assert(document.documentElement.scrollWidth <= innerWidth, 'horizontal overflow');
    await emit(`ux_${String(++count).padStart(2, '0')}_${name}`, { width: innerWidth });
  }
  const shortcuts = () => one('[aria-label="我的主页快捷入口"]');
  const navigation = (name) => {
    const group = shortcuts();
    return group && all('button', group).find((node) => node.textContent.trim().endsWith(name));
  };
  async function accountMenu() {
    click(one('.account-button'));
    return wait(() => shortcuts() && one('dialog[open]'), 'nickname opens account controls');
  }
  try {
    const avatar = await wait(() => btn('打开我的主页') && !btn('打开我的主页').disabled &&
      one('.account-button').textContent.includes('合成本人资料') && btn('打开我的主页'), 'signed in');
    await emit('resize');
    await wait(() => innerWidth === 760, 'compact window');
    click(avatar);
    await wait(() => one('.playlist-creator-page h1')?.textContent.trim() === '合成本人资料' &&
      one('[data-profile-rank-entry]')?.textContent.includes('我的听歌排行'), 'avatar routes to own profile');
    assert(!one('dialog[open]'), 'avatar opens account dialog instead of home');
    await capture('own_profile');
    await accountMenu();
    assert(navigation('笔记').textContent.includes('3') && navigation('关注').textContent.includes('4') &&
      navigation('粉丝').textContent.includes('5'), 'verified shortcut counters');
    await capture('account_controls');
    click(navigation('笔记'));
    await wait(() => one('[aria-label="用户笔记"]')?.textContent.includes('合成本人笔记') &&
      !one('dialog[open]'), 'own notes shortcut');
    await capture('own_notes');
    await accountMenu();
    click(navigation('关注'));
    await wait(() => one('.profile-relations')?.textContent.includes('暂无关注') && !one('dialog[open]'),
      'own mixed following route');
    await capture('own_following');
    await accountMenu();
    click(navigation('粉丝'));
    await wait(() => one('.profile-relations')?.textContent.includes('暂无粉丝') && !one('dialog[open]'),
      'own follower route');
    await capture('own_followers');
    await emit('complete', { ok: true, stageCount: count, realMusicRequests: 0, remoteWrites: 0 });
  } catch (error) {
    await emit('failed', { message: String(error), stageCount: count });
  }
})();
