(async () => {
  const all = (selector, root = document) => [...root.querySelectorAll(selector)];
  const one = (selector) => document.querySelector(selector);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (condition, label) => { if (!condition) throw Error(label); };
  const emit = (stage, rest = {}) =>
    window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
      event: 'ux-proof', payload: { stage, ...rest },
    });
  async function wait(test, label) {
    for (let i = 0; i < 180; i++) {
      if (test()) return;
      await sleep(80);
    }
    throw Error(label);
  }
  const page = () => one('.community-page');
  const body = () => page()?.textContent ?? '';
  const button = (label, root = document) =>
    root && all('button', root).find((node) => node.textContent.trim() === label);
  const back = () => one('[aria-label="返回上一页"]');
  function click(node) {
    assert(node && !node.disabled, 'click target unavailable');
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    node.click();
  }
  const feedCount = () => all('.community-events > article').length;
  let count = 0;
  async function capture(label) {
    if (['first_error', 'feed', 'back_to_feed', 'empty'].includes(label)) page().scrollTop = 0;
    const anchor =
      label === 'comments' || label === 'comment_next' ? one('.community-comments h3') :
      label === 'next_recovered'
        ? all('.community-events > article').find((article) => article.textContent.includes('匿名用户'))
        : null;
    anchor?.scrollIntoView({ block: 'center', behavior: 'instant' });
    // Capture the painted compositor state, not merely the next Vue DOM tick.
    await Promise.race([
      new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))),
      sleep(180),
    ]);
    await sleep(240);
    assert(document.documentElement.scrollWidth <= innerWidth, 'horizontal overflow');
    assert(feedCount() <= 24, 'feed DOM exceeds render window');
    assert(!body().includes('SYNTHETIC_PRIVATE'), 'private field reached DOM');
    await emit(`ux_${String(++count).padStart(2, '0')}_${label}`, {
      width: innerWidth, height: innerHeight, theme: document.documentElement.dataset.theme,
      feedCount: feedCount(), detail: one('.community-heading h1')?.textContent === '动态详情',
    });
  }
  try {
    await wait(() => button('关注') && !button('关注').disabled, 'application ready');
    await emit('resize');
    await wait(() => innerWidth === 760, 'narrow window');
    click(one('[aria-label="外观主题"]'));
    await wait(() => button('浅色'), 'theme menu');
    click(button('浅色'));
    await wait(() => document.documentElement.dataset.theme === 'light', 'light theme');
    click(button('关注'));
    await wait(() => one('.community-page [role=alert]'), 'first feed failure');
    assert(feedCount() === 0 && !body().includes('暂无关注动态'), 'failure disguised as empty');
    await capture('first_error');
    click(button('重试', page()));
    await wait(() => feedCount() === 5, 'first page');
    assert(body().includes('该笔记类型暂不支持展示') && !body().includes('不得显示的类型'), 'unsupported row');
    const first = one('[aria-label="查看 合成作者 11 的动态详情"]');
    assert(first && body().includes('合成动态专辑'), 'first row and attachment');
    await capture('feed');
    click(first);
    await wait(() => one('.community-heading h1')?.textContent === '动态详情' && one('.community-page [role=alert]'), 'detail error');
    await capture('detail_error');
    click(button('重试', page()));
    await wait(() => body().includes('合成详情正文') && button('重试评论', page()), 'read detail, comment error');
    await capture('comment_error');
    click(button('重试评论', page()));
    await wait(() => body().includes('合成评论一') && body().includes('合成精彩评论'), 'comment retry');
    assert(!one('.community-page textarea'), 'write UI is present');
    await capture('comments');
    click(button('下一页评论', page()));
    await wait(() => body().includes('合成评论续页'), 'comment page two');
    assert(!body().includes('合成评论一'), 'comment page was not replaced');
    await capture('comment_next');
    click(back());
    await wait(() => feedCount() === 5 && document.activeElement?.getAttribute('aria-label') === first.getAttribute('aria-label'), 'feed and entry focus restored');
    await capture('back_to_feed');
    click(one('[aria-label="查看 合成作者 15 的动态详情"]'));
    await wait(() => body().includes('这条笔记已不存在或无法查看'), 'missing detail');
    await capture('unavailable');
    click(button('返回关注', page()));
    await wait(() => feedCount() === 5, 'feed after missing detail');
    click(one('[aria-label="外观主题"]'));
    await wait(() => button('深色'), 'theme menu');
    click(button('深色'));
    await wait(() => document.documentElement.dataset.theme === 'dark', 'dark theme');
    button('加载更多动态', page()).scrollIntoView({ block: 'end' });
    await wait(() => one('.community-page [role=alert]'), 'scroll next failed');
    assert(feedCount() === 5, 'next failure removed loaded cards');
    await capture('next_error');
    click(button('重试', page()));
    await wait(() => body().includes('合成第二页') && feedCount() === 6, 'next retry');
    assert(body().includes('匿名用户') && !body().includes('合成作者 13') &&
      !one('[aria-label="查看 合成作者 13 的动态详情"]'), 'anonymous replacement leaked author or action');
    await capture('next_recovered');
    click(button('关注'));
    await wait(() => body().includes('暂无关注动态'), 'sidebar reselect refresh empty');
    await capture('empty');
    await emit('complete', { ok: true, stageCount: count, realMusicRequests: 0, remoteWrites: 0 });
  } catch (error) {
    await emit('failed', { ok: false, stageCount: count, error: String(error),
      focus: document.activeElement?.getAttribute('aria-label') ?? document.activeElement?.tagName,
      cards: feedCount(),
    });
  }
})();
