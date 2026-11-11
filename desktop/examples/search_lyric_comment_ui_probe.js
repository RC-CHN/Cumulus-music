// Actual Lyric result row gestures, Vue menu and native comments/queue IPC.
window.__searchLyricCommentRun = async ({
  one, wait, button, click, key, fresh, choose, capture, fixture, emit, sleep, assert, ready,
}) => {
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const row = (index) => one(`#search-result-panel .search-lyric-result[data-lyric-index="${index}"]`);
  const more = (index) => row(index)?.querySelector('[data-result-action="more"]');
  const menu = () => one('[role="menu"][aria-label="歌曲操作"]:not([inert])');
  const comment = () => menu()?.querySelector('[data-menu-key="comments"]');
  const badge = () => one('.playerbar .player-comment-count')?.textContent.trim();
  const active = (tab) => one(`[data-category="${tab}"]`)?.getAttribute('aria-selected') === 'true' && ready();
  const live = (node) => node?.isConnected && node.getClientRects().length && !node.closest('[inert]');
  const hit = (index) => ({
    index,
    id: row(index)?.querySelector('.track-row')?.dataset.trackId,
    rowKey: row(index)?.dataset.resultKey,
    excerpt: row(index)?.querySelector('.search-lyric-lines')?.textContent.trim(),
  });
  let sequence = 0;
  async function service(action = 'observe') {
    const request = ++sequence;
    await emit('search-lyric-comment-control', { action, sequence: request });
    await wait(() => window.__searchLyricCommentAck?.sequence === request, 'native Lyrics comment control');
    return window.__searchLyricCommentAck.report;
  }

  await wait(() => one('.track-row'), 'daily ready');
  await fixture('good');
  const { epoch } = await invoke('account_snapshot');
  // Observe actual typed adapter arguments/replies, forwarding both unchanged.
  // The final evidence never persists the opaque receipts themselves.
  const provides = one('#app').__vue_app__._context.provides;
  const core = provides[Reflect.ownKeys(provides).find((k) =>
    typeof k === 'symbol' && k.description === 'core-client')];
  assert(typeof core?.searchResults === 'function' && typeof core.searchSongRowCommentCount === 'function',
    'real typed search adapter available');
  let projected = [];
  const requested = [];
  const searchResults = core.searchResults.bind(core);
  core.searchResults = async (...args) => {
    const result = await searchResults(...args);
    if (args[1].category === 'lyrics' && args[1].offset === 0 && result.epoch === epoch) {
      projected = result.value.sections.find((section) => section.category === 'lyrics')?.items ?? [];
    }
    return result;
  };
  const readCount = core.searchSongRowCommentCount.bind(core);
  core.searchSongRowCommentCount = (...args) => {
    requested.push(args);
    return readCount(...args);
  };
  function exactReceipt(before, info) {
    if (info?.kind === 'hidden') return requested.length === before;
    return info?.kind === 'eligible' && requested.length === before + 1 &&
      requested[before].length === 2 && requested[before][0] === epoch && requested[before][1] === info.receipt;
  }
  const player = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  await invoke('play_selection', { epoch, selection: { kind: 'songs', ids: ['91', '92', '3'], index: 0, quality: 'standard' } });
  for (let i = 0; i < 160; i++) {
    const state = await player();
    if (state.state === 'playing' && state.track?.id === '91' && state.position > .1) break;
    assert(i < 159, 'native paused queue baseline decoded');
    await sleep(70);
  }
  await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
  const identity = (s) => [s.epoch, s.state, s.playId, s.entryId, s.queueRevision, s.track?.id];
  const ordering = (q) => [q.epoch, q.current, q.revision, q.tracks.map((r) => [r.entryId, r.track.id])];
  const original = identity(await player()), originalQueue = ordering(await queue());
  assert(original[1] === 'paused' && original[5] === '91' &&
    originalQueue[3].map((r) => r[1]).join(',') === '91,92,3', 'exact paused queue baseline');
  await wait(() => badge() === '17', 'settled independent native player comment badge');
  assert((await service()).playerBadgeReads.length === 1, 'one native baseline badge request');
  async function stage(name, details = {}) {
    assert(JSON.stringify(identity(await player())) === JSON.stringify(original) &&
      JSON.stringify(ordering(await queue())) === JSON.stringify(originalQueue) &&
      badge() === '17', 'Lyrics actions preserve paused native playback and its badge');
    await capture(name, { playerBadge: badge(), ...details });
  }

  async function gesture(index, via) {
    if (!row(index)) {
      const panel = one('#search-result-panel');
      panel.scrollTop = index < 2 ? 0 : panel.scrollHeight;
    }
    await wait(() => live(more(index)), 'live exact Lyric result More ' + index);
    const anchor = more(index);
    anchor.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'instant' });
    anchor.focus({ preventScroll: true });
    let previous, since;
    await wait(() => {
      if (anchor !== more(index) || !live(anchor) || document.activeElement !== anchor) return false;
      const rect = anchor.getBoundingClientRect();
      const bounds = [rect.left, rect.top, rect.width, rect.height].join(',');
      if (bounds !== previous) { previous = bounds; since = performance.now(); return false; }
      return performance.now() - since >= 90 && rect.top >= 0 && rect.bottom <= innerHeight &&
        anchor.contains(document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2));
    }, 'stable hittable Lyric More ' + index);
    if (via === 'keyboard') {
      assert(!anchor.dispatchEvent(new KeyboardEvent('keydown', { key: 'F10', shiftKey: true, bubbles: true, cancelable: true })),
        'Lyric row handles Shift+F10');
    } else if (via === 'context') {
      const target = row(index).querySelector('.track-row'), box = target.getBoundingClientRect();
      assert(!target.dispatchEvent(new MouseEvent('contextmenu', { clientX: box.left + 20, clientY: box.top + 12,
        button: 2, bubbles: true, cancelable: true })), 'Lyric row handles context click');
    } else click(anchor);
    return anchor;
  }
  async function open(index, via, label) {
    const observed = projected[index], before = requested.length;
    const anchor = await gesture(index, via);
    assert(observed?.kind === 'song' && observed.track.id === hit(index).id,
      'native projected exact Lyric ordinal ' + index);
    await wait(() => live(menu()) && menu().style.visibility === 'visible' &&
      anchor.getAttribute('aria-expanded') === 'true', 'menu bound to exact Lyric More ' + index);
    const keys = [...menu().querySelectorAll('[data-menu-key]')].map((node) => node.dataset.menuKey);
    const actual = comment()?.textContent.trim() ?? null;
    assert(actual === label && keys[0] === 'play' && keys[1] === 'next' && (!label || keys[2] === 'comments'),
      'Lyric row ' + index + ' expected comment ' + label + ', got ' + actual);
    const receiptMatched = exactReceipt(before, observed.songCommentMenu);
    assert(receiptMatched && observed.songCommentMenu.kind === (label === null ? 'hidden' : 'eligible'),
      'exact clicked Lyric row receipt or explicit hidden marker used');
    return { anchor, ...hit(index), via, label: actual, keys, receiptMatched };
  }
  async function dismiss(anchor) {
    key(menu(), 'Escape');
    await wait(() => !menu() && document.activeElement === anchor &&
      anchor.getAttribute('aria-expanded') === 'false', 'Escape restores exact Lyric hit focus');
  }
  async function menuStage(name, opened) {
    const { anchor, ...evidence } = opened;
    await stage(name, { action: evidence });
  }

  await fresh('合成歌词评论');
  await choose('lyrics');
  await wait(() => active('lyrics') && row(0) && row(1), 'first two Lyric result occurrences');
  const first = hit(0), second = hit(1);
  assert(first.id === second.id && first.id === '3' && first.rowKey === 'lyric:0:3' &&
    second.rowKey === 'lyric:1:3' && first.excerpt !== second.excerpt, 'duplicate Song results keep distinct exact lyric rows');
  const duplicateReceiptsDistinct = projected[0]?.songCommentMenu?.kind === 'eligible' &&
    projected[1]?.songCommentMenu?.kind === 'eligible' &&
    projected[0].songCommentMenu.receipt !== projected[1].songCommentMenu.receipt;
  assert(duplicateReceiptsDistinct && !requested.length && (await service()).counts.length === 0 && !menu(),
    'native duplicate row receipts differ and rendering does not read them');
  await stage('lyric_comment_initial', { first, second, duplicateReceiptsDistinct });

  const countA = await open(0, 'keyboard', '查看评论(36)');
  await menuStage('lyric_comment_first', countA);
  await dismiss(countA.anchor);
  const countB = await open(1, 'button', '查看评论(42)');
  assert(countB.anchor !== countA.anchor && countA.anchor.getAttribute('aria-expanded') === 'false',
    'same-ID second result opens its own anchor and fresh count');
  await menuStage('lyric_comment_duplicate', countB);
  click(comment());
  await wait(() => {
    const page = one('.comments-page:not(.comments-embedded)');
    const header = page?.querySelector('.comment-resource-header');
    return !menu() && live(header) && header.dataset.resourceKind === 'track' && header.dataset.resourceId === '3' &&
      page.getAttribute('aria-busy') === 'false' && page.textContent.includes('合成歌词点中歌曲评论');
  }, 'same-Song duplicate menu opens exact track comments');
  const header = one('.comments-page .comment-resource-header');
  await stage('lyric_comment_thread', { clickedIndex: 1, resource: [header.dataset.resourceKind, header.dataset.resourceId] });
  click(button('返回上一页'));
  await wait(() => active('lyrics') && !one('.page-leave-active,.page-enter-active') && row(0) && row(1), 'Back to dedicated Lyrics');
  assert(!menu(), 'Back never reopens prior lyric menu');

  for (const [index, via, label, name] of [
    [2, 'context', '查看评论', 'lyric_comment_zero'],
    [3, 'keyboard', '查看评论', 'lyric_comment_transport'],
    [4, 'button', null, 'lyric_comment_type_hidden'],
  ]) {
    const opened = await open(index, via, label);
    await menuStage(name, opened);
    await dismiss(opened.anchor);
  }

  const heldReceipt = projected[5]?.songCommentMenu, beforeHeld = requested.length;
  const old = await gesture(5, 'button');
  const dispatchDeadline = performance.now() + 850;
  let pending;
  do {
    pending = await service();
    if (pending.held) break;
    await sleep(30);
  } while (performance.now() < dispatchDeadline);
  assert(exactReceipt(beforeHeld, heldReceipt) && pending.held && !pending.released &&
    !pending.returned && !menu(), 'exact native count held while menu still closed');
  click(one('[data-category="all"]'));
  await service('release');
  await wait(() => active('all') && !one('.page-enter-active,.page-leave-active'), 'left the held lyric visit');
  assert((await service()).returned && !menu() && !old.isConnected, 'held native reply cannot open a departed lyric menu');
  await stage('lyric_comment_stale', {
    clickedIndex: 5, heldReturned: true, oldMenuSuppressed: true, receiptMatched: true,
  });

  await fresh('合成歌词评论再查');
  await choose('lyrics');
  await wait(() => active('lyrics') && row(0), 'new lyric search visit');
  const reopened = await open(5, 'button', '查看评论(51)');
  assert(reopened.anchor !== old && !old.isConnected, 'new exact anchor owns the reopened lyric menu');
  await menuStage('lyric_comment_reopened', reopened);
  await dismiss(reopened.anchor);
  return { focusRestored: document.activeElement === reopened.anchor };
};
