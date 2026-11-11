// Real Recently Played row Next gestures with synthetic resources and libmpv null output.
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
  const nextItem = () => one('[data-menu-key="next"]', panel() ?? null);
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
  async function openMenu(clicked, gesture, selection) {
    const opener = more(clicked);
    assert(ready(opener), 'enabled history row More ' + clicked);
    if (gesture === 'contextmenu') {
      const target = row(clicked);
      const bounds = target.getBoundingClientRect();
      assert(!target.dispatchEvent(new MouseEvent('contextmenu', {
        bubbles: true, cancelable: true, button: 2,
        clientX: bounds.left + 18, clientY: bounds.top + 12,
      })), 'history handled the row context menu');
    } else if (gesture === 'keyboard') {
      opener.focus({ preventScroll: true });
      assert(!opener.dispatchEvent(new KeyboardEvent('keydown', {
        bubbles: true, cancelable: true, shiftKey: true, key: 'F10',
      })), 'history handled Shift+F10');
    } else click(opener);
    await wait(() => ready(panel()) && ready(nextItem()) &&
      opener.getAttribute('aria-expanded') === 'true', 'visible Next action for row ' + clicked);
    assert(same(selectedRows().sort(), [...selection].sort()), 'menu preserves the explicit group');
    assert(text(nextItem()) === '下一首播放', 'official history row Next label');
    const bounds = panel().getBoundingClientRect();
    assert(bounds.left >= 0 && bounds.top >= 0 && bounds.right <= innerWidth + 1 &&
      bounds.bottom <= innerHeight + 1, 'history row menu stays inside the WebView');
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
  try {
    epoch = await wait(async () => {
      const account = await invoke('account_snapshot');
      return account.status === 'authenticated' && account.epoch;
    }, 'synthetic account ready');
    click(await wait(() => all('.sidebar button').find(node => ready(node) && text(node) === '最近播放'),
      'recently played sidebar'));
    const voice = await enter('voice', '声音', ['1', '2', '3', '4']);
    await action({ kind: 'mode', mode: 'sequential' });
    await invoke('play_selection', { epoch, selection: {
      kind: 'history', history_kind: 'voice', revision: voice.revision,
      ids: ['5', '6', '7'], index: 1, quality: 'standard',
    } });
    const seedIds = ['voice:5', 'voice:6', 'voice:7'];
    const incumbent = await wait(async () => {
      const shown = await observe();
      return shown && same(shown.ids, seedIds) && shown.state.track?.voiceId === '6' &&
        shown.state.state === 'playing' && shown.state.position > .25 && shown.state;
    }, 'incumbent episode is decoding to native null output');
    await action({ kind: 'pause', paused: true });
    let previous, stableAt;
    held = await wait(async () => {
      const shown = await observe();
      if (!shown || shown.state.state !== 'paused' || shown.state.playId !== incumbent.playId ||
          shown.state.entryId !== incumbent.entryId || shown.state.position <= .25) return;
      const current = decoder(shown.state);
      if (!same(current, previous)) {
        previous = current;
        stableAt = performance.now();
        return;
      }
      return performance.now() - stableAt >= 400 && current;
    }, 'incumbent native clock is exactly paused');

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
    await emit('complete', { ok: true, stages });
  } catch (error) {
    await emit('failed', {
      ok: false, step, error: String(error?.message ?? error), stages,
      menu: panel()?.textContent, category: page()?.dataset.historyCategory,
      selection: selectedRows(), notice: nextNotice(),
      rowError: text(one('.history-row-next-error', page())),
    });
  }
})();
