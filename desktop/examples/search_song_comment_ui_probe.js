// Real dedicated Songs row gestures; production menu, comments page and player IPC.
window.__searchSongCommentRun = async ({ one, all, wait, button, click, key, fresh, choose,
  capture, fixture, emit, sleep, assert, ready }) => {
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const row = (id) => one(`#search-result-panel .track-row[data-track-id="${id}"]`);
  const more = (id) => row(id)?.querySelector('[data-result-action="more"]');
  const menu = () => one('[role="menu"][aria-label="歌曲操作"]:not([inert])');
  const item = (name) => menu()?.querySelector(`[data-menu-key="${name}"]`);
  const menuKeys = () => [...menu().querySelectorAll('[data-menu-key]')].map((node) => node.dataset.menuKey);
  const text = (node) => node?.textContent.trim() ?? null;
  const selected = () => all('#search-result-panel .track-row[aria-selected="true"]').map((r) => r.dataset.trackId);
  const dedicatedReady = () => ready() &&
    one('[data-category="songs"]')?.getAttribute('aria-selected') === 'true' &&
    ['3', '4', '5', '6', '7', '8'].every((id) => !!row(id));
  const live = (node) => node?.isConnected && node.getClientRects().length && !node.closest('[inert]');
  let sequence = 0;
  async function service(action = 'observe') {
    const request = ++sequence;
    await emit('search-song-comment-control', { action, sequence: request });
    await wait(() => window.__searchSongCommentAck?.sequence === request, 'native Search Song comment control');
    return window.__searchSongCommentAck.report;
  }

  await wait(() => one('.track-row'), 'daily ready before Search Song comment proof');
  await fixture('good');
  const { epoch } = await invoke('account_snapshot');
  const player = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  await invoke('play_selection', { epoch, selection: { kind: 'songs', ids: ['91', '92', '3'], index: 0, quality: 'standard' } });
  for (let i = 0; i < 160; i++) {
    const current = await player();
    if (current.track?.id === '91' && current.state === 'playing' && current.position > .1) break;
    assert(i < 159, 'native player decodes the independent baseline queue');
    await sleep(70);
  }
  await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
  const original = await player();
  const identity = (snapshot) => [snapshot.epoch, snapshot.playId, snapshot.entryId, snapshot.queueRevision];
  const queueIdentity = (queue) => [queue.epoch, queue.current, queue.revision,
    queue.tracks.map((r) => [r.entryId, r.track.id])];
  const originalQueue = queueIdentity(await queue());
  assert(original.state === 'paused' && original.track.id === '91' &&
    originalQueue[3].map((r) => r[1]).join(',') === '91,92,3', 'paused queue baseline');
  const playerBadge = () => text(one('.playerbar .player-comment-count'));
  await wait(() => playerBadge() === '17', 'native baseline player comment badge settled');
  const baseline = await service();
  assert(baseline.playerBadgeReads.length === 1 && baseline.counts.length === 0,
    'one baseline badge read is distinct from dedicated Search row counts');
  async function unchanged() {
    const now = await player();
    assert(now.state === 'paused' && now.track?.id === '91' &&
      JSON.stringify(identity(now)) === JSON.stringify(identity(original)) &&
      JSON.stringify(queueIdentity(await queue())) === JSON.stringify(originalQueue),
    'comment count and comment page leave the independent native queue unchanged');
  }
  async function stage(label, evidence = {}) {
    await unchanged();
    assert(playerBadge() === '17', 'Search row count never updates the independent player badge');
    await capture(label, { playerBadge: playerBadge(), ...evidence });
  }
  function selectRow(id, ctrlKey = false) {
    const target = row(id);
    assert(live(target), 'selectable dedicated row ' + id);
    target.dispatchEvent(new PointerEvent('pointerdown', { pointerType: 'mouse', button: 0, buttons: 1, ctrlKey, bubbles: true }));
    target.dispatchEvent(new MouseEvent('click', { button: 0, ctrlKey, bubbles: true }));
  }
  async function gesture(id, via) {
    const anchor = more(id);
    assert(live(anchor), 'visible dedicated More for Song ' + id);
    anchor.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'instant' });
    anchor.focus({ preventScroll: true });
    let previous, since;
    await wait(() => {
      if (more(id) !== anchor || !live(anchor) || document.activeElement !== anchor) return false;
      const box = anchor.getBoundingClientRect();
      const bounds = [box.left, box.top, box.width, box.height].join(',');
      if (bounds !== previous) { previous = bounds; since = performance.now(); return false; }
      return performance.now() - since >= 90 && box.top >= 0 && box.bottom <= innerHeight &&
        anchor.contains(document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2));
    }, 'stable hittable dedicated More for Song ' + id);
    if (via === 'keyboard') {
      assert(!anchor.dispatchEvent(new KeyboardEvent('keydown', { key: 'F10', shiftKey: true, bubbles: true, cancelable: true })),
        'dedicated row handles Shift+F10');
    } else if (via === 'context') {
      const target = row(id), box = target.getBoundingClientRect();
      assert(!target.dispatchEvent(new MouseEvent('contextmenu', { clientX: box.left + 20, clientY: box.top + 12,
        button: 2, bubbles: true, cancelable: true })), 'dedicated row handles context click');
    } else click(anchor);
    return anchor;
  }
  async function open(id, via, label) {
    const anchor = await gesture(id, via);
    await wait(() => live(menu()) && menu().style.visibility === 'visible' &&
      anchor.getAttribute('aria-expanded') === 'true', 'exact dedicated Song action menu ' + id);
    const keys = menuKeys();
    assert(text(item('comments')) === label && keys[0] === 'play' && keys[1] === 'next' &&
      (!label || keys[2] === 'comments'), 'expected comment label and ordering for Song ' + id + ': ' + text(item('comments')));
    return anchor;
  }
  async function dismiss(anchor) {
    key(menu(), 'Escape');
    await wait(() => !menu() && document.activeElement === anchor &&
      anchor.getAttribute('aria-expanded') === 'false', 'Escape restores exact dedicated More focus');
  }
  async function fallback(id, via, label, name) {
    const anchor = await open(id, via, label);
    await stage(name, { clicked: id, via, comment: text(item('comments')), menuKeys: menuKeys() });
    await dismiss(anchor);
  }

  await fresh('合成评论菜单');
  await choose('songs');
  await wait(dedicatedReady, 'initial dedicated search visit');
  assert((await service()).counts.length === 0 && !menu(), 'browsing dedicated Songs does not read comments');
  await stage('search_comment_initial', { countCalls: 0 });

  const exact = await open('3', 'keyboard', '查看评论(36)');
  await stage('search_comment_count', { clicked: '3', via: 'keyboard', comment: text(item('comments')), menuKeys: menuKeys() });
  click(item('comments'));
  await wait(() => {
    const section = one('.comments-page:not(.comments-embedded)');
    const header = section?.querySelector('.comment-resource-header');
    return !menu() && live(header) && header.dataset.resourceKind === 'track' && header.dataset.resourceId === '3' &&
      section.getAttribute('aria-busy') === 'false' && text(section).includes('合成搜索点中单曲评论');
  }, 'actual track comments page for the exact clicked Search Song 3');
  assert(!exact.isConnected || exact.getAttribute('aria-expanded') === 'false', 'navigation closed the original row menu');
  const header = one('.comments-page .comment-resource-header');
  await stage('search_comment_thread', { clicked: '3', resourceKind: header.dataset.resourceKind, resourceId: header.dataset.resourceId });
  click(button('返回上一页'));
  await wait(() => dedicatedReady() && !one('.page-enter-active,.page-leave-active'), 'Back returns to dedicated Songs');
  assert(!menu(), 'Back does not reopen comment menu');

  await fallback('4', 'button', '查看评论', 'search_comment_zero');
  await fallback('5', 'context', '查看评论', 'search_comment_wrong_thread');
  await fallback('6', 'keyboard', '查看评论', 'search_comment_transport');
  await fallback('7', 'button', null, 'search_comment_type_hidden');
  selectRow('3');
  selectRow('4', true);
  await wait(() => selected().join(',') === '3,4', 'two dedicated rows selected');
  const group = await open('3', 'button', null);
  await stage('search_comment_multi_hidden', {
    clicked: '3', selectedIds: selected(), comment: text(item('comments')), menuKeys: menuKeys(),
  });
  await dismiss(group);
  selectRow('8');
  await wait(() => selected().join(',') === '8', 'select only Song 8 before late count');

  const old = await gesture('8', 'button');
  const dispatchDeadline = performance.now() + 850;
  let pending;
  do {
    pending = await service();
    if (pending.heldStarted) break;
    await sleep(30);
  } while (performance.now() < dispatchDeadline);
  assert(pending.heldStarted && !pending.heldReleased && !pending.heldReturned &&
    !menu() && old.getAttribute('aria-expanded') !== 'true', 'menu remains closed while native count is held');
  click(one('[data-category="all"]'));
  await service('release');
  await wait(() => ready() && one('[data-category="all"]')?.getAttribute('aria-selected') === 'true' &&
    !one('.page-leave-active,.page-enter-active'), 'leave the dedicated visit while the count resolves');
  const late = await service();
  assert(late.heldReturned && !menu() && !old.isConnected, 'stale count returned without opening the departed menu');
  await stage('search_comment_stale', { clicked: '8', oldMenuSuppressed: true, heldReturned: late.heldReturned });

  await fresh('合成评论菜单再查');
  await choose('songs');
  await wait(dedicatedReady, 'new dedicated search visit');
  assert(!menu(), 'fresh visit starts without any previous popup');
  const reopened = await open('8', 'button', '查看评论(51)');
  assert(reopened !== old && !old.isConnected, 'same Song in new search has its own More anchor');
  await stage('search_comment_reopened', { clicked: '8', comment: text(item('comments')), newAnchor: true });
  await dismiss(reopened);
  await unchanged();
  return { focusRestored: document.activeElement === reopened };
};
