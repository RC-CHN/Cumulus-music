// Production profile podcast UI and native read-only requests against a closed API.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])];
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const button = (name, root = document) =>
    all('button', root).find((node) => (node.getAttribute('aria-label') || node.textContent.trim()) === name);
  const assert = (value, label) => { if (!value) throw Error(label); };
  const section = () => one('[aria-label="用户播客"]');
  const group = (kind) => one(`[data-profile-podcast-heading="${kind}"]`, section())?.closest('section');
  const cards = (kind) => all(`[data-profile-podcast-group="${kind}"]`);
  const card = (id) => one(`[data-profile-podcast="${id}"]`);
  const emit = (stage, payload = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...payload },
  });
  async function wait(test, label) {
    for (let i = 0; i < 170; i++) {
      const found = test();
      if (found) return found;
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
    assert(!document.body.textContent.includes('PAYLOAD_PRIVATE'), 'private native field reached UI');
    assert(cards('created').length <= 20 && cards('collected').length <= 20, 'unbounded UI group');
    await emit(`ux_${String(++stageCount).padStart(2, '0')}_${label}`, {
      width: innerWidth, created: cards('created').length, collected: cards('collected').length,
      focused: document.activeElement?.dataset.profilePodcast,
    });
  }
  async function openProfile(id, uid, total) {
    click(button('我的歌单'));
    click(await wait(() => button('打开歌单链接'), 'playlist library'));
    const input = await wait(() => one('[aria-label="歌单分享链接"]'), 'playlist link dialog');
    input.value = String(id);
    input.dispatchEvent(new Event('input', { bubbles: true }));
    click(one('.playlist-dialog button.primary'));
    click(await wait(() => one('.playlist-hero h1')?.textContent === `合成播客歌单 ${id}` &&
      !one('.playlist-dialog[open]') && one('button.playlist-creator'), 'playlist creator'));
    const tab = await wait(() => one('.playlist-creator-page h1')?.textContent === `合成播客作者 ${uid}` &&
      button(`播客 ${total}`), 'guest/owner header with aggregate count');
    assert(button('笔记 3') && button('歌单'), 'profile sibling tab missing');
    return tab;
  }
  const back = () => button('返回上一页');
  try {
    await wait(() => button('我的歌单') && !button('我的歌单').disabled, 'application ready');
    await emit('resize');
    await wait(() => innerWidth === 760, 'resize');
    click(await openProfile(20, 73, 42));
    const favRetry = await wait(() => cards('created').length === 19 &&
      button('重试', group('collected')), 'guest groups and collected error');
    assert(!card(820) && !button('管理播客') &&
      group('created').getAttribute('aria-label') === 'Ta创建的播客' &&
      group('collected').getAttribute('aria-label') === 'Ta收藏的播客', 'guest ownership boundary');
    await capture('guest_error');
    click(favRetry);
    await wait(() => cards('collected').length === 20, 'collected retry');
    click(button('列表显示', group('created')));
    await wait(() => button('列表显示', group('created')).getAttribute('aria-pressed') === 'true',
      'created group switched to list');
    assert(group('created').textContent.includes('17 人收藏') &&
      !group('created').textContent.includes('合成播客作者 73') &&
      button('网格显示', group('collected')).getAttribute('aria-pressed') === 'true',
      'list shows subscriber count and layout mode is independent');
    await capture('guest_list');

    click(button('下一页', group('created')));
    const createRetry = await wait(() => button('重试', group('created')), 'created continuation error');
    assert(cards('created').length === 19, 'failed created continuation lost first page');
    await capture('created_error');
    click(createRetry);
    await wait(() => card(821) && cards('created').length === 1, 'created numeric next offset recovered');
    assert(button('列表显示', group('created')).getAttribute('aria-pressed') === 'true', 'layout reset on page change');
    await capture('created_next');
    click(card(821));
    await wait(() => one('.podcast-heading h1')?.textContent === '播客详情' &&
      one('.podcast-summary h2')?.textContent === '合成播客 821', 'exact native podcast detail');
    assert(!button('返回播客列表'), 'profile detail offers unrelated library history');
    await capture('detail');
    click(back());
    await wait(() => document.activeElement === card(821) &&
      button('播客 42').getAttribute('aria-pressed') === 'true', 'profile tab Back restored exact opener');
    await capture('back_focus');

    click(button('下一页', group('collected')));
    await wait(() => cards('collected').length === 1 && card(921), 'collection uses local second page');
    assert(cards('created').length === 1 && card(821), 'collected paging changed created group');
    await capture('collected_next');
    click(button('歌单'));
    await wait(() => !section() && button('歌单').getAttribute('aria-pressed') === 'true', 'switch to sibling tab');
    click(button('播客 42'));
    await wait(() => card(821) && card(921), 'switch back preserves independent pages');
    click(back());
    await wait(() => one('.playlist-hero h1')?.textContent === '合成播客歌单 20',
      'profile siblings replace rather than stack history');
    await capture('source_back');

    click(await openProfile(22, 2, 2));
    await wait(() => card(701) && card(702), 'owner groups');
    assert(group('created').getAttribute('aria-label') === '我创建的播客' &&
      group('collected').getAttribute('aria-label') === '我收藏的播客', 'owner headings');
    for (const branch of ['sso', 'plain fallback']) {
      click(button('管理播客'));
      await wait(() => button('管理播客').disabled, `management ${branch} pending`);
      await wait(() => !button('管理播客').disabled, `management ${branch} handoff`);
      assert(!section().textContent.includes('无法打开浏览器'), 'browser handoff failed');
    }
    await capture('own_manage');

    click(await openProfile(21, 74, 2));
    await wait(() => section()?.textContent.includes('此用户的播客不可查看'), 'fresh guest denial');
    assert(!cards('created').length && !cards('collected').length && !button('管理播客'),
      'guest denial reused data or actions');
    await capture('blocked');
    await emit('complete', { ok: true, stageCount, realMusicRequests: 0, remoteWrites: 0 });
  } catch (error) {
    await emit('failed', { message: String(error), stageCount });
  }
})();
