// Real WebView and production message IPC, exclusively synthetic native responses.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (condition, detail) => { if (!condition) throw Error(detail); };
  const wait = async (predicate, detail) => {
    for (let i = 0; i < 220; i++) {
      const result = await predicate();
      if (result) return result;
      await sleep(50);
    }
    throw Error(detail);
  };
  const invoke = (name, data = {}) => window.__TAURI_INTERNALS__.invoke(name, data);
  const emit = (stage, data = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...data } });
  const button = (label) => all('.message-drawer button')
    .find((node) => node.getAttribute('aria-label') === label || node.textContent.trim() === label);
  const click = (node) => {
    assert(node?.isConnected && !node.disabled, 'expected an enabled visible message control');
    node.focus({ preventScroll: true });
    node.click();
  };
  const drawer = () => one('.message-drawer');
  const scroller = () => one('.message-drawer-scroll');
  const trigger = () => one('.message-drawer-trigger');
  const ready = () => scroller()?.getAttribute('aria-busy') === 'false';
  const contains = (text) => drawer()?.textContent.includes(text);
  const pick = (tab) => click(one(`[data-message-tab="${tab}"]`));
  let stage = 'startup';
  let sequence = 0;
  let count = 0;
  window.__messageFixtureAck = {};
  async function fixture(mode = 'read') {
    const id = ++sequence;
    await emit('message-fixture', { mode, id });
    return wait(() => window.__messageFixtureAck[id], 'native message fixture acknowledgement');
  }
  async function capture(label) {
    stage = label;
    // WebKit's Web Animations list can settle before the transformed fixed
    // box reports its final position. Wait for the real layout as well.
    await wait(
      () => {
        const panel = drawer();
        if (!panel) return false;
        const bounds = panel.getBoundingClientRect();
        return Math.abs(bounds.right - innerWidth) < 0.5 && panel.getAnimations().every(
          (animation) => !animation.pending && animation.playState !== 'running',
        );
      },
      'drawer transition settled',
    );
    const bounds = drawer().getBoundingClientRect();
    assert(bounds.width <= 387 && bounds.right <= innerWidth + 1 && bounds.bottom <= innerHeight,
      'drawer remains within window: ' + JSON.stringify({ bounds: bounds.toJSON(), innerWidth, innerHeight, position: getComputedStyle(drawer()).position }));
    assert(document.documentElement.scrollWidth <= innerWidth, 'drawer causes no horizontal overflow');
    await emit(`ux_${String(++count).padStart(2, '0')}_${label}`, {
      label, dark: document.documentElement.dataset.theme === 'dark',
      rows: all('.message-drawer-item').length,
    });
    await sleep(170);
  }
  try {
    await wait(() => trigger() && !trigger().disabled && one('.track-row'), 'authenticated initial page');
    assert((await fixture()).requests.length === 0, 'no background unread or message reads');
    const account = await invoke('account_snapshot');
    assert(account.status === 'authenticated', 'isolated account active');
    const stale = await invoke('message_page', {
      epoch: account.epoch + '0', query: { tab: 'notices', cursor: null },
    }).then(() => null, (error) => error);
    assert(stale?.code === 'stale' && (await fixture()).requests.length === 0,
      'wrong account generation is rejected before contacting the service');
    click(trigger());
    await wait(() => ready() && contains('私信预览 1') && contains('私信预览 15')
      && one('[data-message-tab="notices"] .message-drawer-badge')?.textContent.trim() === '4',
    'private inbox and server unread counts');
    assert(!contains('一键全读') && !drawer().querySelector('textarea'),
      'read-only drawer has no mark-all or compose controls');
    await capture('private');
    scroller().scrollTop = 170;
    await wait(() => scroller().scrollTop > 0, 'private scroller moved');
    const before = scroller().scrollTop;
    click(one('[data-message-peer="505"]'));
    await wait(() => ready() && contains('较新的自己消息'), 'selected private thread');
    let items = all('.message-drawer-item');
    assert(items.length === 2 && items[1].classList.contains('message-drawer-own'),
      'thread places newer and own messages correctly');
    click(button('查看更早消息'));
    await wait(() => ready() && contains('不打开的附件'), 'older thread history');
    items = all('.message-drawer-item');
    assert(items.length === 3 && items[0].textContent.includes('不打开的附件') &&
      !drawer().querySelector('a, iframe, img') && !drawer().textContent.includes('private.invalid'),
      'oldest thread entry prepended with inert attachment');
    await capture('thread');
    click(button('返回私信列表'));
    await wait(() => document.activeElement?.dataset.messagePeer === '505'
      && Math.abs(scroller().scrollTop - before) < 1, 'private list return restores scroll and focus');
    scroller().scrollTop = scroller().scrollHeight;
    await wait(() => ready() && contains('合成较早联系人'), 'private list cursor pagination');
    await capture('private_return');
    pick('comments');
    await wait(() => ready() && contains('评论正文') && contains('引用原文'), 'received comments');
    click(button('加载更多'));
    await wait(() => ready() && contains('更早的消息读取失败') && contains('评论正文'),
      'continuation failure preserves loaded comment');
    await capture('comment_retry');
    click(button('重试'));
    await wait(() => ready() && contains('较早评论') && contains('评论正文'), 'same comment cursor retries');
    pick('forwards');
    await wait(() => ready() && contains('@合成自己') && contains('引用的笔记'), 'mentions rendered');
    click(button('加载更多'));
    await wait(() => ready() && !button('加载更多'), 'forward server cursor and end');
    await capture('forwards');
    pick('notices');
    await wait(() => ready() && contains('收藏了你的歌单') && contains('合成歌单'),
      'known notification shape');
    click(button('加载更多'));
    await wait(() => ready() && !button('加载更多'), 'notice server cursor and end');
    assert(one('[data-message-tab="notices"] .message-drawer-badge')?.textContent.trim() === '4',
      'visiting notifications does not clear unread count');
    await capture('notices');
    await fixture('hold');
    click(button('刷新消息'));
    await wait(async () => (await fixture()).noticeHeld, 'native notice refresh held');
    drawer().dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await Promise.resolve();
    assert(!drawer() && document.activeElement === trigger(), 'Escape purges private DOM immediately and restores focus');
    click(trigger());
    await wait(() => ready() && contains('私信预览 1'), 'new private list session');
    await fixture('release');
    await wait(async () => !(await fixture()).noticeHeld, 'stale notice reply released');
    pick('notices');
    await wait(() => ready() && contains('合成歌单'), 'fresh notification tab after reopen');
    assert(!contains('旧面板数据不能显示'), 'old drawer response is ignored');
    await capture('reopened');
    const result = await fixture();
    assert(!result.blockedWrite, 'native service observed no clear/send/delete request');
    await emit('complete', { ok: true, count, staleVisitDiscarded: true, realAccountWrites: 0, realMusicRequests: 0 });
  } catch (error) {
    await emit('failed', { stageLabel: stage, message: String(error) });
  }
})();
