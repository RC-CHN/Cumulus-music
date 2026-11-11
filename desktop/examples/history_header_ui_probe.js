// Real Recently Played header actions over a sealed account and a null audio output.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])];
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, payload = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...payload } });
  const assert = (ok, message) => { if (!ok) throw Error(message); };
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const ready = node => node?.isConnected && node.getClientRects().length &&
    !node.matches(':disabled,[aria-disabled="true"]') && !node.closest('[inert]');
  const text = node => node?.textContent.trim();
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const tabs = '[aria-label="播放记录类型"]';
  const page = () => one('.history-page');
  const active = () => text(one(`${tabs} [aria-current]`)) ?? page()?.dataset.historyCategory;
  const filter = () => one('[aria-label="筛选播放记录"]', page());
  const trigger = () => one('[data-history-menu] button.menu-trigger', page());
  const menu = () => one('[role="menu"][aria-label="更多播放记录操作"]:not([inert])');
  const item = key => one(`[data-menu-key="${key}"]`, menu() ?? null);
  const menuKeys = () => all('[role="menuitem"]', menu() ?? null).map(node => node.dataset.menuKey);
  const keys = ['add-all', 'batch', 'clear'];
  const dialog = () => one('dialog[aria-labelledby="history-confirm-title"][open]:not([inert])');
  const visibleRows = () => all('.history-window .history-row', page()).length;
  const checkedRows = () => all('.history-window input[type="checkbox"]:checked', page()).length;
  const action = action => invoke('player_action', { epoch, action });
  const queue = () => invoke('player_queue', { epoch });
  const player = () => invoke('player_snapshot', { epoch });
  const history = async kind => (await invoke('history_page', { epoch, kind, refresh: false })).value;
  const tracks = queue => queue.tracks.map(row => row.track.voiceId
    ? `voice:${row.track.voiceId}` : `song:${row.track.id}`);
  const decoder = state => ({
    epoch: state.epoch, playId: state.playId, entryId: state.entryId, state: state.state,
    position: state.position, mediaId: state.track?.id, voiceId: state.track?.voiceId,
  });
  const names = [];
  let epoch, held, step = 'startup';

  async function wait(observe, label) {
    step = label;
    const deadline = performance.now() + 18_000;
    do {
      const value = await observe();
      if (value) return value;
      await sleep(70);
    } while (performance.now() < deadline);
    throw Error(label);
  }
  function click(node) {
    assert(ready(node), 'enabled target for ' + step);
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    node.click();
  }
  function input(value) {
    const node = filter();
    assert(ready(node), 'filter input available');
    node.value = value;
    node.dispatchEvent(new Event('input', { bubbles: true }));
  }
  const settled = () => page()?.getAttribute('aria-busy') === 'false' &&
    !one('.history-content:is(.catalog-tab-enter-active,.catalog-tab-leave-active)') &&
    !page()?.closest('.page-enter-active,.page-leave-active');
  async function tab(label, kind) {
    const node = await wait(() => all(`${tabs} button`).find(node => ready(node) && text(node) === label),
      'history tab ' + label);
    if (active() !== label) click(node);
    return wait(async () => {
      if (active() !== label || !settled()) return;
      const current = await history(kind);
      return current.kind === kind && current.remoteIncluded && current.entries.length && current;
    }, 'settled history category ' + kind);
  }
  async function open() {
    click(await wait(() => ready(trigger()) && trigger(), 'enabled history header More'));
    await wait(() => ready(menu()) && same(menuKeys(), keys) &&
      trigger()?.getAttribute('aria-expanded') === 'true', 'official history header menu');
    assert(text(item('add-all')) === '添加全部至播放列表' &&
      text(item('batch')) === '批量操作' && text(item('clear')) === '清空列表',
    'official header action labels');
    const bounds = menu().getBoundingClientRect();
    assert(bounds.left >= 0 && bounds.top >= 0 && bounds.right <= innerWidth + 1 &&
      bounds.bottom <= innerHeight + 1, 'history header menu stays in the WebView');
  }
  async function capture(name, data = {}, playback = 'paused') {
    step = name;
    names.push(name);
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const native = await player(), current = await queue();
    assert(native.queueRevision === current.revision && native.entryId === current.current,
      'native queue and decoder receipt agree');
    if (playback === 'paused') assert(same(decoder(native), held), 'action disturbed paused decoder');
    if (playback === 'playing') assert(native.state === 'playing' && native.position > .25,
      'audio reached real native clock');
    await emit(`ux_${String(names.length).padStart(2, '0')}_${name}`, {
      ...data, category: active(), visibleRows: visibleRows(), checkedRows: checkedRows(),
      filter: filter()?.value ?? null, queueIds: tracks(current),
      checkPausedPlayback: playback === 'paused', playing: playback === 'playing',
    });
  }
  async function pauseStable(incumbent) {
    await action({ kind: 'pause', paused: true });
    let previous, stableAt;
    return wait(async () => {
      const current = await player();
      if (current.state !== 'paused' || current.playId !== incumbent.playId ||
          current.entryId !== incumbent.entryId || !Number.isFinite(current.position) ||
          current.position <= .25) return;
      const observed = { ...decoder(current), queueRevision: current.queueRevision };
      if (!same(observed, previous)) {
        previous = observed;
        stableAt = performance.now();
        return;
      }
      return performance.now() - stableAt >= 450 && decoder(current);
    }, 'exact paused decoder and clock for 450 ms');
  }
  const fullSongs = Array.from({ length: 300 }, (_, i) => `song:${i + 1}`);
  const baselineQueue = ['voice:5', 'voice:6', 'voice:7'];
  const songQueue = ['voice:5', 'voice:6', ...fullSongs, 'voice:7'];
  const notice = () => text(one('.history-add-notice,.history-add-error'));

  try {
    epoch = await wait(async () => {
      const state = await invoke('account_snapshot');
      return state.status === 'authenticated' && state.epoch;
    }, 'synthetic account ready');
    click(await wait(() => all('.sidebar button').find(node => text(node) === '最近播放' && ready(node)),
      'recently played sidebar'));
    const voice = await tab('声音', 'voice');
    await action({ kind: 'mode', mode: 'sequential' });
    await invoke('play_selection', { epoch, selection: {
      kind: 'history', history_kind: 'voice', revision: voice.revision,
      ids: ['5', '6', '7'], index: 1, quality: 'standard',
    } });
    const incumbent = await wait(async () => {
      const state = await player();
      return state.state === 'playing' && state.track?.voiceId === '6' &&
        state.track?.id === '1006' && state.position > .25 && state;
    }, 'independent current voice is decoding');
    held = await pauseStable(incumbent);
    assert(same(tracks(await queue()), baselineQueue), 'seeded ordinary queue in exact order');

    const songs = await tab('歌曲', 'song');
    assert(same(songs.entries.map(row => `song:${row.id}`), fullSongs), 'native song history order');
    assert(visibleRows() < songs.entries.length, 'song history exceeds the mounted virtual window');
    input('台湾 a');
    await wait(() => visibleRows() === 1 && !checkedRows(), 'one filtered unselected song');
    await open();
    assert(ready(item('add-all')), 'full history Add All needs no selection');
    await capture('song_filtered_menu', { menu: menuKeys(), nativeHistorySize: songs.entries.length });
    click(item('add-all'));
    await wait(async () => same(tracks(await queue()), songQueue) && notice() === '已添加到播放列表',
      'all 300 songs insert in native history order after the current voice');
    await capture('song_add_all', { notice: notice() });

    // Batch is a separate action; opening it cannot perform a queue edit.
    await open();
    click(item('batch'));
    await wait(() => !menu() && settled() &&
      page()?.dataset.historyView === 'batch' && !one(tabs, page()) && !trigger() &&
      ready(one('.history-window input[type="checkbox"]', page())),
    'batch operations enter the song selection controls');
    assert(active() === '歌曲', 'batch keeps the song category');
    const playSelected = one('.history-play-selected', page());
    const addSelected = one('.history-add-selected', page());
    assert(playSelected?.disabled && addSelected?.disabled && checkedRows() === 0,
      'batch opens without an implicit selection');
    click(one('.history-window input[type="checkbox"]', page()));
    await wait(() => checkedRows() === 1 && ready(playSelected) && ready(addSelected),
      'only an explicit row selection enables the batch actions');
    assert(same(tracks(await queue()), songQueue), 'opening batch is read-only for the queue');
    await capture('song_batch', { batch: true, browseTabs: !!one(tabs, page()) });
    click(await wait(() => all('button', page()).find(node =>
      text(node) === '退出批量操作' && ready(node)), 'exit batch via the Back stack'));
    await wait(() => settled() && page()?.dataset.historyView === 'browse' && active() === '歌曲' &&
      ready(trigger()) && !one('.history-window input[type="checkbox"]', page()),
    'return to original song browse category without stale selection');
    await capture('song_batch_back', { browse: true });
    input('台湾 a');
    await wait(() => visibleRows() === 1, 'filter before whole-category clear');
    const beforeClear = await history('song');

    await open();
    click(item('clear'));
    await wait(() => ready(dialog()) && text(one('h2', dialog())) === '清空歌曲播放记录？' &&
      text(document.activeElement) === '取消', 'clear confirms whole category and focuses Cancel');
    await capture('song_clear_confirmation', { title: text(one('h2', dialog())), body: text(one('p', dialog())) });
    click(one('button.secondary', dialog()));
    await wait(() => !dialog() && settled(), 'cancel leaves the song category intact');
    assert((await history('song')).revision === beforeClear.revision, 'cancel preserves native song history');
    await capture('song_clear_cancelled', { historySize: (await history('song')).entries.length });

    await open();
    click(item('clear'));
    await wait(() => ready(dialog()), 'explicit clear confirmation reopened');
    click(one('button.primary', dialog()));
    await wait(async () => !dialog() && settled() && (await history('song')).entries.length === 0,
      'explicit clear empties only the synthetic song history');
    assert(same(tracks(await queue()), songQueue), 'clearing history preserves the current playback queue');
    assert(trigger()?.disabled, 'no header action on empty native history');
    await capture('song_cleared', { historySize: 0, emptyMenuDisabled: true });

    const hidden = [];
    for (const [label, kind] of [['播客', 'podcast'], ['有声书', 'audiobook'],
      ['歌单', 'playlist'], ['专辑', 'album']]) {
      await tab(label, kind);
      assert(!trigger(), 'header queue actions are absent from ' + kind);
      hidden.push(kind);
    }
    await capture('other_categories', { withoutQueueMenu: hidden });

    await tab('声音', 'voice');
    const before = await queue();
    await action({ kind: 'queue_for', expected_queue_revision: before.revision, edit: { kind: 'clear' } });
    await wait(async () => (await queue()).tracks.length === 0 && (await player()).state === 'idle',
      'test setup clears only the independent playback queue');
    const voices = await history('voice');
    const order = voices.entries.map(row => `voice:${row.id}`);
    assert(order.length === 100 && new Set(order).size === order.length, 'complete native voice history');
    input('不存在的节目');
    await wait(() => !visibleRows() && !!one('.history-content .empty-state', page()),
      'voice filter shows no rows without emptying the source');
    await open();
    assert(ready(item('add-all')), 'non-empty native voice history stays queueable');
    await capture('voice_empty_filter_menu', { menu: menuKeys(), nativeVoiceOrder: order }, 'idle');
    click(item('add-all'));
    const started = await wait(async () => {
      const state = await player(), current = await queue();
      return state.state === 'playing' && state.position > .25 &&
        `voice:${state.track?.voiceId}` === order[0] &&
        state.track?.id === String(Number(state.track.voiceId) + 1000) &&
        same(tracks(current), order) && state;
    }, 'empty queue starts the first program from the full voice list');
    assert(started.playId !== held.playId, 'empty-queue Add creates a new native playback');
    await capture('voice_add_all_started', { nativeVoiceOrder: order }, 'playing');
    await emit('complete', { ok: true, stages: names });
  } catch (error) {
    await emit('failed', {
      ok: false, step, error: String(error?.message ?? error), stages: names,
      category: active(), keys: menuKeys(), notice: notice(), rows: visibleRows(),
    });
  }
})();
