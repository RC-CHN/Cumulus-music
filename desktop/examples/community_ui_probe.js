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
    await wait(() => one('.sidebar-new-song-dot'), 'account-scoped new song indicator');
    await emit('resize');
    await wait(() => innerWidth === 760, 'narrow window');
    click(one('[aria-label="外观主题"]'));
    await wait(() => button('浅色'), 'theme menu');
    click(button('浅色'));
    await wait(() => document.documentElement.dataset.theme === 'light', 'light theme');
    click(button('关注'));
    await wait(() => one('.community-page [role=alert]'), 'first feed failure');
    await wait(() => one('.new-song-notice') && !one('.sidebar-new-song-dot'), 'visit loads card and clears indicator');
    assert(feedCount() === 0 && !body().includes('暂无关注动态'), 'failure disguised as empty');
    await capture('first_error');
    click(button('重试', page()));
    await wait(() => feedCount() === 5, 'first page');
    assert(body().includes('该笔记类型暂不支持展示') && !body().includes('不得显示的类型'), 'unsupported row');
    const first = one('[aria-label="查看 合成作者 11 的动态详情"]');
    assert(first && body().includes('合成动态专辑'), 'first row and attachment');
    await capture('feed');
    const trigger = button('写笔记', page());
    const composer = () => one('.community-composer[open]');
    const input = (node, value) => {
      node.value = value;
      node.dispatchEvent(new Event('input', { bubbles: true }));
    };
    click(trigger);
    await wait(() => composer()?.querySelector('textarea') === document.activeElement, 'draft text receives focus');
    input(composer().querySelector('textarea'), '这是本机合成草稿');
    await wait(() => button('发布笔记', composer())?.disabled, 'text-only draft must not publish');
    await capture('composer_draft');
    click(button('添加音乐附件', composer()));
    await wait(() => one('[aria-label="搜索音乐附件"]') === document.activeElement, 'attachment query receives focus');
    input(one('[aria-label="搜索音乐附件"]'), '合成附件');
    await wait(() => button('搜索', composer())?.disabled === false, 'query enables search');
    click(button('搜索', composer()));
    await wait(() => button('合成附件单曲 8', composer()), 'eight official song results');
    click(button('加载更多附件', composer()));
    await wait(() => button('重试', composer()), 'attachment continuation failure');
    assert(button('合成附件单曲 1', composer()), 'continuation failure preserves rows');
    click(button('重试', composer()));
    await wait(() => button('合成附件单曲续页', composer()), 'retry appends exact offset');
    await capture('composer_search');
    for (const type of ['歌手','歌单','播客','专辑']) {
      click(button(type, composer()));
      await wait(() => button('合成附件' + type, composer()), 'attachment tab ' + type);
    }
    click(button('合成附件专辑', composer()));
    await wait(() => composer()?.querySelector('textarea')?.value === '这是本机合成草稿' &&
      composer().textContent.includes('专辑：合成附件专辑'), 'one attachment restores draft text');
    click(button('保存草稿', composer()));
    await wait(() => composer()?.textContent.includes('草稿已保存在本机'), 'native durable save');
    composer().dispatchEvent(new Event('cancel', {cancelable:true}));
    await wait(() => !composer() && document.activeElement === trigger, 'draft close returns to composer trigger');
    click(trigger);
    await wait(() => composer()?.querySelector('textarea')?.value === '这是本机合成草稿' &&
      composer().textContent.includes('专辑：合成附件专辑'), 'reopened draft uses native record');
    assert(!composer().textContent.includes('SYNTHETIC_PRIVATE'), 'raw attachment data reached composer');
    await capture('composer_restored');
    composer().dispatchEvent(new Event('cancel', {cancelable:true}));
    await wait(() => !composer() && document.activeElement === trigger, 'restored draft closes');
    click(one('.new-song-notice'));
    await wait(() => one('.follow-release-page')?.textContent.includes('关注的歌手最近没有发布内容'),
      'new song notice opens the actual followed-release route');
    click(back());
    await wait(() => feedCount() === 5 && document.activeElement?.matches('.new-song-notice'),
      'return restores the notice card and its focus');
    await capture('notice_return');
    click(one('[aria-label="查看 合成作者 11 的动态详情"]'));
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
    await wait(() => !one('.new-song-notice'), 'no replacement notice on the second visit');
    await capture('empty');
    await emit('complete', { ok: true, stageCount: count, composerVerified: true, realMusicRequests: 0, remoteWrites: 0 });
  } catch (error) {
    await emit('failed', { ok: false, stageCount: count, error: String(error),
      focus: document.activeElement?.getAttribute('aria-label') ?? document.activeElement?.tagName,
      cards: feedCount(),
    });
  }
})();
