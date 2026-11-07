// Real WebView and production message IPC, exclusively synthetic native responses.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (condition, detail) => { if (!condition) throw Error(detail); };
  const wait = async (predicate, detail, attempts = 220) => {
    for (let i = 0; i < attempts; i++) {
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
  const dot = () => trigger()?.querySelector('.message-drawer-unread-dot');
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
  const countCalls = async () => (await fixture()).requests
    .filter(([path]) => path === '/api/pl/count').length;
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
    let paint = {};
    if (window.__cumulusMessageDeletionProof) {
      const confirm = label === 'delete_conversation_confirm';
      const modal = await wait(() => {
        const dialog = one('[data-message-delete-confirm]');
        if (!confirm) return dialog ? false : { closed: true };
        if (!dialog?.open || dialog.getBoundingClientRect().width <= 0 ||
            getComputedStyle(dialog).opacity !== '1' ||
            dialog.getAnimations().some((a) => a.pending || a.playState === 'running'))
          return false;
        return dialog;
      }, 'actual confirmation layout settled');
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      paint = {
        ratio: devicePixelRatio, screenX, screenY,
        dialogs: confirm ? [{
          open: true, rect: modal.getBoundingClientRect().toJSON(), opacity: '1',
          background: getComputedStyle(modal).backgroundColor,
        }] : [],
        paintSamples: confirm ? [] : [{
          x: bounds.left + 4, y: bounds.top + 35,
          color: getComputedStyle(drawer()).backgroundColor,
        }],
      };
    }
    if (window.__cumulusMessagePollProof) {
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      const indicator = dot();
      const rect = indicator?.getBoundingClientRect();
      paint = { ratio: devicePixelRatio, screenX, screenY, paintSamples: [{
        x: bounds.left + 4, y: bounds.top + 35,
        color: getComputedStyle(drawer()).backgroundColor,
      }] };
      if (rect) paint.paintSamples.push({
        x: rect.x + rect.width / 2, y: rect.y + rect.height / 2,
        color: getComputedStyle(indicator).backgroundColor,
      });
    }
    await emit(`ux_${String(++count).padStart(2, '0')}_${label}`, {
      label, dark: document.documentElement.dataset.theme === 'dark',
      rows: all('.message-drawer-item').length,
      ...paint,
    });
    await sleep(170);
  }
  try {
    await wait(() => trigger() && !trigger().disabled && one('.track-row'), 'authenticated initial page');
    await wait(() => dot(), 'header unread dot before opening messages');
    assert((await fixture()).requests.length === 1 && await countCalls() === 1,
      'authenticated startup reads only message counts, once');
    const account = await invoke('account_snapshot');
    assert(account.status === 'authenticated', 'isolated account active');
    const stale = await invoke('message_page', {
      epoch: account.epoch + '0', query: { tab: 'notices', cursor: null },
    }).then(() => null, (error) => error);
    assert(stale?.code === 'stale' && (await fixture()).requests.length === 1,
      'wrong account generation is rejected before contacting the service');
    click(trigger());
    await wait(() => ready() && contains('私信预览 1') && contains('私信预览 15')
      && one('[data-message-tab="notices"] .message-drawer-badge')?.textContent.trim() === '4',
    'private inbox and server unread counts');
    assert(!contains('一键全读') && !drawer().querySelector('textarea'),
      'read-only drawer has no mark-all or compose controls');
    await wait(async () => await countCalls() === 2, 'visible drawer rearmed exactly once');
    await capture('private');
    if (window.__cumulusMessagePollProof) {
      await fixture('poll-zero');
      await wait(async () => !dot() && await countCalls() === 3, 'real 30-second native interval', 950);
      assert(!one('[data-message-tab="notices"] .message-drawer-badge'), 'zero hides tab badges and header dot');
      await capture('poll_zero');
      await fixture('poll-positive');
      await emit('message-poll-hide');
      const reveal = await wait(() => window.__messagePollRevealed, 'native window hide then reveal', 1100);
      assert(reveal.baseline === 3 && reveal.hidden === 4,
        'window hiding rearmed once and suppressed the next 30-second read');
      await wait(async () => dot() && await countCalls() === 5, 'window reveal returns the drawer to fast cadence');
      await capture('poll_foreground');
      await fixture('poll-fail');
      await wait(() => !button('刷新消息')?.disabled, 'current poll settled before manual refresh');
      click(button('刷新消息'));
      await wait(() => contains('未读数暂不可用') && dot(), 'failed read keeps previous header dot');
      assert(await countCalls() === 6, 'explicit message refresh uses the same count poller');
      await capture('poll_retry');
      await fixture('poll-positive');
      click(one('.message-drawer-count-error button'));
      await wait(() => !contains('未读数暂不可用') && dot(), 'explicit counter retry recovers without a new drawer');
      assert(await countCalls() === 7, 'counter retry read once');
      await emit('message-poll-switch');
      await wait(async () => !drawer() && !dot() && (await fixture()).countHeld,
        'switch hides old counts while the new account read is held');
      assert((await invoke('account_snapshot')).epoch !== account.epoch, 'native generation actually changed');
      await fixture('poll-release');
      await wait(async () => dot() && await countCalls() === 8, 'one new account read supplies the header dot');
      await emit('complete', { ok: true, messagePollingVerified: true, count,
        hiddenCount: reveal.hidden, realAccountWrites: 0, realMusicRequests: 0 });
      return;
    }
    if (window.__cumulusMessageReplyProof) {
      const textarea = () => one('[aria-label="私信正文"]');
      const notice = () => one('[data-message-reply-status]')?.textContent.trim();
      const send = () => button('发送');
      const writes = async () => (await fixture()).requests.filter(([path]) => path === '/api/msg/private/send');
      const input = async (value) => {
        const editor = textarea();
        editor.value = value;
        editor.dispatchEvent(new Event('input', { bubbles: true }));
        await Promise.resolve();
      };
      const key = (flags = {}) => {
        const event = new KeyboardEvent('keydown', { key:'Enter', cancelable:true, bubbles:true, ...flags });
        textarea().dispatchEvent(event);
        return event;
      };
      const enter = async (id, writable = true) => {
        click(one(`[data-message-peer="${id}"]`));
        await wait(() => ready() && textarea() && (!writable || !textarea().readOnly),
          'exact authenticated thread and composer ' + id);
      };
      const back = async () => {
        click(button('返回私信列表'));
        await wait(() => !textarea() && one('[data-message-peer="505"]'), 'only thread has editor');
      };
      await enter(505);
      assert(send().disabled, 'blank editor cannot send');
      await input('🪷'.repeat(501));
      await wait(() => send().disabled && contains('1002 / 1000') && contains('字数超出限制'),
        'official UTF-16 limit counts both surrogate code units');
      assert(key().defaultPrevented && (await writes()).length === 0, 'overlength Enter is fenced');
      await capture('reply_limit');
      const denied = '  合成拦截正文\n';
      await input(denied);
      assert(!key({shiftKey:true}).defaultPrevented && !key({isComposing:true}).defaultPrevented,
        'newline and composition Enter never send');
      await back();
      await enter(506);
      assert(textarea().value === '', 'other peer never inherits private draft');
      await back();
      await enter(505);
      await wait(() => textarea().value === denied && !send().disabled, 'same peer restores in-drawer draft');
      await capture('reply_peer_draft');
      assert(key().defaultPrevented, 'Enter is explicit send');
      await wait(() => one('.account-binding-qr-dialog[open] [data-account-binding-qr] path[d]'),
        'missing phone binding opens official fixed QR');
      assert((await writes()).length === 0 && textarea().value === denied, 'no message before phone binding');
      await sleep(220);
      await capture('reply_phone');
      click(one('[aria-label="关闭扫码设置"]'));
      await wait(() => !one('.account-binding-qr-dialog[open]') && document.activeElement === textarea(),
        'close QR returns to exact editor');
      click(send());
      await wait(() => notice()?.includes('被服务拦截') && textarea().value === denied && !textarea().readOnly,
        '2202 denial preserves whitespace and draft');
      assert(!contains('SYNTHETIC_RAW_INTERCEPTION'), 'raw service message is private');
      await capture('reply_denied');
      await input('  合成发送正文\n');
      key();
      await wait(() => notice()?.includes('已收到发送回执') && textarea().value === '' &&
        contains('服务端返回的合成已发送正文'), 'exact positive receipt clears editor and appends canonical row');
      assert(all('.message-drawer-own').at(-1)?.textContent.includes('服务端返回的合成已发送正文'),
        'native returned own message placed at bottom');
      await capture('reply_delivered');
      await fixture('reply-hold');
      await input('合成迟到发送正文');
      click(send());
      await wait(async () => (await fixture()).replyHeld, 'native positive receipt held');
      await back();
      await fixture('reply-release');
      await wait(async () => !(await fixture()).replyHeld, 'native receipt released outside the thread');
      await enter(505, false);
      const enabledCheck = () => {
        const check = button('核对本机回执');
        return check && !check.disabled && check;
      };
      await wait(() => notice()?.includes('已收到发送回执') || enabledCheck(), 'late receipt status ready');
      if (textarea().readOnly && enabledCheck()) click(enabledCheck());
      await wait(() => notice()?.includes('已收到发送回执') && textarea().value === '' &&
        contains('服务端返回的合成迟到正文'), 'late exact receipt clears only its previous draft');
      await capture('reply_late_delivery');
      await input('合成未知正文');
      click(send());
      await wait(() => textarea().readOnly && notice()?.includes('无法确认私信是否已发送'),
        'lost acknowledgement retains locked draft');
      assert(textarea().value === '合成未知正文' && send().disabled && !contains('合成未知正文'),
        'unknown reply never fabricates a sent row');
      await capture('reply_unknown');
      click(button('核对本机回执'));
      await wait(() => notice()?.includes('已核对本机回执'), 'explicit local-only status readback');
      click(button('刷新消息'));
      await wait(() => ready() && contains('合成未知正文') && textarea().readOnly,
        'matching historical body does not resolve the uncertain send');
      assert((await writes()).length === 4 && send().disabled, 'no retry from readback or history');
      await capture('reply_matching_history');
      click(button('关闭消息'));
      await wait(() => !drawer(), 'closing purges private draft body');
      click(trigger());
      await wait(() => ready() && one('[data-message-peer="505"]'), 'inbox reopens');
      await enter(505, false);
      await wait(() => notice()?.includes('无法确认私信是否已发送') && textarea().readOnly &&
        textarea().value === '', 'durable unknown survives reopen without cached private text');
      await capture('reply_reopened');
      await emit('complete', {
        ok: true, count, privateReplyVerified: true, realAccountWrites: 0, realMusicRequests: 0,
      });
      return;
    }
    if (window.__cumulusMessageDeletionProof) {
      const action = (id) => one(`[data-message-delete="${id}"]`);
      const menuItem = (key) => one(`[role="menu"][aria-label="私信操作"] [data-menu-key="${key}"]`);
      const dialog = () => one('dialog[open][data-message-delete-confirm]');
      const notice = () => one('[data-message-delete-result]')?.textContent.trim();
      const writes = async () => (await fixture()).requests.filter(([path]) => path.includes('/delete'));
      const open = async (id, key = 'remove', context = false) => {
        const trigger = await wait(() => {
          const button = action(id);
          return button?.isConnected && !button.disabled && button;
        }, 'deletion action enabled on exact row ' + id);
        if (context) {
          const row = trigger.closest('li');
          const box = row.getBoundingClientRect();
          row.dispatchEvent(new MouseEvent('contextmenu', {
            bubbles: true, cancelable: true, clientX: box.left + 20, clientY: box.top + 10,
          }));
        } else click(trigger);
        const choice = await wait(() => {
          const item = menuItem(key);
          return item?.isConnected && !item.disabled && !item.closest('[inert]') &&
            document.activeElement === item && item;
        }, 'private context action ready ' + key);
        choice.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }));
        click(choice);
      };
      const confirm = async (title, submit = true) => {
        const modal = await wait(dialog, 'native confirmation dialog');
        assert(modal.querySelector('h2').textContent === title, 'distinct official confirmation');
        await wait(() => document.activeElement === modal.querySelector('[autofocus]'),
          'cancel is default focus in destructive confirmation');
        click(modal.querySelector(submit ? '.primary' : '.secondary'));
        await wait(() => !dialog(), 'confirmation fully leaves');
      };
      await open(501, 'remove', true);
      await wait(dialog, 'conversation context menu opens modal');
      await capture('delete_conversation_confirm');
      await confirm('确定删除私信？', false);
      await wait(() => document.activeElement === action(501), 'cancel returns to exact row action');
      assert((await writes()).length === 0 && one('[data-message-peer="501"]'),
        'cancel never dispatches and leaves original conversation');
      await capture('delete_cancel');
      await open(502);
      await confirm('确定删除私信？');
      await wait(() => notice()?.includes('已保留原消息'), 'explicit service rejection reports failure');
      assert(one('[data-message-peer="502"]') && action(502).title === '私信操作',
        'rejection preserves conversation without marking uncertainty');
      await capture('delete_rejected');
      await open(501);
      await confirm('确定删除私信？');
      await wait(() => !one('[data-message-peer="501"]') && notice() === '删除成功。',
        'confirmed whole deletion removes just that conversation');
      assert(one('[data-message-peer="502"]') && one('[data-message-peer="505"]'),
        'unrelated conversations stay intact');
      await capture('delete_conversation');
      click(one('[data-message-peer="505"]'));
      await wait(() => ready() && contains('可确认删除的私信') && action('9007199254740997'),
        'thread real identities and data are rendered');
      assert(contains('没有原生消息 ID 的私信') && !action(303),
        'fallback UI identifier never authorizes a deletion');
      await open(301);
      await confirm('确定删除这条私信？');
      await wait(() => !contains('可确认删除的私信') && notice() === '删除成功。',
        'confirmed single deletion removes only that row');
      assert(contains('回执丢失的私信') && contains('没有原生消息 ID 的私信'),
        'single deletion does not remove other rows');
      await capture('delete_single');
      const long = '9007199254740997';
      await open(long);
      await confirm('确定删除这条私信？');
      await wait(() => action(long)?.title === '删除结果待核对' && notice()?.includes('已保留原消息'),
        'lost response leaves visible row marked for readback');
      await capture('delete_uncertain');
      await open(long, 'check');
      await wait(() => notice()?.includes('当前页仍能看到'), 'readback can observe existing message');
      await fixture('deletion-omit');
      await open(long, 'check');
      await wait(() => notice()?.includes('当前页未看到'), 'bounded absence is explicitly inconclusive');
      assert(action(long)?.title === '删除结果待核对' && contains('回执丢失的私信'),
        'bounded absence never clears uncertain row');
      await wait(() => !button('刷新消息')?.disabled, 'refresh enabled after readback');
      click(button('刷新消息'));
      await wait(() => ready() && action(long)?.title === '删除结果待核对',
        'explicit refresh also retains unresolved row during the visit');
      await capture('delete_bounded_absence');
      click(button('关闭消息'));
      await wait(() => !drawer(), 'close clears transient private row content');
      click(trigger());
      await wait(() => ready() && one('[data-message-check-pending]'),
        'native account journal exposes unresolved check after reopening');
      assert(!contains('回执丢失的私信'), 'journal contains no private message body');
      click(one('[data-message-check-pending]'));
      await wait(() => notice()?.includes('当前页未看到'), 'journal readback without original row');
      assert((await writes()).length === 4, 'no unknown readback or reopening replays a mutation');
      await capture('delete_pending_reopen');
      await emit('complete', {
        ok: true, count, privateDeletionVerified: true, realAccountWrites: 0, realMusicRequests: 0,
      });
      return;
    }
    if (window.__cumulusMessageAttachmentsProof) {
      const epoch = account.epoch;
      const target = (id) => one(`[data-message-attachment="${id}"]`);
      const focused = (id) => document.activeElement?.dataset.messageAttachment === String(id);
      const returnToDrawer = async (id) => {
        click(one('.back-button'));
        await wait(() => drawer() && focused(id), 'same attachment restored after direct Back: ' + id);
      };
      const destination = async (label, predicate) => {
        stage = label;
        await wait(() => !drawer() && predicate(), label);
        await emit(`ux_${String(++count).padStart(2, '0')}_${label}`, { label });
        await sleep(170);
      };
      const located = (id) => {
        const target = one(`[data-group="current"] [data-comment-id="${id}"]`);
        const bounds = target?.getBoundingClientRect();
        const viewport = one('.main-content')?.getBoundingClientRect();
        return target && target === document.activeElement && bounds.top >= viewport.top &&
          bounds.bottom <= viewport.bottom;
      };
      click(one('[data-message-peer="505"]'));
      await wait(() => ready() && target(360) && target(359) && target(358), 'known thread attachments');
      assert(!target(357) && !target(356) && !target(355) &&
        !drawer().querySelector('a[href],iframe') && !contains('private.invalid'),
      'private cloud, external URLs and mismatched comment identifiers stay inert');
      await capture('attachment_thread');
      click(target(360));
      await destination('attachment_album', () => one('.catalog-page')?.textContent.includes('测试专辑'));
      await returnToDrawer(360);
      assert((await fixture()).requests.filter(([path]) => path === '/api/msg/private/history').length === 1,
        'direct Back restores same thread without rereading it');
      await capture('attachment_return');
      click(target(358));
      await destination('attachment_album_comment', () => one('.catalog-page') && located(701));
      await returnToDrawer(358);
      await invoke('play_selection', {epoch,selection:{kind:'songs',ids:['1','2'],index:0,quality:'standard'}});
      const baseline = await wait(async () => {
        const view = await invoke('player_snapshot', {epoch});
        return view.state === 'playing' && view.track?.id === '1' && view.position > .1 && view;
      }, 'native baseline queue playing');
      click(target(359));
      await wait(async () => {
        const view = await invoke('player_snapshot',{epoch});
        return drawer() && view.state === 'playing' && view.track?.id === '3' && view.position > .1;
      }, 'message track plays through native engine and leaves drawer open');
      const queue = await invoke('player_queue',{epoch});
      assert(queue.tracks.map((row) => row.track.id).join(',') === '1,3,2' &&
        queue.tracks[0].entryId === baseline.entryId, 'message play inserts and preserves the existing queue');
      await capture('attachment_native_play');
      click(button('返回私信列表'));
      await wait(() => one('[data-message-tab="comments"]'),'private list restored');
      pick('comments');
      await wait(() => ready() && target(411), 'received comment has canonical target');
      click(target(411));
      await destination('attachment_song_comment', () => located(411));
      await returnToDrawer(411);
      pick('notices');
      await wait(() => ready() && target(61) && target(62) && contains('移动端查看的评论'), 'typed notice targets');
      assert(!target(63),'MV like notification remains unsupported as in official NoticeItem');
      click(target(61));
      await destination('attachment_notice_playlist', () => one('.playlist-hero'));
      await returnToDrawer(61);
      click(target(62));
      await destination('attachment_playlist_comment', () => located(702));
      await returnToDrawer(62);
      assert(one('[data-message-tab="notices"] .message-drawer-badge')?.textContent.trim() === '4',
        'viewing typed targets never clears unread');
      assert(!(await fixture()).blockedWrite,'no explicit message mutation');
      await emit('complete',{ok:true,attachmentTargetsVerified:true,count,realAccountWrites:0,realMusicRequests:0});
      return;
    }

    scroller().scrollTop = 170;
    await wait(() => scroller().scrollTop > 0, 'private scroller moved');
    const before = scroller().scrollTop;
    const badge = (selector) => one(selector)?.querySelector('.message-drawer-badge')?.textContent.trim();
    const peerRow = (id) => `[data-message-peer="${id}"]`;
    const privateTab = '[data-message-tab="private"]';
    assert(badge(peerRow(501)) === '1' && badge(peerRow(505)) === '2' && badge(privateTab) === '3',
      'both synthetic peers and the private aggregate are initially unread');
    await fixture('history-hold');
    click(one(peerRow(505)));
    await wait(async () => (await fixture()).historyHeld && button('返回私信列表'), 'first history held');
    click(button('返回私信列表'));
    await wait(() => document.activeElement?.dataset.messagePeer === '505'
      && Math.abs(scroller().scrollTop - before) < 1, 'return before history resolves restores focus and scroll');
    assert((await fixture()).historyHeld && !badge(peerRow(505)) && badge(peerRow(501)) === '1'
      && badge(privateTab) === '3' && dot(), 'only the opened peer clears before history responds');
    await capture('peer_unread_while_history_pending');
    await fixture('history-release');
    await wait(async () => !(await fixture()).historyHeld, 'stale history released');
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
    await wait(async () => await countCalls() === 4, 'closing returns the same poller to normal cadence');
    click(trigger());
    await wait(() => ready() && contains('私信预览 1'), 'new private list session');
    assert(badge(peerRow(505)) === '2' && badge(peerRow(501)) === '1' && badge(privateTab) === '3',
      'a fresh authoritative inbox can show a positive peer count again');
    await fixture('release');
    await wait(async () => !(await fixture()).noticeHeld, 'stale notice reply released');
    pick('notices');
    await wait(() => ready() && contains('合成歌单'), 'fresh notification tab after reopen');
    assert(!contains('旧面板数据不能显示'), 'old drawer response is ignored');
    await capture('reopened');
    await wait(async () => await countCalls() === 5, 'reopening rearmed once');
    const result = await fixture();
    assert(!result.blockedWrite, 'native service observed no clear/send/delete request');
    await emit('complete', { ok: true, count, staleVisitDiscarded: true, peerUnreadVerified: true,
      realAccountWrites: 0, realMusicRequests: 0 });
  } catch (error) {
    await emit('failed', { stageLabel: stage, message: String(error) });
  }
})();
