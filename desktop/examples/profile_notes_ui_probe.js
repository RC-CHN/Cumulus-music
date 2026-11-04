// Production profile navigation with an isolated, read-only notes service.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])];
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const button = (name, root = document) =>
    all('button', root).find((node) => (node.getAttribute('aria-label') || node.textContent.trim()) === name);
  const assert = (condition, label) => { if (!condition) throw Error(label); };
  const section = () => one('[aria-label="用户笔记"]');
  const cards = () => all('.profile-note-list > article', section());
  const back = () => button('返回上一页');
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
    assert(node && !node.disabled, 'missing or disabled button');
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
    assert(!document.body.textContent.includes('_PRIVATE'), 'private data reached the DOM');
    await emit(`ux_${String(++stageCount).padStart(2, '0')}_${label}`, {
      width: innerWidth, visibleNotes: cards().length,
      openerFocused: !!document.activeElement?.dataset.communityOpener,
    });
  }

  async function openProfile(id, uid) {
    click(button('我的歌单'));
    click(await wait(() => button('打开歌单链接'), 'playlist library'));
    const input = await wait(() => one('[aria-label="歌单分享链接"]'), 'playlist link dialog');
    input.value = String(id);
    input.dispatchEvent(new Event('input', { bubbles: true }));
    click(one('.playlist-dialog button.primary'));
    const creator = await wait(() => one('.playlist-hero h1')?.textContent === `合成笔记歌单 ${id}` &&
      !one('.playlist-dialog[open]') && one('button.playlist-creator'), 'synthetic playlist creator');
    click(creator);
    return wait(() => one('.playlist-creator-page h1')?.textContent === `合成笔记作者 ${uid}` &&
      button('笔记 17'), 'guest profile and notes count');
  }

  try {
    await wait(() => button('我的歌单') && !button('我的歌单').disabled, 'application ready');
    await emit('resize');
    await wait(() => innerWidth === 760, 'resize');
    click(await openProfile(20, 73));
    const firstRetry = await wait(() => section()?.getAttribute('aria-busy') === 'false' &&
      one('[role=alert]', section()) && button('重试', section()), 'first read failed with retry');
    assert(!cards().length && !section().textContent.includes('暂无笔记'), 'failure treated as empty');
    await capture('first_error');
    click(firstRetry);
    await wait(() => cards().length === 12, 'first read recovered, only verified author rows');
    assert(!section().textContent.includes('匿名用户'), 'anonymous row was rendered');
    await capture('first_page');

    const first = cards().find((card) => one('h2', card)?.textContent === '合成笔记 31');
    const detail = one('button[data-community-opener]', first);
    const opener = detail?.dataset.communityOpener;
    assert(opener, 'verified row did not have an opaque detail opener');
    click(detail);
    await wait(() => one('.community-heading h1')?.textContent === '动态详情' &&
      one('.community-page')?.textContent.includes('合成笔记详情') &&
      one('.community-page')?.textContent.includes('合成笔记评论'), 'exact note detail and comments');
    await capture('detail');
    click(back());
    await wait(() => cards().length === 12 &&
      document.activeElement?.dataset.communityOpener === opener, 'back restores exact profile note and focus');
    await capture('back_focus');

    const more = button('更多笔记', section());
    assert(more, 'continuation affordance missing');
    more.scrollIntoView({ block: 'end', behavior: 'instant' });
    if (!more.disabled) click(more);
    const continuationRetry = await wait(() => section()?.getAttribute('aria-busy') === 'false' &&
      one('[role=alert]', section()) && button('重试', section()), 'continuation failed with retry');
    assert(cards().length === 12, 'continuation failure discarded first page');
    await capture('next_error');
    click(continuationRetry);
    await wait(() => cards().length === 13 && section().textContent.includes('合成续页笔记 43'),
      'continuation recovered with verified author');
    assert(!button('更多笔记', section()), 'terminal page still advertises continuation');
    await capture('next_page');

    const blockedTab = await openProfile(21, 74);
    await emit('profile-notes-deny');
    await wait(() => window.__profileNotesBlocked === true, 'fixture revoked guest authorization');
    click(blockedTab);
    await wait(() => section()?.textContent.includes('此用户的笔记不可查看'), 'fresh guest authorization denied');
    assert(!cards().length && !section().textContent.includes('暂无笔记') &&
      !button('更多笔记', section()), 'blocked profile reused previous owner notes');
    await capture('blocked');
    click(back());
    await wait(() => one('button.playlist-creator')?.textContent.trim() === '合成笔记作者 74' &&
      !section(), 'profile tabs replaced one history slot; back returns to the original playlist');
    await capture('blocked_back');
    await emit('complete', { ok: true, stageCount, realMusicRequests: 0, remoteWrites: 0 });
  } catch (error) {
    await emit('failed', { message: String(error), stageCount });
  }
})();
