// Native local batch removal from two pages, with a real paused decoder.
(async () => {
  const one = (selector, root = document) => root.querySelector(selector);
  const all = (selector, root = document) => [...root.querySelectorAll(selector)];
  const button = (label, root = document) => all('button', root).find(
    (item) => item.getAttribute('aria-label') === label || item.textContent.trim() === label,
  );
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'local-proof', payload: { stage, ...data },
  });
  const assert = (value, label) => { if (!value) throw Error(label); };
  async function wait(test, label, timeout = 18000) {
    const stop = performance.now() + timeout;
    while (performance.now() < stop) {
      const value = await test();
      if (value) return value;
      await new Promise((done) => setTimeout(done, 100));
    }
    throw Error(label);
  }
  function click(item) {
    assert(item?.isConnected && !item.disabled, 'control is missing or disabled');
    item.focus({ preventScroll: true });
    item.click();
  }
  const query = (keyword = '') => ({
    keyword, sort: 'title', descending: true, group: 'list', groupKey: null,
    includeMissing: true, offset: 0, limit: 200, revision: null,
  });
  const nativePage = (keyword = '') => invoke('local_page', { query: query(keyword) });
  const filter = 'qx-local-selection';
  const toolbar = () => button('更多本地音乐操作', one('.local-page') || document);
  const menu = () => one('.action-menu[role="menu"]');
  const batch = () => one('[data-local-batch]');
  const rows = () => batch() ? all('[data-local-batch-id]', batch()) : [];
  const firstCheck = () => {
    const row = rows()[0];
    return row?.matches('input') ? row : row?.querySelector('input[type="checkbox"]');
  };
  const selected = (count) => new RegExp(`(^|[^0-9])${count}([^0-9]|$)`).test(
    one('[data-local-batch-selection]')?.textContent ?? '',
  );
  const remove = () => one('[data-local-batch-remove]');
  const play = () => one('[data-local-batch-play]');
  const batchPlay = window.__localProof.batchPlay;
  const batchAdd = window.__localProof.batchAdd;
  const reversed = batchPlay || batchAdd;
  const add = () => one('[data-local-batch-add]');
  const localSearch = () => one('[aria-label="搜索本地音乐"]');
  const localReady = () => one('.local-page')?.dataset.localLoading === 'false';
  function visible(target) {
    if (!target?.isConnected) return false;
    const bounds = target.getBoundingClientRect();
    return bounds.width > 0 && bounds.height > 0 && target.contains(
      document.elementFromPoint(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2));
  }
  async function capture(stage, target, data = {}) {
    if (!window.__localProof.capture) return;
    await wait(async () => {
      const before = target();
      if (one('.menu-enter-active,.menu-leave-active') || !visible(before)) return false;
      await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));
      return target() === before && visible(before);
    }, `${stage}: expected current painted control`);
    await emit(stage, data);
    await wait(() => window.__folderCaptureAck === stage, 'capture acknowledgement');
  }
  try {
    await wait(() => one('.welcome'), 'guest startup');
    const { epoch } = await wait(async () => {
      const state = await invoke('account_snapshot');
      return ['signed_out', 'expired'].includes(state.status) && state;
    }, 'settled guest');
    click(button('本地音乐'));
    await wait(() => one('.local-page'), 'local page');
    await invoke('local_command', {
      command: { kind: 'watch', path: window.__localProof.root + '/media' },
    });
    const initial = await wait(async () => {
      const page = await nativePage(filter);
      return page.total === 105 && page;
    }, '105 generated files indexed', 40000);
    click(await wait(() => one('[aria-label="播放本地歌曲 雨声"]:not(:disabled)'), 'unrelated local playback'));
    await wait(async () => {
      const state = await invoke('player_snapshot', { epoch });
      return state.state === 'playing' && state.position > .25 && state.track?.name === '雨声';
    }, 'native clock moves before batch action');
    const paused = await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    assert(paused.state === 'paused' && paused.playId, 'native player paused');
    const search = localSearch();
    search.value = filter;
    search.dispatchEvent(new Event('input', { bubbles: true }));
    search.form.requestSubmit();
    await wait(() => localReady() && all('.local-tracks tbody tr').length === 105, 'ordinary filter applied');
    click(await wait(() => toolbar() && !toolbar().disabled && toolbar(), 'ordinary toolbar menu'));
    const batchAction = () => menu() && button('批量操作', menu());
    await wait(batchAction, 'official batch menu entry');
    await capture('ux_01_batch_menu', batchAction);
    click(batchAction());
    await wait(() => batch() && rows().length === 100 && firstCheck(), 'first 100 batch rows');
    const first = rows()[0].dataset.localBatchId;
    assert(play()?.disabled && add()?.disabled && remove()?.disabled, 'an empty batch cannot play or remove');
    if (!reversed) {
      click(firstCheck());
      await wait(() => selected(1), 'first checked row');
    }
    click(one('[data-local-batch-next]'));
    await wait(() => rows().length === 5 && firstCheck() && selected(reversed ? 0 : 1), 'second batch page');
    const second = rows()[0].dataset.localBatchId;
    click(firstCheck());
    if (reversed) {
      await wait(() => selected(1), 'selected second-page row first');
      click(one('[data-local-batch-previous]'));
      await wait(() => rows().length === 100 && firstCheck() && selected(1), 'return to first page with selection');
      click(firstCheck());
    }
    await wait(() => selected(2) && remove() && !remove().disabled &&
      play() && !play().disabled, 'two checked across pages');
    assert(first && second && first !== second, 'distinct native selection IDs');
    assert(!button('从本地磁盘删除', batch()), 'no physical deletion action');
    await capture('ux_02_batch_cross_page', batchAdd ? add : batchPlay ? play : remove, { ids: [first, second] });

    if (batchAdd) {
      const queue = () => invoke('player_queue', { epoch });
      const snapshot = () => invoke('player_snapshot', { epoch });
      const ack = () => one('[data-local-batch-notice]')?.textContent.includes('已添加到播放列表');
      const prior = await queue();
      const anchor = prior.tracks.findIndex((row) => row.entryId === paused.entryId) + 1;
      const ids = [first, second];
      const before = prior.tracks.map((row) => row.track.id);
      const keep = (id) => !ids.includes(id);
      assert(anchor > 0 && ids.every((id) => before.includes(id)), 'live queue contains both duplicates');
      const expected = [...before.slice(0, anchor).filter(keep), ...ids, ...before.slice(anchor).filter(keep)];
      click(add());
      await wait(() => ack() && !add().disabled, 'native queue edit acknowledgement');
      const added = await queue();
      const still = await snapshot();
      assert(added.tracks.map((row) => row.track.id).join() === expected.join() &&
        added.current === paused.entryId && added.revision !== prior.revision, 'exact nonempty insertion and dedup');
      assert(still.playId === paused.playId && still.entryId === paused.entryId &&
        still.state === 'paused' && Math.abs(still.position - paused.position) < .05 && selected(2),
        'nonempty add retains native decoder, pause and clock');
      await capture('ux_03_batch_added', add);

      await invoke('player_action', { epoch, action: {
        kind: 'queue_for', expected_queue_revision: added.revision, edit: { kind: 'clear' },
      } });
      await wait(async () => {
        const state = await snapshot();
        return state.state === 'idle' && state.queueLen === 0 && !state.playId && add() && !add().disabled;
      }, 'empty native queue ready');
      click(add());
      const playing = await wait(async () => {
        const state = await snapshot();
        return state.state === 'playing' && state.track?.id === first && state.position > .2 && state;
      }, 'empty add starts first selected native file');
      const fresh = await queue();
      assert(fresh.tracks.map((row) => row.track.id).join() === ids.join(), 'empty add contains only selected');
      await capture('ux_04_batch_empty_started', add);
      await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
      await invoke('player_action', { epoch, action: { kind: 'seek', seconds: 3 } });
      const held = await wait(async () => {
        const state = await snapshot();
        return state.state === 'paused' && Math.abs(state.position - 3) < .08 && state;
      }, 'exact now-current local file paused');
      const extra = rows()[1].dataset.localBatchId;
      click(rows()[1].querySelector('input'));
      await wait(() => selected(3) && !add().disabled, 'same current plus a new local file selected');
      click(add());
      await wait(() => ack() && !add().disabled, 'active duplicate add acknowledged');
      const duplicate = await queue();
      const final = await snapshot();
      assert(duplicate.tracks.map((row) => row.track.id).join() === [first, extra, second].join() &&
        final.playId === playing.playId && final.entryId === held.entryId && final.state === 'paused' &&
        Math.abs(final.position - held.position) < .05, 'active duplicate stays the same paused decoder');
      await capture('ux_05_batch_active_duplicate', add);

      click(button('返回上一页'));
      await wait(() => localReady() && localSearch()?.value === filter &&
        all('.local-tracks tbody tr').length === 105 && document.activeElement === toolbar(), 'Back restores filter and focus');
      assert((await snapshot()).playId === held.playId, 'Back keeps exact local decoder');
      await capture('ux_06_batch_add_back', localSearch);
      await emit('complete', {
        ok: true, selected: ids, selectedInReverse: true, nonemptyQueueVerified: true, emptyStarts: true,
        activeDuplicateRetainsDecoder: true, retainedFilterOnBack: true, restoredMenuFocus: true,
        count: (await nativePage(filter)).total, realMusicRequests: 0,
      });
      return;
    }

    if (batchPlay) {
      const queue = () => invoke('player_queue', { epoch });
      const snapshot = () => invoke('player_snapshot', { epoch });
      const previous = await queue();
      const anchor = previous.tracks.findIndex((row) => row.entryId === paused.entryId) + 1;
      const expectedOrder = previous.tracks.map((row) => row.track.id);
      const selectedIds = [first, second];
      assert(anchor > 0 && selectedIds.every((id) => expectedOrder.includes(id)),
        'prior native queue contains both duplicates and exact current entry');
      const keep = (id) => !selectedIds.includes(id);
      const expected = [...expectedOrder.slice(0, anchor).filter(keep), ...selectedIds,
        ...expectedOrder.slice(anchor).filter(keep)];
      click(play());
      const playing = await wait(async () => {
        const state = await snapshot();
        return state.state === 'playing' && state.track?.id === first && state.position > .2 && state;
      }, 'first selected row reaches real local decoder');
      const installed = await queue();
      assert(playing.playId !== paused.playId && selected(2) &&
        installed.current === playing.entryId &&
        installed.tracks.map((row) => row.track.id).join() === expected.join(),
        'native queue keeps unselected prefix/suffix, moves duplicates and starts in canonical order');
      await capture('ux_03_batch_playing', play, { playId: playing.playId, selectedIds });
      await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
      await invoke('player_action', { epoch, action: { kind: 'seek', seconds: 3 } });
      const held = await wait(async () => {
        const state = await snapshot();
        return state.state === 'paused' && Math.abs(state.position - 3) < .08 && state;
      }, 'exact current local file paused at a nonzero time');
      click(await wait(() => play() && !play().disabled && play(), 'play can resume'));
      const resumed = await wait(async () => {
        const state = await snapshot();
        return state.state === 'playing' && state.playId === held.playId &&
          state.position >= held.position && state;
      }, 'toolbar resumes the existing local decoder');
      assert(resumed.entryId === held.entryId && resumed.queueRevision === held.queueRevision &&
        resumed.position - held.position < 1.0 && selected(2), 'resume preserves exact queue and clock');
      await capture('ux_04_batch_resumed', play, { playId: resumed.playId, resumedPosition: resumed.position });
      const back = () => button('返回上一页');
      click(back());
      await wait(() => localReady() && localSearch()?.value === filter &&
        all('.local-tracks tbody tr').length === 105 && document.activeElement === toolbar(),
      'Back restores filter, rows and the local menu focus');
      const current = await snapshot();
      assert(current.playId === resumed.playId && current.track?.id === first, 'Back leaves exact local decoder');
      await capture('ux_05_batch_play_back', localSearch);
      await emit('complete', {
        ok: true, selected: selectedIds, selectedInReverse: true, insertionOrderVerified: true,
        decoderAndClockRetained: true, retainedFilterOnBack: true, restoredMenuFocus: true,
        count: (await nativePage(filter)).total, realMusicRequests: 0,
      });
      return;
    }

    click(remove());
    const hidden = await wait(async () => {
      const page = await nativePage(filter);
      return page.total === 103 && batch() && selected(0) && page;
    }, 'batch visibility committed');
    assert(hidden.revision !== initial.revision &&
      hidden.tracks.every((row) => ![first, second].includes(row.track.id)), 'exact two chosen rows hidden');
    const stable = await invoke('player_snapshot', { epoch });
    assert(stable.state === 'paused' && stable.playId === paused.playId &&
      stable.entryId === paused.entryId && stable.queueRevision === paused.queueRevision &&
      Math.abs(stable.position - paused.position) < .05, 'native paused clock and queue retained');
    const back = () => button('返回上一页');
    await capture('ux_03_batch_hidden', back, { remaining: hidden.total });
    click(back());
    await wait(() => localReady() && localSearch()?.value === filter &&
      all('.local-tracks tbody tr').length === 103, 'Back preserves original keyword and fresh result');
    await wait(() => document.activeElement === toolbar(), 'Back restores the local menu opener focus');
    const scan = () => button('重新扫描', one('.local-page') || document);
    click(await wait(() => scan() && !scan().disabled && scan(), 'explicit restore command'));
    const restored = await wait(async () => {
      const page = await nativePage(filter);
      return localReady() && scan() && !scan().disabled && page.total === 105 &&
        all('.local-tracks tbody tr').length === 105 && page;
    }, 'explicit rescan restores both rows');
    assert([first, second].every((id) => restored.tracks.some((row) => row.track.id === id)),
      'exact IDs restored');
    await capture('ux_04_batch_restored', localSearch);
    await emit('complete', {
      ok: true, counts: [initial.total, hidden.total, restored.total], selected: [first, second],
      pausedDecoderUnchanged: true, retainedFilterOnBack: true, restoredMenuFocus: true,
      realMusicRequests: 0,
    });
  } catch (error) {
    await emit('failed', { ok: false, error: error.message, stack: error.stack });
  }
})();
