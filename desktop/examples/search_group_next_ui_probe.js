// Actual GTK/WebKit input on a sealed synthetic search and native null player.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
  const assert = (ok, message) => { if (!ok) throw Error(message); };
  const wait = async (check, message) => {
    for (let i = 0; i < 160; i++) {
      const value = await check();
      if (value) return value;
      await new Promise((resolve) => setTimeout(resolve, 60));
    }
    throw Error(message);
  };
  const invoke = (...args) =>
    (window.__CUMULUS_PROOF_INVOKE__ ?? window.__TAURI_INTERNALS__.invoke)(...args);
  const emit = (stage, info = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...info } });
  const button = (label, root = document) => [...root.querySelectorAll('button')]
    .find((node) => node.getAttribute('aria-label') === label || node.textContent.trim() === label);
  const click = (node) => {
    assert(node && !node.disabled, 'control is enabled');
    node.focus({ preventScroll: true });
    node.click();
  };
  const row = (id) => one(`.search-song-panel .track-row[data-track-id="${id}"]`);
  const title = (id) => row(id)?.querySelector('.track-row-title-text');
  const selected = () => all('.search-song-panel .track-row[aria-selected="true"]')
    .map((element) => element.dataset.trackId).join(',');
  const next = () => one('.track-action-menu [data-menu-key="next"]') ??
    all('[data-menu-key="next"]').find((node) => node.getClientRects().length && !node.closest('[inert]'));
  const play = () => all('[data-menu-key="play"]').find((node) =>
    node.getClientRects().length && !node.closest('[inert]'));
  const menuReady = () => {
    const node = next(), surface = node?.closest('[role="menu"]');
    return node && !node.disabled && surface &&
      !surface.matches('.menu-enter-active,.menu-leave-active') &&
      getComputedStyle(surface).opacity === '1';
  };
  const groupMenuReady = () => {
    const playButton = play(), surface = playButton?.closest('[role="menu"]');
    return menuReady() && playButton && !playButton.disabled &&
      [...surface.querySelectorAll('[data-menu-key]')].map((node) => node.dataset.menuKey)
        .join(',') === 'play,next';
  };
  const menuPaint = (node) => {
    const surface = node.closest('[role="menu"]');
    if (!surface) return [];
    const r = surface.getBoundingClientRect();
    const style = getComputedStyle(surface);
    const color = style.backgroundColor;
    const x = r.left + r.width / 2;
    const y = r.top + parseFloat(style.borderTopWidth) + parseFloat(style.paddingTop) / 2;
    assert(/^rgb\(\d+, \d+, \d+\)$/.test(color) &&
      document.elementFromPoint(x, y) === surface, 'opaque menu padding can be sampled');
    return [{ x, y, color }];
  };
  const ids = (queue) => queue.tracks.map(({ track }) => track.id).join(',');
  const bar = () => one('.playerbar');
  const tip = () => one('[data-search-queue-add-tip]');
  const tipAnimation = (name) => tip()?.getAnimations().find((animation) => animation.animationName === name);
  function visibleTip() {
    const node = tip(), queueButton = bar()?.querySelector('.queue-toggle');
    if (!node || !queueButton || Number(getComputedStyle(node).opacity) < 0.99 ||
      node.classList.contains('queue-more-tip-enter-active')) return;
    const bubble = node.getBoundingClientRect(), anchor = queueButton.getBoundingClientRect();
    const result = { label: node.textContent.trim(), gap: anchor.top - bubble.bottom };
    assert(result.label === '已添加至播放列表' && Math.abs(result.gap - 8) <= 2 &&
      node.parentElement === queueButton.parentElement, 'exact passive bubble above the queue control');
    assert(!one('.track-action-feedback'), 'successful insertion does not also show the old centered toast');
    return result;
  }
  let step = 'setup', epoch, stageCount = 0, before, baseline;
  const actionTrace = [];
  const extended = window.__CUMULUS_SEARCH_GROUP_EXTENDED__ === true;
  const playProof = window.__CUMULUS_SEARCH_GROUP_PLAY__ === true;
  let playStarted = false;
  const state = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  const native = async () => {
    const [checkedPlayer, checkedQueue] = await Promise.all([state(), queue()]);
    assert(checkedPlayer.queueRevision === checkedQueue.revision, 'consistent native queue revision');
    if (playProof && playStarted) {
      assert(checkedPlayer.mode === 'sequential' && !checkedPlayer.error &&
        checkedPlayer.track?.id === '101' && checkedQueue.current === checkedPlayer.entryId,
      'the explicit anchor is the actual current song');
    } else if (!extended || before) {
      assert(checkedPlayer.state === 'paused' && checkedPlayer.playId === before.playId &&
        checkedPlayer.entryId === before.entryId && checkedPlayer.position === before.position &&
        !checkedPlayer.error && checkedQueue.current === baseline.current, 'same paused decoder and clock');
    } else {
      assert(checkedPlayer.mode === 'random' && !checkedPlayer.error,
        'ordinary random mode is still active');
    }
    return { checkedPlayer, checkedQueue };
  };
  const capture = async (label, extra = {}) => {
    step = label;
    await emit(`ux_${String(++stageCount).padStart(2, '0')}_${label}`, {
      ...(await native()), selected: selected(), actionTrace, screenX, screenY, ratio: devicePixelRatio,
      ...extra,
    });
  };
  const target = (node) => {
    assert(node?.isConnected, 'actual visible target');
    const r = node.getBoundingClientRect();
    const x = r.left + r.width / 2, y = r.top + r.height / 2;
    assert(node.contains(document.elementFromPoint(x, y)), 'native target is pointer reachable');
    return { x: Math.floor((screenX + x) * devicePixelRatio),
      y: Math.floor((screenY + y) * devicePixelRatio) };
  };
  async function input(label, node, mouseButton = 1, ctrl = false) {
    let trusted = false;
    const observe = (event) => {
      if (event.isTrusted && event.button === mouseButton - 1 &&
          event.ctrlKey === ctrl && node.contains(event.target)) trusted = true;
    };
    document.addEventListener('pointerdown', observe, true);
    try {
      await capture(label, { pointer: { ...target(node), button: mouseButton, ctrl },
        paintSamples: menuPaint(node) });
      await wait(() => trusted, 'the browser received trusted X11 pointer and modifiers');
    } finally {
      document.removeEventListener('pointerdown', observe, true);
    }
  }

  try {
    await wait(() => one('.track-row') && button('列表循环', bar() ?? document)?.disabled === false,
      'authenticated daily page and native player ready');
    await emit('resize', { width: 1120, height: 760 });
    await wait(() => innerWidth === 1120 && innerHeight === 760, 'wide viewport');
    ({ epoch } = await invoke('account_snapshot'));
    await invoke('player_action', { epoch, action: { kind: 'mode', mode: extended ? 'random' : playProof ? 'sequential' : 'single_loop' } });
    if (!extended) {
      await invoke('play_selection', {
        epoch, selection: {
          kind: 'songs', ids: playProof ? ['701'] : ['701', '702'], index: 0, quality: 'standard',
        },
      });
      await wait(() => bar()?.dataset.state === 'playing' && Number(bar().dataset.position) > .2,
        'actual decoder advances');
      click(button('暂停', bar()));
      before = await wait(async () => {
        const view = await state();
        return view.state === 'paused' && view.position > .2 && view;
      }, 'native paused baseline');
      baseline = await queue();
      assert(ids(baseline) === (playProof ? '701' : '701,702') &&
        before.mode === (playProof ? 'sequential' : 'single_loop'), 'baseline source and mode');
    } else {
      const empty = await state();
      assert(empty.state === 'idle' && empty.playId === null && empty.entryId === null &&
        empty.mode === 'random' && ids(await queue()) === '', 'no startup selection or playback');
    }

    window.__searchFixtureAck = false;
    await emit('search-fixture', {
      mode: extended || playProof ? 'song_waterfall_group_next_extended' : 'song_waterfall_group_next',
    });
    await wait(() => window.__searchFixtureAck, 'synthetic search fixture active');
    const search = one('[aria-label="搜索音乐"]');
    search.value = '合成连续单曲';
    search.dispatchEvent(new Event('input', { bubbles: true }));
    search.form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await wait(() => one('.search-results-page h1')?.textContent === '合成连续单曲' &&
      one('#search-result-panel')?.getAttribute('aria-busy') === 'false' &&
      !one('.page-enter-active,.page-leave-active'), 'initial search route');
    click(one('[data-category="songs"]'));
    await wait(() => one('[data-category="songs"]')?.getAttribute('aria-selected') === 'true' &&
      one('#search-result-panel')?.getAttribute('aria-busy') === 'false' &&
      title('101') && title('104') && !one('.search-results-enter-active'),
    'actual Single Songs rows');
    const provides = one('#app').__vue_app__._context.provides;
    const core = provides[Reflect.ownKeys(provides)
      .find((key) => typeof key === 'symbol' && key.description === 'core-client')];
    const original = core.playerAction.bind(core);
    core.playerAction = async (owner, action) => {
      const trace = ['search_group_play', 'search_group_next_play', 'search_next_play'].includes(action.kind) ?
        { action, outcome: 'pending' } : undefined;
      if (trace) actionTrace.push(trace);
      try {
        const view = await original(owner, action);
        if (trace) trace.outcome = 'confirmed';
        return view;
      } catch (error) {
        if (trace) trace.outcome = error?.code ?? 'rejected';
        throw error;
      }
    };

    await input('select_first', title('103'));
    await wait(() => selected() === '103', 'first row selected');
    await input('select_blocked', title('102'), 1, true);
    await wait(() => selected() === '102,103', 'Ctrl includes blocked row');
    await input('select_third', title('101'), 1, true);
    await wait(() => selected() === '101,102,103', 'Ctrl preserves reversed selection');

    if (playProof) {
      await input('open_blocked_play_anchor', title('102'), 3);
      await wait(groupMenuReady, 'mixed group exposes Play and Next in that order');
      await input('blocked_play_menu', play());
      await wait(() => actionTrace[0]?.outcome === 'search_group_anchor_unavailable' &&
        document.body.innerText.includes('请从同组中可播放的歌曲打开菜单'),
      'the actual group Play command rejected a blocked anchor');
      assert(ids(await queue()) === '701' && !one('.playback-message button'),
        'blocked Play leaves the paused queue intact and cannot retry stale selection');
      assert(!tip(), 'blocked Play does not acknowledge an insertion');

      await input('open_new_play_anchor', title('101'), 3);
      await wait(groupMenuReady, 'Play on the clicked available song');
      await input('group_play_menu', play());
      const started = await wait(async () => {
        const view = await state();
        return actionTrace[1]?.outcome === 'confirmed' && view.state === 'playing' &&
          view.track?.id === '101' && view.playId !== before.playId && view.position > .3 && view;
      }, 'different clicked song started, after authorizing only that source');
      const startedQueue = await queue();
      assert(ids(startedQueue) === '701,103,101',
        'group is inserted without clearing and excludes the blocked non-anchor');
      playStarted = true;
      await capture('group_play_started', { addTip: await wait(visibleTip, 'group Play queue acknowledgement') });

      click(button('暂停', bar()));
      const paused = await wait(async () => {
        const view = await state();
        return view.state === 'paused' && view.playId === started.playId &&
          view.entryId === started.entryId && view.position >= started.position && view;
      }, 'current anchor is paused at a native clock position');
      await input('select_resume_first', title('104'));
      await wait(() => selected() === '104', 'ordinary click replaces the previous selection');
      await input('select_resume_current', title('101'), 1, true);
      await wait(() => selected() === '101,104', 'current track included after the other song');
      await input('open_current_play_anchor', title('101'), 3);
      await wait(groupMenuReady, 'same-anchor explicit group Play remains available');
      await input('current_play_menu', play());
      const resumed = await wait(async () => {
        const view = await state();
        return actionTrace[2]?.outcome === 'confirmed' && view.state === 'playing' &&
          view.playId === paused.playId && view.entryId === paused.entryId &&
          view.position > paused.position && view;
      }, 'current native decoder resumes instead of reopening or toggling');
      const final = await queue();
      assert(ids(final) === '701,103,104,101' &&
        final.current === resumed.entryId && final.revision !== startedQueue.revision,
      'the current entry moves inside selected display order without replacing the decoder');
      await capture('current_play_resumed', { addTip: await wait(visibleTip, 'group Play changes the queue while resuming') });
      core.playerAction = original;
      await emit('complete', { ok: true, stageCount, realAccountWrites: 0, realMusicRequests: 0 });
      return;
    }

    if (extended) {
      const empty = await state();
      await input('open_empty_anchor', title('101'), 3);
      await wait(groupMenuReady, 'group Next is enabled in the empty random queue');
      await input('empty_menu', next());
      const enter = await wait(() => tipAnimation('queue-add-tip-in'), 'add bubble entrance');
      const tipStarted = performance.now();
      const playing = await wait(async () => {
        const view = await state();
        return actionTrace[0]?.outcome === 'confirmed' && view.state === 'playing' &&
          view.track?.id === '101' && view.position > .2 && view;
      }, 'clicked song really starts instead of the first selected');
      const initial = await queue();
      assert(ids(initial) === '103,101' && initial.current === playing.entryId,
        'empty queue retains selected display order, excludes blocked, and starts anchor');
      assert(actionTrace[0].action.expected_play_id === null &&
        actionTrace[0].action.expected_queue_revision === empty.queueRevision,
      'empty command is bound to its exact native revision');
      const emptyTip = await wait(visibleTip, 'empty Next shows its queue acknowledgement');
      await capture('empty_started', { addTip: { ...emptyTip, enter: enter.effect.getTiming().duration } });
      const leave = await wait(() => tipAnimation('queue-add-tip-out'), 'add bubble exits after its dwell');
      const dwell = performance.now() - tipStarted;
      const leaveDuration = leave.effect.getTiming().duration;
      await wait(() => !tip(), 'closed add bubble is unmounted');

      const motionKey = 'cumulus.motion.v1';
      const oldMotion = localStorage.getItem(motionKey);
      localStorage.setItem(motionKey, 'disabled');
      dispatchEvent(new StorageEvent('storage', { key: motionKey, newValue: 'disabled', storageArea: localStorage }));
      await wait(() => document.documentElement.dataset.motion === 'reduced', 'app reduced motion');

      click(button('暂停', bar()));
      before = await wait(async () => {
        const view = await state();
        return view.state === 'paused' && view.playId === playing.playId &&
          view.entryId === playing.entryId && view.position >= playing.position && view;
      }, 'paused same decoder before inserting in random');
      baseline = await queue();
      await input('select_random_first', title('104'));
      await wait(() => selected() === '104', 'plain click replaces the old group');
      await input('select_random_anchor', title('105'), 1, true);
      await wait(() => selected() === '104,105', 'Ctrl includes the new anchor');
      await input('open_random_anchor', title('105'), 3);
      await wait(groupMenuReady, 'group Next remains enabled in paused random');
      await input('random_menu', next());
      await wait(async () => actionTrace[1]?.outcome === 'confirmed' &&
        ids(await queue()) === '103,101,104,105', 'the new group inserts in visible selected order');
      const reducedTip = await wait(visibleTip, 'random Next still shows the text in reduced motion');
      assert(!tip().getAnimations().length, 'reduced-motion queue acknowledgement does not animate');
      await capture('random_inserted', { addTip: { ...reducedTip, leave: leaveDuration, dwell, reduced: true } });
      if (oldMotion === null) localStorage.removeItem(motionKey);
      else localStorage.setItem(motionKey, oldMotion);
      dispatchEvent(new StorageEvent('storage', { key: motionKey, newValue: oldMotion, storageArea: localStorage }));
      core.playerAction = original;
      await emit('complete', { ok: true, stageCount, realAccountWrites: 0, realMusicRequests: 0 });
      return;
    }

    await input('open_blocked_anchor', title('102'), 3);
    await wait(groupMenuReady,
      'mixed group menu shows Play then Next');
    await input('blocked_anchor_menu', next());
    await wait(() => actionTrace[0]?.outcome && actionTrace[0].outcome !== 'pending',
      'actual blocked-anchor IPC settled');
    assert(actionTrace[0].outcome === 'search_group_anchor_unavailable' &&
      ids(await queue()) === '701,702',
    'blocked anchor is rejected natively, without starting another selection');
    await wait(() => document.body.innerText.includes('请从同组中可播放的歌曲打开菜单'),
      'blocked anchor has actionable feedback');
    assert(!one('.playback-message button'), 'a frozen selection has no retry button');
    assert(!tip(), 'blocked Next does not show a queue acknowledgement');

    await input('open_playable_anchor', title('101'), 3);
    await wait(groupMenuReady, 'ready selected-group menu');
    assert(selected() === '101,102,103', 'right-click in selection preserves the group');
    await input('group_menu', next());
    await wait(async () => actionTrace[1]?.outcome === 'confirmed' &&
      ids(await queue()) === '701,103,101,702', 'ordered group insert with a filtered member');
    for (const [index, clicked] of [[0, '102'], [1, '101']]) {
      const action = actionTrace[index].action;
      assert(action.kind === 'search_group_next_play' && action.anchor.track_id === clicked &&
        action.rows.map((target) => target.track_id).join(',') === '103,102,101' &&
        action.rows.every(({ receipt }) => /^[a-f0-9]{32}$/.test(receipt)) &&
        action.expected_play_id === before.playId &&
        action.expected_queue_revision === baseline.revision,
      'each native request preserves its exact anchor, selection order and queue authority');
    }
    await input('group_inserted', title('104'), 3);
    await wait(() => menuReady() && play() && selected() === '104',
      'outside right-click collapses to a normal single menu');
    await input('single_menu', next());
    await wait(async () => actionTrace[2]?.outcome === 'confirmed' &&
      ids(await queue()) === '701,104,103,101,702', 'unchanged single Next inserts only its anchor');
    assert(actionTrace.length === 3 && actionTrace[2].action.kind === 'search_next_play' &&
      actionTrace[2].action.id === '104', 'one single native Next command');
    await capture('single_inserted', { addTip: await wait(visibleTip, 'single search Next queue acknowledgement') });
    core.playerAction = original;
    await emit('complete', { ok: true, stageCount, realAccountWrites: 0, realMusicRequests: 0 });
  } catch (error) {
    await emit('failed', {
      ok: false, error: String(error), stack: error.stack, step, stageCount, actionTrace,
      selected: selected(), body: document.body.innerText.slice(0, 2800),
    });
  }
})();
