// Real Recently Played row actions with synthetic resources and libmpv null output.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])];
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, payload = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...payload } });
  const assert = (ok, message) => { if (!ok) throw Error(message); };
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const text = node => node?.textContent.trim();
  const ready = node => node?.isConnected && node.getClientRects().length &&
    !node.matches(':disabled,[aria-disabled="true"]') && !node.closest('[inert]');
  const tabs = '[aria-label="播放记录类型"]';
  const page = () => one('.history-page');
  const row = id => one(`[data-history-row-id="${id}"]`, page());
  const more = id => one(`[data-history-row-more="${id}"]`, page());
  const panel = () => one('[role="menu"][aria-label="播放记录操作"]:not([inert])');
  const menuItem = key => one(`[data-menu-key="${key}"]`, panel() ?? null);
  const nextItem = () => menuItem('next');
  const playItem = () => menuItem('play');
  const disabled = node => node?.matches(':disabled,[aria-disabled="true"]');
  const selectedRows = () => all('[data-history-row-id][aria-selected="true"]', page())
    .map(node => node.dataset.historyRowId);
  const nextNotice = () => text(one('.history-row-next-notice', page()));
  const action = action => invoke('player_action', { epoch, action });
  const player = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  const history = async kind => (await invoke('history_page', { epoch, kind, refresh: false })).value;
  const tracks = queue => queue.tracks.map(({ track }) => track.voiceId
    ? `voice:${track.voiceId}` : `song:${track.id}`);
  const decoder = state => ({
    epoch: state.epoch, playId: state.playId, entryId: state.entryId, state: state.state,
    position: state.position, mediaId: state.track?.id, voiceId: state.track?.voiceId ?? null,
  });
  const stages = [];
  let epoch, held, step = 'startup', clipboardAttempt = 0, opening;
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
    assert(ready(node), 'enabled control for ' + step);
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    node.click();
  }
  async function observe() {
    const state = await player(), current = await queue();
    if (state.epoch !== current.epoch || state.queueRevision !== current.revision ||
        state.entryId !== current.current) return;
    return { state, current, ids: tracks(current) };
  }
  async function enter(kind, label, ids) {
    const button = await wait(() => all(`${tabs} button`).find(node =>
      ready(node) && text(node) === label), 'history tab ' + label);
    if (!button.hasAttribute('aria-current')) click(button);
    const source = await wait(async () => {
      if (text(one(`${tabs} [aria-current]`)) !== label ||
          page()?.dataset.historyView !== 'browse' ||
          page()?.getAttribute('aria-busy') !== 'false' ||
          page()?.closest('.page-enter-active,.page-leave-active') ||
          one('.history-content:is(.catalog-tab-enter-active,.catalog-tab-leave-active)') ||
          !ids.every(id => ready(row(id)))) return;
      const current = await history(kind);
      return current.kind === kind && current.remoteIncluded && current;
    }, 'settled history rows ' + kind);
    assert(source.entries.length === (kind === 'song' ? 300 : 100) &&
      ids.every(id => source.entries.some(entry => entry.id === id)), 'sealed native history source');
    return source;
  }
  function select(ids) {
    for (const [index, id] of ids.entries()) {
      const target = row(id);
      assert(ready(target), 'mounted selection row ' + id);
      target.scrollIntoView({ block: 'nearest' });
      target.dispatchEvent(new MouseEvent('click', {
        bubbles: true, cancelable: true, button: 0, ctrlKey: index > 0,
      }));
    }
  }
  async function openMenu(clicked, gesture, selection, key = 'next', enabled = true) {
    const opener = more(clicked);
    assert(ready(opener), 'enabled history row More ' + clicked);
    const events = new AbortController();
    const trace = opening = { clicked, gesture, key, events: [], dispatched: false };
    let quietSince = performance.now(), previous;
    const rect = () => {
      const bounds = opener.getBoundingClientRect();
      return [bounds.left, bounds.top, bounds.width, bounds.height].map(n => Math.round(n * 10) / 10);
    };
    for (const event of ['scroll', 'focusin', 'resize', 'blur']) {
      window.addEventListener(event, notice => {
        quietSince = performance.now();
        if (trace.events.length < 16) trace.events.push({
          type: event, afterGesture: trace.dispatched,
          target: notice.target instanceof Element ?
            notice.target.getAttribute('aria-label') ?? notice.target.className?.toString() : null,
        });
      }, { capture: true, signal: events.signal });
    }
    try {
      opener.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'instant' });
      if (gesture !== 'contextmenu') opener.focus({ preventScroll: true });
      // Menu preparation deliberately cancels on scroll/focus drift. Give the
      // fixture's programmatic selection scrolls time to finish before dispatch.
      await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      trace.before = await wait(() => {
        if (more(clicked) !== opener || !ready(opener) ||
            gesture !== 'contextmenu' && document.activeElement !== opener) return;
        const current = rect();
        if (!same(current, previous)) {
          previous = current;
          quietSince = performance.now();
          return;
        }
        return performance.now() - quietSince >= 105 && current;
      }, 'settled More anchor and fixture scrolling for ' + clicked);
      trace.dispatched = true;
      if (gesture === 'contextmenu') {
        const target = row(clicked);
        const bounds = target.getBoundingClientRect();
        assert(!target.dispatchEvent(new MouseEvent('contextmenu', {
          bubbles: true, cancelable: true, button: 2,
          clientX: bounds.left + 18, clientY: bounds.top + 12,
        })), 'history handled the row context menu');
      } else if (gesture === 'keyboard') {
        assert(!opener.dispatchEvent(new KeyboardEvent('keydown', {
          bubbles: true, cancelable: true, shiftKey: true, key: 'F10',
        })), 'history handled Shift+F10');
      } else opener.click();
      await wait(() => {
        const item = menuItem(key);
        trace.expanded = opener.getAttribute('aria-expanded');
        trace.current = rect();
        return ready(panel()) && item?.getClientRects().length && disabled(item) === !enabled &&
          trace.expanded === 'true';
      }, 'visible ' + key + ' action for row ' + clicked);
    } finally {
      events.abort();
    }
    assert(same(selectedRows().sort(), [...selection].sort()), 'menu preserves the explicit group');
    if (key === 'play' || key === 'next')
      assert(text(menuItem(key)) === (key === 'play' ? '播放' : '下一首播放'),
        'official history row action label');
    if (key === 'play') {
      const items = all('[data-menu-key]', panel());
      assert(items[0] === playItem() && items[1] === nextItem(), 'Play precedes Next in the row menu');
      if (enabled)
        await wait(() => document.activeElement === playItem(), 'Play receives first keyboard focus');
    }
    const bounds = panel().getBoundingClientRect();
    assert(bounds.left >= 0 && bounds.top >= 0 && bounds.right <= innerWidth + 1 &&
      bounds.bottom <= innerHeight + 1, 'history row menu stays inside the WebView');
  }
  async function dismissMenu(clicked) {
    assert(panel(), 'menu is present for Escape');
    panel().dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await wait(() => !panel() && document.activeElement === more(clicked),
      'Escape restores the clicked row More focus');
  }
  async function capture(name, expected, data = {}, paused = true) {
    step = name;
    const shown = await wait(observe, 'coherent capture ' + name);
    assert(same(shown.ids, expected), 'capture queue order ' + name);
    if (paused) assert(same(decoder(shown.state), held), 'paused decoder/clock changed');
    else assert(shown.state.state === 'playing' && shown.state.position > .25 &&
      shown.state.playId, 'native null clock did not advance');
    stages.push(name);
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await emit(`ux_${String(stages.length).padStart(2, '0')}_${name}`, {
      ...data, queueIds: shown.ids, checkPausedPlayback: paused, playing: !paused,
    });
    return shown;
  }
  async function afterInsert(before, expected) {
    return wait(async () => {
      if (panel() || nextNotice() !== '已设为下一首播放') return;
      const shown = await observe();
      if (!shown || shown.current.revision === before.current.revision || !same(shown.ids, expected))
        return;
      assert(same(decoder(shown.state), held), 'row Next replaced or moved the paused decoder');
      return shown;
    }, 'Next inserts after the current row in gesture order');
  }
  async function clearQueue(mode) {
    const old = await wait(observe, 'coherent queue before empty setup');
    await action({ kind: 'queue_for', expected_queue_revision: old.current.revision, edit: { kind: 'clear' } });
    await action({ kind: 'mode', mode });
    const empty = await wait(async () => {
      const shown = await observe();
      return shown && !shown.ids.length && shown.state.state === 'idle' && !shown.state.playId &&
        shown.state.mode === mode && shown;
    }, 'empty native queue in ' + mode);
    return { revision: empty.current.revision, priorPlayId: old.state.playId, mode, ids: empty.ids };
  }
  async function emptyNext(kind, label, ids, clicked, mode, name) {
    const empty = await clearQueue(mode);
    await enter(kind, label, ids);
    select(ids);
    await openMenu(clicked, 'button', ids);
    const current = await wait(observe, 'empty queue still unchanged by menu');
    assert(!current.ids.length && current.state.state === 'idle' &&
      current.current.revision === empty.revision, 'opening Next is read-only');
    click(nextItem());
    const expected = ids.map(id => `${kind}:${id}`);
    const started = await wait(async () => {
      const shown = await observe();
      if (!shown || panel() || !same(shown.ids, expected) ||
          shown.current.revision === empty.revision || shown.state.mode !== mode ||
          shown.state.state !== 'playing' || shown.state.position <= .25) return;
      return shown;
    }, 'empty Next plays the clicked source and preserves gesture order');
    const expectedMedia = kind === 'voice' ? String(Number(clicked) + 1000) : clicked;
    assert(started.state.playId !== empty.priorPlayId && started.state.track?.id === expectedMedia &&
      (started.state.track?.voiceId ?? null) === (kind === 'voice' ? clicked : null),
    'empty Next decoded the clicked row, not the first selected row');
    await capture(name, expected, { clicked, selected: ids, gesture: 'button', empty }, false);
  }
  async function pauseAt(incumbent, minimum = .25) {
    await action({ kind: 'pause', paused: true });
    let previous, stableAt;
    return wait(async () => {
      const shown = await observe();
      if (!shown || shown.state.state !== 'paused' || shown.state.playId !== incumbent.playId ||
          shown.state.entryId !== incumbent.entryId || shown.state.position <= minimum) return;
      const current = decoder(shown.state);
      if (!same(current, previous)) {
        previous = current;
        stableAt = performance.now();
        return;
      }
      return performance.now() - stableAt >= 400 && current;
    }, 'current native clock is exactly paused');
  }
  const seedIds = ['voice:5', 'voice:6', 'voice:7'];
  const playSeedIds = ['voice:11', 'voice:12', 'voice:13'];
  async function seedPausedVoice(queueIds = seedIds) {
    const voice = await enter('voice', '声音', ['1', '2', '3', '4']);
    await action({ kind: 'mode', mode: 'sequential' });
    const ids = queueIds.map(id => id.split(':')[1]);
    step = 'seed native history voices ' + ids.join(',');
    await invoke('play_selection', { epoch, selection: {
      kind: 'history', history_kind: 'voice', revision: voice.revision,
      ids, index: 1, quality: 'standard',
    } });
    const incumbent = await wait(async () => {
      const shown = await observe();
      return shown && same(shown.ids, queueIds) && shown.state.track?.voiceId === ids[1] &&
        shown.state.state === 'playing' && shown.state.position > .25 && shown.state;
    }, 'incumbent episode is decoding to native null output');
    held = await pauseAt(incumbent);
  }
  function activatePlay(clicked) {
    click(playItem());
    assert(document.activeElement === more(clicked), 'Play returns focus to the clicked row');
  }
  function playerIdentity(state) {
    return [state.epoch, state.playId, state.entryId];
  }
  async function played(ids, media, voice, before, resume = false) {
    let first;
    return wait(async () => {
      const shown = await observe();
      if (!shown || panel() || !same(shown.ids, ids) || shown.state.state !== 'playing' ||
          shown.state.track?.id !== media || (shown.state.track?.voiceId ?? null) !== voice ||
          !Number.isFinite(shown.state.position)) return;
      const identity = playerIdentity(shown.state);
      assert(identity.every(Boolean), 'Play has a bound native identity');
      assert(resume ? same(identity, playerIdentity(before.state)) :
        shown.state.playId !== before.state.playId && shown.state.entryId !== before.state.entryId,
      'Play resumed or loaded the expected native decoder');
      if (resume)
        assert(shown.state.position >= before.state.position - .05, 'Play rewound the paused clock');
      if (!first) first = { identity, clock: shown.state.position };
      assert(same(first.identity, identity), 'one decoder owns both Play clock samples');
      if (shown.state.position > .25 && shown.state.position - first.clock > .2)
        return { first: first.clock, last: shown.state.position };
    }, 'same native decoder advanced the clicked Play row');
  }
  async function playRows() {
    await seedPausedVoice(playSeedIds);
    await enter('song', '歌曲', ['1', '3', '8']);
    select(['8']);
    await openMenu('8', 'button', ['8'], 'play', false);
    assert(disabled(nextItem()), 'all blocked selection disables both playback actions');
    assert(same(menuKeys(), ['play', 'next', 'comments', 'collect', 'remove']) &&
      ready(menuItem('comments')) && ready(menuItem('collect')),
      'copyright-blocked song keeps comments but omits copy');
    playItem().click();
    const blocked = await capture('play_blocked', playSeedIds, {
      clicked: '8', selected: ['8'], playDisabled: disabled(playItem()), nextDisabled: disabled(nextItem()),
      copyrightHint: [playItem().title, nextItem().title], keys: menuKeys(),
    });
    await dismissMenu('8');

    const rejectedIds = ['3', '8'];
    select(rejectedIds);
    await openMenu('8', 'contextmenu', rejectedIds, 'play');
    activatePlay('8');
    const error = await wait(() => {
      const alert = one('.history-row-menu-error', page());
      return !panel() && ready(alert) && text(alert);
    }, 'mixed selection with blocked clicked row is rejected');
    const rejected = await capture('play_rejected', playSeedIds, {
      clicked: '8', selected: rejectedIds, error, focusRestored: true,
    });
    assert(rejected.current.revision === blocked.current.revision,
      'blocked clicked Play left the native queue revision unchanged');

    const songIds = ['3', '8', '1'];
    select(songIds);
    await openMenu('1', 'keyboard', songIds, 'play');
    const songQueue = ['voice:11', 'voice:12', 'song:3', 'song:1', 'voice:13'];
    activatePlay('1');
    const songClock = await played(songQueue, '1', null, rejected);
    const song = await capture('play_song', songQueue, {
      clicked: '1', selected: songIds, clock: songClock, focusRestored: true,
    }, false);

    await action({ kind: 'seek', seconds: 4.25 });
    held = await pauseAt(song.state, 3.5);
    await enter('song', '歌曲', ['1', '9']);
    const resumeIds = ['9', '1'];
    select(resumeIds);
    await openMenu('1', 'button', resumeIds, 'play');
    const paused = await capture('play_resume_ready', songQueue, { clicked: '1', selected: resumeIds });
    activatePlay('1');
    const resumeQueue = ['voice:11', 'voice:12', 'song:3', 'song:9', 'song:1', 'voice:13'];
    const resumedClock = await played(resumeQueue, '1', null, paused, true);
    const resumed = await capture('play_resumed', resumeQueue, {
      clicked: '1', selected: resumeIds, clock: resumedClock, focusRestored: true,
    }, false);

    await enter('voice', '声音', ['2', '4']);
    const voiceIds = ['4', '2'];
    select(voiceIds);
    await openMenu('2', 'contextmenu', voiceIds, 'play');
    const voiceQueue = ['voice:11', 'voice:12', 'song:3', 'song:9', 'song:1',
      'voice:4', 'voice:2', 'voice:13'];
    activatePlay('2');
    const voiceClock = await played(voiceQueue, '1002', '2', resumed);
    return capture('play_voice', voiceQueue, {
      clicked: '2', selected: voiceIds, clock: voiceClock, focusRestored: true,
    }, false);
  }
  const menuKeys = () => all('[data-menu-key]', panel()).map(node => node.dataset.menuKey);
  const commentsPage = () => one('.comments-page:not(.comments-embedded):not(.page-leave-active)');
  const commentHeader = () => one('.comment-resource-header', commentsPage());
  const deleteDialog = () => one('dialog[open][aria-labelledby="history-confirm-title"]:not([inert])');
  const copyNotice = () => text(one('.history-row-menu-notice[role="status"]', page()));
  const back = () => one('button[aria-label="返回上一页"]');
  const singleActions = ['play', 'next', 'comments', 'collect', 'copy', 'remove'];
  const voiceActions = ['play', 'next', 'comments', 'download', 'copy', 'remove'];
  async function openActions(kind, ids, clicked, gesture, expected) {
    const title = kind === 'song' ? '歌曲' : '声音';
    const source = await enter(kind, title, ids);
    select(ids);
    await openMenu(clicked, gesture, ids);
    const keys = menuKeys();
    assert(same(keys, expected), 'official row menu action order: ' + keys.join(','));
    return source;
  }
  async function routeComments(kind, clicked, source, label, body, queueIds) {
    const resourceKind = kind === 'song' ? 'track' : 'voice';
    click(menuItem('comments'));
    const header = await wait(() => {
      const current = commentHeader();
      const page = commentsPage();
      if (!ready(current) || current.dataset.resourceKind !== resourceKind ||
          current.dataset.resourceId !== clicked || current.getAttribute('aria-busy') !== 'false' ||
          page?.getAttribute('aria-busy') !== 'false' ||
          page?.closest('.page-enter-active,.page-leave-active') ||
          !all('.comment-card', page).some(card => card.textContent.includes(body))) return;
      return current;
    }, 'actual comment page for ' + resourceKind + ':' + clicked);
    await capture('actions_' + kind + '_comment', queueIds, {
      commentKind: header.dataset.resourceKind, commentId: header.dataset.resourceId,
      body, label, historyRevision: source.revision,
    });
    click(await wait(() => ready(back()) && back(), 'history comment Back control'));
    const returned = await enter(kind, kind === 'song' ? '歌曲' : '声音', [clicked]);
    assert(returned.revision === source.revision, 'comment navigation did not change the source');
    await wait(() => !commentsPage() && document.activeElement === more(clicked),
      'comment Back returns to the same clicked history row');
  }
  async function copyPublic(kind, clicked, selected, queueIds) {
    click(menuItem('copy'));
    await wait(() => !panel() && copyNotice() === '复制成功' &&
      document.activeElement === more(clicked), 'copy acknowledgement and same row focus');
    const attempt = ++clipboardAttempt;
    const expected = 'https://music.163.com/' + (kind === 'song' ? 'song' : 'dj') + '?id=' + clicked;
    await emit('history-row-clipboard', { attempt, expected });
    const native = await wait(() => window.__historyRowClipboard?.attempt === attempt &&
      window.__historyRowClipboard, 'native system clipboard readback ' + expected);
    assert(native.matches, 'native clipboard contains exact public link without tracking');
    await capture('actions_' + kind + '_copied', queueIds, {
      clicked, selected, clipboard: expected, nativeClipboard: native.matches,
      focusRestored: true, notice: copyNotice(),
    });
  }
  async function confirmDelete() {
    assert(text(menuItem('remove')) === '从列表中删除', 'official Delete menu label');
    click(menuItem('remove'));
    return wait(() => {
      const current = deleteDialog();
      if (!panel() && ready(current) && text(one('h2', current)) === '删除 2 条播放记录？' &&
          text(document.activeElement) === '取消') return current;
    }, 'confirm exact two-row history deletion and focus Cancel');
  }
  async function rowActions(incumbent) {
    held = await pauseAt(incumbent.state);
    const queueIds = incumbent.ids;
    const song = await openActions('song', ['10'], '10', 'keyboard', singleActions);
    const songLabel = text(menuItem('comments'));
    assert(songLabel === '查看评论(36)', 'song menu projects the clicked count');
    await capture('actions_song_menu', queueIds, { clicked: '10', keys: menuKeys(), label: songLabel });
    await routeComments('song', '10', song, songLabel, '合成历史歌曲评论', queueIds);
    await openActions('song', ['10'], '10', 'button', singleActions);
    await copyPublic('song', '10', ['10'], queueIds);

    await openActions('song', ['11'], '11', 'button', singleActions);
    const fallback = text(menuItem('comments'));
    assert(fallback === '查看评论' && ready(menuItem('comments')),
      'failed count leaves the plain enabled comment action');
    await capture('actions_song_count_fallback', queueIds, { clicked: '11', label: fallback });
    await dismissMenu('11');

    const voiceIds = ['9', '10'];
    const voice = await openActions('voice', voiceIds, '10', 'contextmenu', voiceActions);
    const voiceLabel = text(menuItem('comments'));
    assert(voiceLabel === '查看评论(47)', 'voice group uses the clicked episode count');
    await capture('actions_voice_menu', queueIds, {
      clicked: '10', selected: voiceIds, keys: menuKeys(), label: voiceLabel,
    });
    await routeComments('voice', '10', voice, voiceLabel, '合成历史声音评论', queueIds);
    await openActions('voice', voiceIds, '10', 'button', voiceActions);
    await copyPublic('voice', '10', voiceIds, queueIds);

    const remove = ['10', '7'];
    const groupActions = ['play', 'next', 'collect', 'remove'];
    const before = await openActions('song', remove, '7', 'contextmenu', groupActions);
    const confirmation = await confirmDelete();
    await capture('actions_delete_confirm', queueIds, {
      clicked: '7', selected: remove, keys: groupActions, title: text(one('h2', confirmation)),
      actions: all('footer button', confirmation).map(text), focus: text(document.activeElement),
      description: text(one('p', confirmation)), beforeRevision: before.revision,
    });
    click(one('footer button.secondary', confirmation));
    await wait(() => !deleteDialog() && document.activeElement === more('7'),
      'cancel keeps the row and restores the original More focus');
    const cancelled = await history('song');
    assert(cancelled.revision === before.revision &&
      same(cancelled.entries.map(entry => entry.id), before.entries.map(entry => entry.id)),
    'Cancel does not change native history');
    await capture('actions_delete_cancelled', queueIds, {
      clicked: '7', selected: remove, focusRestored: true, revision: cancelled.revision,
    });

    await openActions('song', remove, '7', 'button', groupActions);
    const second = await confirmDelete();
    click(one('footer button.primary', second));
    const after = await wait(async () => {
      if (deleteDialog() || row('7') || row('10') || !ready(more('9')) ||
          document.activeElement !== one(`${tabs} [aria-current]`)) return;
      const current = await history('song');
      const expected = before.entries.map(entry => entry.id).filter(id => !remove.includes(id));
      if (current.revision !== before.revision && same(current.entries.map(entry => entry.id), expected))
        return current;
    }, 'confirmed deletion removes only the frozen group and focuses the history tab');
    await capture('actions_delete_confirmed', queueIds, {
      clicked: '7', selected: remove, remaining: after.entries.length,
      revision: after.revision, fallbackFocus: text(document.activeElement),
    });
  }
  try {
    epoch = await wait(async () => {
      const account = await invoke('account_snapshot');
      return account.status === 'authenticated' && account.epoch;
    }, 'synthetic account ready');
    click(await wait(() => all('.sidebar button').find(node => ready(node) && text(node) === '最近播放'),
      'recently played sidebar'));
    await seedPausedVoice();

    const songs = await enter('song', '歌曲', ['1', '2', '3', '4']);
    const visibleRows = all('.history-window .history-row', page()).length;
    assert(visibleRows < songs.entries.length, 'song source includes unmounted virtual rows');
    const songIds = ['3', '1'];
    select(songIds);
    await openMenu('1', 'contextmenu', songIds);
    const opened = await capture('song_context', seedIds, {
      selected: songIds, clicked: '1', gesture: 'contextmenu', visibleRows,
      nativeHistorySize: songs.entries.length, menuKey: nextItem().dataset.menuKey,
    });
    const songQueue = ['voice:5', 'voice:6', 'song:3', 'song:1', 'voice:7'];
    click(nextItem());
    await afterInsert(opened, songQueue);
    await capture('song_inserted', songQueue, {
      selected: songIds, clicked: '1', gesture: 'contextmenu', notice: nextNotice(),
    });

    await enter('voice', '声音', ['1', '2', '3', '4']);
    await action({ kind: 'mode', mode: 'random' });
    const voiceIds = ['4', '2'];
    select(voiceIds);
    await openMenu('2', 'keyboard', voiceIds);
    const beforeVoice = await wait(observe, 'voice Next menu leaves prior queue untouched');
    assert(same(beforeVoice.ids, songQueue) && beforeVoice.state.mode === 'random' &&
      same(decoder(beforeVoice.state), held), 'keyboard menu is read-only');
    const voiceQueue = ['voice:5', 'voice:6', 'voice:4', 'voice:2', 'song:3', 'song:1', 'voice:7'];
    click(nextItem());
    await afterInsert(beforeVoice, voiceQueue);
    await capture('voice_inserted', voiceQueue, {
      selected: voiceIds, clicked: '2', gesture: 'keyboard', notice: nextNotice(),
    });

    const nextButton = () => one('.playerbar [aria-label="下一首"]');
    click(await wait(() => ready(nextButton()) && nextButton(), 'native Next transport control'));
    await wait(async () => {
      const shown = await observe();
      return shown && shown.state.state === 'playing' && shown.state.playId !== held.playId &&
        shown.state.track?.voiceId === '4' && shown.state.track?.id === '1004' &&
        shown.state.position > .25 && shown;
    }, 'random traversal puts first selected episode next');
    await capture('voice_random_step', voiceQueue, { previousPlayId: held.playId }, false);

    await emptyNext('song', '歌曲', ['4', '2'], '2', 'random', 'empty_song_clicked');
    await emptyNext('voice', '声音', ['3', '1'], '1', 'sequential', 'empty_voice_clicked');
    await rowActions(await playRows());
    await emit('complete', { ok: true, stages });
  } catch (error) {
    await emit('failed', {
      ok: false, step, error: String(error?.message ?? error), stages,
      menu: panel()?.textContent, keys: menuKeys(), opening, category: page()?.dataset.historyCategory,
      selection: selectedRows(), notice: nextNotice(),
      rowError: text(one('.history-row-menu-error,.history-row-next-error', page())),
      focused: document.activeElement?.getAttribute('aria-label') ??
        text(document.activeElement)?.slice(0, 80),
      confirm: text(one('h2', deleteDialog())),
      comment: commentHeader() && {
        kind: commentHeader().dataset.resourceKind,
        id: commentHeader().dataset.resourceId,
        headerBusy: commentHeader().getAttribute('aria-busy'),
        pageBusy: commentsPage()?.getAttribute('aria-busy'),
        cards: all('.comment-card', commentsPage()).length,
      },
    });
  }
})();
