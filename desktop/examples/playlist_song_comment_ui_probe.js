// Ordinary playlist Song menu uses real Vue, Tauri and a sealed native transport.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])];
  const text = (node) => node?.textContent.trim();
  const row = (id) => one(`.track-row[data-track-id="${id}"]`);
  const more = (id) => one('[data-result-action="more"]', row(id));
  const menu = () => one('[role="menu"][aria-label="歌曲操作"]:not([inert])');
  const item = (key) => one(`[data-menu-key="${key}"]`, menu() ?? null);
  const back = () => one('button[aria-label="返回上一页"]');
  const songTab = () => one('.playlist-content-nav button[aria-pressed="true"]');
  const commentsTab = () => one('button[aria-label="歌单评论"]');
  const durationSort = () => one('button[aria-label="按时长排序"]');
  const trackScroll = () => one('.track-table .track-scroll');
  const loadMore = () => one('button.load-more');
  const sidebarPlaylist = (id) => one(`[data-sidebar-id="${id}"]`);
  const playlistCard = () => one('.playlist-library-page [data-library-open="2"]');
  const sleeps = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const frames = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const ready = (node) => node?.isConnected && node.getClientRects().length && !node.closest('[inert]') &&
    !node.matches(':disabled,[aria-disabled="true"]');
  const assert = (condition, message) => { if (!condition) throw Error(message); };
  const emit = (stage, payload = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...payload },
  });
  const keyboard = (node, key, extra = {}) => node.dispatchEvent(new KeyboardEvent('keydown', {
    bubbles: true, cancelable: true, key, ...extra,
  }));
  let step = 'startup', sequence = 0, lastAnchor;
  const stages = [];
  async function wait(observe, label) {
    step = label;
    const deadline = performance.now() + 16_000;
    do {
      const value = await observe();
      if (value) return value;
      await sleeps(55);
    } while (performance.now() < deadline);
    throw Error(label);
  }
  async function fixture(action = 'observe') {
    const expected = ++sequence;
    await emit('playlist-song-comment-control', { action, sequence: expected });
    const ack = await wait(() => window.__playlistSongCommentAck?.sequence === expected &&
      window.__playlistSongCommentAck, 'native playlist Song comment fixture receipt');
    return ack.report;
  }
  function click(node) {
    assert(ready(node), 'enabled input for ' + step);
    node.focus({ preventScroll: true });
    node.click();
  }
  async function capture(name, data = {}) {
    await frames();
    stages.push(name);
    await emit(`ux_${String(stages.length).padStart(2, '0')}_${name}`, {
      ...data, captureHandshake: true, selectedTrack: one('.track-row[aria-selected="true"]')?.dataset.trackId,
    });
  }
  function library() {
    return wait(() => ready(playlistCard()) && !one('.page-enter-active,.page-leave-active') &&
      playlistCard(), 'live playlist library');
  }
  async function playlist() {
    await wait(() => ready(one('.playlist-hero')) && ready(commentsTab()) &&
      commentsTab().getAttribute('aria-pressed') === 'false' && text(songTab())?.startsWith('歌曲') &&
      !one('.page-enter-active,.page-leave-active') && ['3','4','5','6','7','8'].every((id) => row(id)),
    'ordinary playlist 2 Songs rows loaded');
  }
  async function gesture(id, via) {
    const anchor = lastAnchor = more(id);
    assert(anchor?.isConnected, 'exact row More exists: ' + id);
    anchor.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'instant' });
    anchor.focus({ preventScroll: true });
    await frames();
    let previous, stableSince;
    await wait(() => {
      if (more(id) !== anchor || !ready(anchor) || document.activeElement !== anchor) return false;
      const b = anchor.getBoundingClientRect();
      const next = [b.left,b.top,b.width,b.height].join(',');
      if (previous !== next) { previous = next; stableSince = performance.now(); return false; }
      return performance.now() - stableSince >= 90 && b.top >= 0 && b.bottom <= innerHeight;
    }, 'stable More anchor for Song ' + id);
    if (via === 'keyboard') {
      assert(!keyboard(anchor, 'F10', { shiftKey: true }), 'row handles Shift+F10');
    } else if (via === 'context') {
      const target = row(id), bounds = target.getBoundingClientRect();
      assert(!target.dispatchEvent(new MouseEvent('contextmenu', {
        bubbles: true, cancelable: true, button: 2, clientX: bounds.left + 15, clientY: bounds.top + 10,
      })), 'row handles context menu');
    } else click(anchor);
    return anchor;
  }
  async function open(id, via, label) {
    const anchor = await gesture(id, via);
    await wait(() => menu()?.style.visibility === 'visible' && ready(menu()) &&
      anchor.getAttribute('aria-expanded') === 'true', 'settled menu for Song ' + id);
    const rows = all('button[data-menu-key]', menu());
    const actual = text(item('comments')) ?? null;
    assert(actual === label, 'comment label for Song ' + id + ': ' + actual);
    assert(!label || ready(item('comments')) && rows[2] === item('comments'), 'Comments follows Play and Next');
    assert(item('play') && item('next'), 'other row actions are still present');
    return anchor;
  }
  async function escape(anchor) {
    keyboard(menu(), 'Escape');
    await wait(() => !menu() && document.activeElement === anchor &&
      anchor.getAttribute('aria-expanded') === 'false', 'Escape returns focus to the clicked Song');
  }
  async function fallback(id, via, label, name) {
    const anchor = await open(id, via, label);
    await capture(name, { clicked: id, via, comment: label, menuKeys: all('[data-menu-key]', menu()).map((n) => n.dataset.menuKey) });
    await escape(anchor);
  }
  async function cycleDuration(order, expectedIndex) {
    const sort = await wait(() => ready(durationSort()) && durationSort(), 'live playlist duration sort');
    sort.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'instant' });
    click(sort);
    await wait(() => sort.closest('[role="columnheader"]')?.getAttribute('aria-sort') === order &&
      (expectedIndex == null || row('9')?.getAttribute('aria-rowindex') === expectedIndex),
    'playlist duration ' + order + ' with expected Song 9 row');
  }
  async function scrollToSong(id) {
    const scroller = await wait(() => trackScroll()?.scrollHeight > trackScroll()?.clientHeight && trackScroll(),
      'real playlist table has a scrollable track viewport');
    const before = scroller.scrollTop;
    scroller.scrollTop = scroller.scrollHeight;
    await wait(() => trackScroll() === scroller && scroller.scrollTop > 0 && row(id),
      'real track scroll reaches Song ' + id);
    return { before, after: scroller.scrollTop };
  }
  async function retainedSuffix() {
    assert(!row('101') && text(loadMore())?.includes('100 / 105'), 'only first page is present');
    const movement = await scrollToSong('100');
    assert(movement.after > movement.before, 'user scroll reached the end of the initial track page');
    const next = await wait(() => ready(loadMore()) && text(loadMore())?.includes('100 / 105') && loadMore(),
      'live 100 / 105 Load more control');
    next.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'instant' });
    await frames();
    click(next);
    await wait(() => !loadMore() && text(songTab())?.includes('105'), 'suffix loaded through the playlist table');
    await scrollToSong('101');
    const suffix = await open('101', 'button', '查看评论(70)');
    await capture('loaded_suffix', { clicked: '101', comment: text(item('comments')),
      ordinal: row('101').getAttribute('aria-rowindex'), tableScroll: movement });
    await escape(suffix);

    const created = await wait(
      () => one('[data-sidebar-group="created"] .sidebar-group-toggle'), 'created sidebar group');
    if (created.getAttribute('aria-expanded') === 'false') click(created);
    created.closest('header').scrollIntoView({ block: 'start', behavior: 'instant' });
    const other = await wait(() => ready(sidebarPlaylist('3')) && sidebarPlaylist('3'), 'real sidebar playlist 3');
    other.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'instant' });
    click(other);
    await wait(() => other.getAttribute('aria-current') === 'page' && text(songTab())?.includes('40') &&
      !row('101') && !one('.page-enter-active,.page-leave-active'), 'switched from suffix to ordinary playlist 3');
    await capture('away_from_suffix', { playlistId: '3' });
    click(back());
    await wait(() => sidebarPlaylist('2')?.getAttribute('aria-current') === 'page' &&
      text(songTab())?.includes('105') && !loadMore() && !one('.page-enter-active,.page-leave-active'),
    'Back restores playlist 2 with its loaded suffix');
    await scrollToSong('101');
    const retained = await open('101', 'keyboard', '查看评论(73)');
    assert(retained !== suffix && !suffix.isConnected, 'retained row is projected into the Back visit');
    await capture('retained_suffix_count', { clicked: '101', comment: text(item('comments')),
      ordinal: row('101').getAttribute('aria-rowindex'), newAnchor: true });
    await escape(retained);
    trackScroll().scrollTop = 0;
  }
  try {
    click(await wait(() => all('.sidebar button').find((n) => ready(n) && text(n) === '我的歌单'), 'library navigation'));
    click(await library());
    await playlist();
    assert(!(await fixture()).counts.length && !menu(), 'playlist load does not read a row count');
    await capture('initial', { playlistId: '2', countCalls: 0 });

    const exact = await open('3', 'keyboard', '查看评论(36)');
    await capture('exact_count', { clicked: '3', via: 'keyboard', comment: text(item('comments')) });
    click(item('comments'));
    const header = await wait(() => {
      const section = one('.comments-page:not(.comments-embedded)');
      const resource = one('.comment-resource-header', section ?? null);
      return !menu() && ready(resource) && resource.dataset.resourceKind === 'track' &&
        resource.dataset.resourceId === '3' && section.getAttribute('aria-busy') === 'false' &&
        text(section).includes('合成歌单点中歌曲评论') && resource;
    }, 'actual comments page for clicked Song 3');
    assert(!exact.closest('[aria-haspopup="menu"]') || exact.getAttribute('aria-expanded') === 'false', 'menu closed on comment navigation');
    await capture('exact_thread', { clicked: '3', resourceKind: header.dataset.resourceKind, resourceId: header.dataset.resourceId });
    click(back());
    await playlist();
    assert(!menu(), 'return to playlist does not reopen comment menu');

    await fallback('4', 'button', '查看评论', 'zero_plain');
    await fallback('5', 'context', '查看评论', 'wrong_thread_plain');
    await fallback('6', 'keyboard', '查看评论', 'transport_plain');
    await fallback('7', 'button', null, 'private_type_hidden');

    const old = await gesture('8', 'button');
    await wait(async () => (await fixture()).heldStarted, 'first Song 8 count held before popup');
    assert(!menu() && old.getAttribute('aria-expanded') !== 'true', 'menu waits for a matching reply');
    click(back());
    await fixture('release');
    await library();
    const late = await fixture();
    assert(late.heldReturned && !menu(), 'late 88 count returned but did not open the departed menu');
    await capture('left_while_held', { clicked: '8', oldMenuSuppressed: true, heldReturned: late.heldReturned });
    click(await library());
    await playlist();
    assert(!menu(), 'fresh playlist visit has no old menu');
    const fresh = await open('8', 'button', '查看评论(51)');
    assert(fresh !== old && !old.isConnected, 'reopen is a new exact row anchor');
    await capture('reopened_count', { clicked: '8', comment: text(item('comments')), newAnchor: true });
    await escape(fresh);
    const focusRestored = document.activeElement === fresh;

    await retainedSuffix();
    await wait(() => row('9')?.getAttribute('aria-rowindex') === '10', 'Song 9 original ordinal');
    await cycleDuration('ascending', '2');
    const sorted = await open('9', 'keyboard', '查看评论(64)');
    await capture('sorted_count', { clicked: '9', comment: text(item('comments')), sort: 'ascending',
      rowIndex: row('9').getAttribute('aria-rowindex') });
    await escape(sorted);
    await cycleDuration('descending');
    await cycleDuration('none', '10');
    const original = await open('9', 'button', '查看评论(64)');
    await capture('restored_order_count', { clicked: '9', comment: text(item('comments')), sort: 'none',
      rowIndex: row('9').getAttribute('aria-rowindex') });
    await escape(original);
    await emit('complete', { ok: true, stages, focusRestored: focusRestored && document.activeElement === original,
      realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', { ok: false, stages, step, error: String(error),
      focused: document.activeElement?.getAttribute('aria-label'),
      menu: menu()?.textContent, anchor: lastAnchor?.outerHTML, rows: all('.track-row[data-track-id]').map((n) => n.dataset.trackId),
      heading: text(one('.page-heading h1, .playlist-hero h1')) });
  }
})();
