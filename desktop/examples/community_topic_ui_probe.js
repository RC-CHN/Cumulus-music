// Actual followed-note hashtag navigation, per-topic lanes and detail return.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])];
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const button = (name, root = document) => all('button', root).find(
    (node) => (node.getAttribute('aria-label') || node.textContent.trim()) === name,
  );
  const topic = () => one('.community-topic-page');
  const group = (kind) => one(`[data-topic-group="${kind}"]`, topic());
  const cards = (kind) => all('.community-topic-cards > article', group(kind));
  const assert = (value, label) => { if (!value) throw Error(label); };
  const emit = (stage, payload = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...payload },
  });
  async function wait(test, label, limit = 180) {
    for (let i = 0; i < limit; i++) {
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
    assert(!document.body.textContent.includes('RAW_PRIVATE'), 'native-only topic field reached the DOM');
    assert(cards('hot').length <= 20 && cards('all').length <= 20, 'topic list is not bounded');
    await emit(`ux_${String(++stageCount).padStart(2, '0')}_${label}`, {
      width: innerWidth, hot: cards('hot').length, all: cards('all').length,
      focused: document.activeElement?.dataset.communityOpener ||
        document.activeElement?.dataset.topicOpener || null,
    });
  }
  const back = () => button('返回上一页');
  const notice = () => button('有新笔记，点击查看', topic());
  try {
    await wait(() => button('关注') && !button('关注').disabled, 'application ready');
    await emit('resize');
    await wait(() => innerWidth === 760, 'resize');
    click(button('关注'));
    const source = await wait(() => button('#听歌 100%#', one('.community-page')), 'literal hashtag in followed note');
    assert(!button('#坏/标签#', one('.community-page')), 'invalid tag was made actionable');
    source.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    const sourceTop = one('.community-page').scrollTop;
    assert(sourceTop > 0, 'source scroll proof needs a nonzero position');
    await capture('source');
    click(source);
    const retryHot = await wait(() => cards('all').length === 12 &&
      one('[role="alert"]', group('hot')) && button('重试', group('hot')), 'separate hot failure with first All');
    assert(one('.community-topic-banner h2')?.textContent === '#听歌 100%#' &&
      topic().textContent.includes('合成话题说明') && topic().textContent.includes('27 人参与'),
      'topic header and safe count');
    await capture('hot_error');
    click(retryHot);
    await wait(() => cards('hot').length === 2, 'hot retry');
    const hotDetail = button('查看 合成话题作者 31 的动态详情', group('hot'));
    const allDetail = button('查看 合成话题作者 31 的动态详情', group('all'));
    assert(hotDetail?.dataset.communityOpener === allDetail?.dataset.communityOpener, 'duplicate event has distinct receipts');
    const receipt = hotDetail.dataset.communityOpener;
    await capture('topic_ready');
    click(hotDetail);
    await wait(() => one('.community-heading h1')?.textContent === '动态详情' &&
      one('.community-page')?.textContent.includes('合成话题详情') &&
      one('.community-page')?.textContent.includes('合成话题评论'), 'exact native topic detail/comments');
    await capture('detail');
    click(back());
    await wait(() => cards('all').length === 12 &&
      document.activeElement?.dataset.communityOpener === receipt &&
      document.activeElement?.closest('[data-topic-group]')?.dataset.topicGroup === 'hot',
      'topic back restores exact hot lane opener');
    await capture('hot_back');

    function reachMore() {
      const more = button('更多笔记', group('all'));
      assert(more, 'missing continuation affordance');
      more.scrollIntoView({ block: 'end', behavior: 'instant' });
      if (!more.disabled) click(more);
    }
    reachMore();
    const retryAll = await wait(() => one('[role="alert"]', group('all')) &&
      button('重试', group('all')), 'first continuation failed');
    assert(cards('all').length === 12 && cards('hot').length === 2, 'continuation failure discarded data');
    await capture('next_error');
    click(retryAll);
    await wait(() => cards('all').length === 20 && button('更多笔记', group('all')), 'string cursor retry');
    await capture('string_next');
    reachMore();
    await wait(() => group('all').textContent.includes('合成末页笔记') ||
      button('下一页', group('all')), 'numeric continuation');
    if (!group('all').textContent.includes('合成末页笔记')) click(button('下一页', group('all')));
    await wait(() => cards('all').length === 1 && group('all').textContent.includes('合成末页笔记'),
      'numeric continuation rendered');
    await capture('number_next');

    const check = await wait(notice, 'foreground 20-second new-note notice', 420);
    assert(!topic().textContent.includes('合成刷新全部') && cards('hot').length === 2,
      'notice automatically refreshed the lists');
    await capture('notice');
    click(check);
    await wait(() => group('all')?.textContent.includes('合成刷新全部') &&
      group('hot')?.textContent.includes('合成刷新热门') && !notice(), 'explicit refresh');
    await capture('refresh');
    click(back());
    await wait(() => document.activeElement === button('#听歌 100%#', one('.community-page')) &&
      Math.abs(one('.community-page').scrollTop - sourceTop) <= 2, 'return to original note and scroll');
    await capture('source_back');

    click(button('#不存在#', one('.community-page')));
    await wait(() => topic()?.textContent.includes('此话题不存在或无法查看'), 'not found distinct from empty');
    assert(!cards('hot').length && !cards('all').length, 'missing topic reused previous lists');
    await capture('unavailable');
    await emit('complete', { ok: true, stageCount, realMusicRequests: 0, remoteWrites: 0 });
  } catch (error) {
    await emit('failed', { message: String(error), stageCount, foreground: document.hasFocus() });
  }
})();
