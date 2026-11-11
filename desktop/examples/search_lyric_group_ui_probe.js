// Production WebView gestures, native playback/queue and sealed lyric results.
window.__searchLyricGroupRun = async ({ one, all, wait, button, click, key, fresh, choose, capture, fixture, sleep, assert, ready }) => {
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const panel = () => one('#search-result-panel');
  const row = (i) => panel()?.querySelector(`[data-lyric-index="${i}"] .track-row`);
  const more = (i) => row(i)?.querySelector('[data-result-action="more"]');
  const active = () => one('[data-category="lyrics"]')?.getAttribute('aria-selected') === 'true' && ready();
  const selected = () => all('#search-result-panel .track-row[aria-selected="true"]')
    .map((node) => Number(node.closest('[data-lyric-index]').dataset.lyricIndex));
  const menu = () => one('[role="menu"][aria-label="歌曲操作"]:not([inert])');
  const item = (name) => menu()?.querySelector(`[data-menu-key="${name}"]`);
  const live = (node) => node?.isConnected && !!node.getClientRects().length && !node.closest('[inert]');

  const rowsByReceipt = new Map(), operations = [];
  const fetchNative = window.fetch;
  window.fetch = async function (url, options) {
    const route = String(url);
    const args = (route.endsWith('/player_action') || route.endsWith('/search_results')) &&
      typeof options?.body === 'string' ? JSON.parse(options.body) : null;
    if (route.endsWith('/player_action') && args?.action?.kind === 'search_lyric_group_playback') {
      const action = args.action;
      assert(Object.keys(action).sort().join(',') ===
        'anchor,expected_play_id,expected_queue_revision,kind,operation,quality,rows',
      'frontend sends only the typed lyric operation, receipts and queue fence');
      const anchor = rowsByReceipt.get(action.anchor);
      const rows = action.rows.map((receipt) => rowsByReceipt.get(receipt));
      assert(anchor && rows.every(Boolean) && action.rows.includes(action.anchor),
        'all submitted receipts came from exact rendered Lyrics results');
      operations.push({ operation: action.operation, anchor, rows,
        queueRevision: action.expected_queue_revision, hasPlayId: typeof action.expected_play_id === 'string' });
    }
    const response = await fetchNative.call(window, url, options);
    if (route.endsWith('/search_results') && args?.query?.category === 'lyrics' && response.ok) {
      const reply = await response.clone().json();
      for (const section of reply.value?.sections ?? []) {
        section.items.forEach((entry, i) => {
          if (entry.lyricGroupPlayback?.receipt) rowsByReceipt.set(entry.lyricGroupPlayback.receipt,
            { id: entry.track.id, ordinal: args.query.offset + i, right: entry.lyricGroupPlayback.right });
        });
      }
    }
    return response;
  };
  await wait(() => one('.track-row'), 'daily ready');
  await fixture('good');
  const { epoch } = await invoke('account_snapshot');
  const player = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  const ids = (snapshot) => snapshot.tracks.map((entry) => entry.track.id);
  const current = (snapshot) => [snapshot.epoch, snapshot.playId, snapshot.entryId, snapshot.track?.id];
  await invoke('play_selection', { epoch, selection: { kind: 'songs', ids: ['91', '92'], index: 0, quality: 'standard' } });
  for (let i = 0; i < 160; i++) {
    const state = await player();
    if (state.state === 'playing' && state.track?.id === '91' && state.position > .1) break;
    assert(i < 159, 'native baseline queue decoded');
    await sleep(70);
  }
  await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
  const paused = await player();
  const pausedIdentity = current(paused);
  assert(paused.state === 'paused' && ids(await queue()).join(',') === '91,92', 'paused native queue baseline');
  const stage = async (name, details = {}) => capture(name, { selectedOrdinals: selected(), operations: [...operations], ...details });

  function send(target, type, flags, mouse) {
    const E = type.startsWith('pointer') ? PointerEvent : MouseEvent;
    return target.dispatchEvent(new E(type, {
      ...flags, ...mouse, pointerType: 'mouse', bubbles: true, cancelable: true,
    }));
  }
  function down(target, flags = {}, button = 0) {
    send(target, 'pointerdown', flags, { button, buttons: button === 2 ? 2 : 1 });
    send(target, 'mousedown', flags, { button, buttons: button === 2 ? 2 : 1 });
  }
  function up(target, flags = {}, button = 0) {
    send(target, 'pointerup', flags, { button, buttons: 0 });
    send(target, 'mouseup', flags, { button, buttons: 0 });
  }
  function select(i, flags = {}) {
    const target = row(i);
    assert(live(target), 'mounted lyric selection row ' + i);
    down(target, flags);
    up(target, flags);
    send(target, 'click', flags, { button: 0 });
  }
  async function open(i, flags = {}) {
    const anchor = more(i), target = row(i);
    assert(live(anchor), 'mounted lyric menu row ' + i);
    anchor.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'instant' });
    anchor.focus({ preventScroll: true });
    let previous, since;
    await wait(() => {
      const box = anchor.getBoundingClientRect();
      if (!live(anchor) || anchor !== more(i) || document.activeElement !== anchor) return false;
      const bounds = [box.left, box.top, box.width, box.height, panel().scrollTop].join(',');
      if (bounds !== previous) { previous = bounds; since = performance.now(); return false; }
      return performance.now() - since >= 100 && anchor.contains(document.elementFromPoint(
        box.left + box.width / 2, box.top + box.height / 2));
    }, 'exact lyric menu anchor hittable after virtual scroll settles');
    const box = target.getBoundingClientRect();
    down(target, flags, 2);
    send(target, 'contextmenu', flags, { button: 2, clientX: box.left + 22, clientY: box.top + 12 });
    up(target, flags, 2);
    await wait(() => live(menu()) && menu().style.visibility === 'visible' && anchor.getAttribute('aria-expanded') === 'true',
      'visible exact lyric context menu ' + i);
    return anchor;
  }
  async function dismiss(anchor) {
    key(menu(), 'Escape');
    await wait(() => !menu() && document.activeElement === anchor, 'exact lyric action focus restored');
  }
  async function stable(check, label) {
    for (let i = 0; i < 150; i++) {
      if (check(await player(), await queue())) return;
      await sleep(65);
    }
    assert(false, label);
  }
  const samePause = (state) => state.state === 'paused' &&
    JSON.stringify(current(state)) === JSON.stringify(pausedIdentity);

  await fresh('合成歌词组选曲');
  await choose('lyrics');
  await wait(() => active() && row(0) && row(3), 'first lyric group rows loaded');
  await stage('lyric_group_initial');
  select(2, { ctrlKey: true });
  select(1, { ctrlKey: true });
  select(0, { ctrlKey: true });
  await wait(() => selected().join(',') === '0,1,2,3', 'coherent duplicate hits reflect same selection');
  const outsideGroup = await open(4, { ctrlKey: true });
  assert(item('play')?.disabled && item('next')?.disabled && !item('comments') &&
    selected().join(',') === '0,1,2,3', 'modified outsider keeps the group but cannot dispatch outside it');
  await stage('lyric_group_selected', { modifiedOutsiderGroupDisabled: true });
  await dismiss(outsideGroup);

  await open(1);
  assert(!item('comments') && item('play') && item('next'), 'group hides single-row comment');
  await wait(() => item('next') && !item('next').disabled, 'mixed-rights selection enables group Next');
  click(item('next'));
  await wait(() => !menu() && one('.playback-message[role="status"]')?.textContent.includes(
    '这首歌当前无法播放，请从同组中可播放的歌曲打开菜单'), 'blocked anchor receives the exact native error');
  assert(samePause(await player()) && ids(await queue()).join(',') === '91,92', 'blocked clicked track leaves paused queue untouched');
  await stage('lyric_group_blocked');

  await open(3);
  assert(!item('comments') && item('next') && !item('next').disabled, 'duplicate hit opens real group Next');
  click(item('next'));
  await stable((state, list) => samePause(state) && ids(list).join(',') === '91,103,101,92', 'Next inserts in gesture order while paused');
  await stage('lyric_group_next');

  await open(3);
  assert(!item('comments') && item('play') && !item('play').disabled, 'same duplicate hit opens real group Play');
  click(item('play'));
  await stable((state, list) => state.state === 'playing' && state.track?.id === '101' &&
    state.position > .1 && ids(list).join(',') === '91,103,101,92', 'Play starts exact clicked duplicate Song');
  await stage('lyric_group_play');

  panel().scrollTop = panel().scrollHeight;
  await wait(() => row(11), 'continuation mounted while prior selected rows virtualize');
  await stage('lyric_group_continuation');
  panel().scrollTop = 0;
  await wait(() => row(0) && selected().join(',') === '0,1,2,3', 'original ordered group survives virtual scroll and appended page');
  await stage('lyric_group_restored');
  const outsider = await open(4);
  assert(item('comments')?.textContent.trim() === '查看评论(3)' &&
    selected().join(',') === '4', 'right-click outsider singles the exact lyric row');
  await stage('lyric_group_single_outsider');
  await dismiss(outsider);
  await choose('all');
  await choose('lyrics');
  await wait(() => active() && row(0), 'lyrics can re-enter from its rendered cache');
  assert(selected().length <= 1 && !menu(), 'return to tab cannot reopen an old group');
  await stage('lyric_group_tab_reset');
  return { finalQueue: ids(await queue()), operations };
};
