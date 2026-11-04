// Production profile playlist and detail routes with a closed synthetic transport.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const button = (name) => all('button').find(
    (node) => (node.getAttribute('aria-label') || node.textContent.trim()) === name,
  );
  const ids = () => all('.playlist-creator-page [data-library-open]').map((item) => item.dataset.libraryOpen);
  const card = (id) => one(`.playlist-creator-page [data-library-open="${id}"]`);
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const assert = (value, label) => { if (!value) throw Error(label); };
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
    assert(!document.body.textContent.includes('RAW_PRIVATE_PIN'), 'raw pin metadata reached the renderer');
    await emit(`ux_${String(++stageCount).padStart(2, '0')}_${label}`, {
      width: innerWidth, ids: ids(), focused: document.activeElement?.dataset.libraryOpen,
    });
  }
  async function openProfile(playlist, uid) {
    click(button('我的歌单'));
    click(await wait(() => button('打开歌单链接'), 'playlist library'));
    const input = await wait(() => one('[aria-label="歌单分享链接"]'), 'playlist link dialog');
    input.value = String(playlist);
    input.dispatchEvent(new Event('input', { bubbles: true }));
    click(one('.playlist-dialog button.primary'));
    click(await wait(() => one('.playlist-hero h1')?.textContent === `合成主页歌单 ${playlist}` &&
      !one('.playlist-dialog[open]') && one('button.playlist-creator'), 'playlist creator'));
    await wait(() => one('.playlist-creator-page h1')?.textContent === `合成歌单作者 ${uid}`,
      'verified profile header');
  }
  try {
    await wait(() => button('我的歌单') && !button('我的歌单').disabled, 'application ready');
    await emit('resize');
    await wait(() => innerWidth === 760, 'resize');
    await openProfile(20, 73);
    await wait(() => same(ids(), ['20', '26', '25']), 'guest public groups and pinned order');
    assert(!button('创建的歌单 4') && !!button('创建的歌单 3') &&
      !card(27) && !card(28), 'guest private rows counted or rendered');
    await capture('guest_created');
    click(button('收藏的歌单 1'));
    await wait(() => same(ids(), ['29']), 'guest collected is public');
    await capture('guest_collected');

    await openProfile(22, 2);
    await wait(() => same(ids(), ['41', '42', '46', '22']), 'owner first item, pins and private playlist');
    assert(card(42).getAttribute('aria-description') === '私密歌单' &&
      card(41).getAttribute('aria-description') === '私密歌单' &&
      !!button('创建的歌单 4') && !!one('[data-profile-rank-entry]') &&
      !card(43) && !card(47), 'private card safety, lock and rank entry');
    await capture('own_created');
    click(card(42));
    await wait(() => one('.playlist-hero h1')?.textContent === '合成主页歌单 42' &&
      one('.playlist-privacy[aria-label="私密歌单"]'), 'private playlist opens own detail with lock');
    assert(!one('.playlist-art .playlist-cover-count'), 'private cover exposed play count');
    await capture('own_private_detail');
    click(button('返回上一页'));
    await wait(() => document.activeElement === card(42) && same(ids(), ['41', '42', '46', '22']),
      'private detail returns focus to the same ordered profile card');
    await capture('own_back_focus');
    click(button('收藏的歌单 1'));
    await wait(() => same(ids(), ['45']), 'owner collected excludes someone else private list');
    await capture('own_collected');

    await openProfile(21, 74);
    await wait(() => one('.playlist-creator-page [role="status"]')?.textContent.trim() === '此用户的歌单不可查看',
      'blocked guest profile');
    assert(!ids().length && !one('[aria-label="用户主页分类"]'), 'blocked guest still rendered profile tabs');
    await capture('blocked');
    await emit('complete', { ok: true, stageCount, realMusicRequests: 0, remoteWrites: 0 });
  } catch (error) {
    await emit('failed', { message: String(error), stageCount });
  }
})();
