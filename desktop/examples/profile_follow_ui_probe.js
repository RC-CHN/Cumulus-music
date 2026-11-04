// Production guest follow/unfollow UI against a network-free synthetic service.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])];
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const button = (name, root = document) => all('button', root).find(
    (node) => (node.getAttribute('aria-label') || node.textContent.trim()) === name,
  );
  const follow = () => one('[data-profile-follow]');
  const followButton = (name) => button(name, follow() ?? one('.playlist-creator-page'));
  const dialog = () => one('.profile-unfollow-dialog[open]');
  const assert = (value, label) => { if (!value) throw Error(label); };
  const emit = (stage, payload = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...payload },
  });
  async function wait(test, label) {
    for (let i = 0; i < 170; i++) {
      const result = test();
      if (result) return result;
      await sleep(80);
    }
    throw Error(label);
  }
  function click(node) {
    assert(node && !node.disabled, 'missing or disabled action');
    node.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    node.focus({ preventScroll: true });
    node.click();
  }
  let stageCount = 0;
  async function capture(label) {
    await Promise.race([
      new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))),
      sleep(180),
    ]);
    await sleep(240);
    assert(document.documentElement.scrollWidth <= innerWidth, 'horizontal overflow');
    await emit(`ux_${String(++stageCount).padStart(2, '0')}_${label}`, {
      width: innerWidth, dialog: !!dialog(),
      active: document.activeElement?.textContent.trim(),
    });
  }
  async function openProfile(playlist, uid) {
    click(button('我的歌单'));
    click(await wait(() => button('打开歌单链接'), 'playlist library'));
    const input = await wait(() => one('[aria-label="歌单分享链接"]'), 'playlist link dialog');
    input.value = String(playlist);
    input.dispatchEvent(new Event('input', { bubbles: true }));
    click(one('.playlist-dialog button.primary'));
    click(await wait(() => one('.playlist-hero h1')?.textContent === `合成关注歌单 ${playlist}` &&
      !one('.playlist-dialog[open]') && one('button.playlist-creator'), 'playlist creator'));
    await wait(() => one('.playlist-creator-page h1')?.textContent === `合成关注用户 ${uid}`,
      'verified profile header');
  }
  try {
    await wait(() => button('我的歌单') && !button('我的歌单').disabled, 'application ready');
    await emit('resize');
    await wait(() => innerWidth === 760, 'resize');
    await openProfile(31, 73);
    const first = await wait(() => followButton('关注') && button('粉丝 10') && followButton('关注'),
      'guest follow and initial count');
    await capture('guest_ready');
    click(first);
    await wait(() => followButton('已互关') && button('粉丝 12'), 'direct follow and server count');
    assert(!dialog(), 'follow opened a confirmation');
    await capture('followed');

    click(followButton('已互关'));
    await wait(() => dialog() && document.activeElement === button('继续关注', dialog()),
      'official unfollow confirmation safely focuses continue');
    assert(dialog().getAttribute('aria-label') === '确定取消关注该好友?', 'wrong confirmation');
    await capture('confirm');
    click(button('继续关注', dialog()));
    await wait(() => !dialog() && document.activeElement === followButton('已互关'), 'cancel focus');
    await capture('cancel');

    click(followButton('已互关'));
    await wait(() => dialog(), 'reopen confirmation');
    click(button('仍要取消', dialog()));
    await wait(() => followButton('核对关注状态') && followButton('已互关')?.disabled &&
      button('粉丝 12'), 'lost response keeps old state and read-only reconciliation');
    await capture('uncertain');
    click(followButton('核对关注状态'));
    await wait(() => followButton('关注') && button('粉丝 8') &&
      !followButton('核对关注状态'), 'readback verified without second unfollow');
    await capture('readback');

    for (const [playlist, uid, name] of [[32, 2, 'own'], [33, 74, 'target_blocked'],
      [34, 75, 'closed'], [35, 76, 'viewer_blocked']]) {
      await openProfile(playlist, uid);
      assert(!follow() && !dialog(), `unexpected follow control on ${name}`);
      await capture(name);
    }
    await emit('complete', { ok: true, stageCount, realMusicRequests: 0, remoteWrites: 0 });
  } catch (error) {
    await emit('failed', { message: String(error), stageCount });
  }
})();
