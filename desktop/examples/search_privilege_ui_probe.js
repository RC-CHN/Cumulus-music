// Real WebView, authoritative membership IPC and an isolated server response.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (condition, message) => { if (!condition) throw Error(message); };
  async function wait(read, message) {
    for (let attempt = 0; attempt < 175; attempt++) {
      const result = await read();
      if (result) return result;
      await sleep(60);
    }
    throw Error(message);
  }
  const invoke = (...args) =>
    (window.__CUMULUS_PROOF_INVOKE__ ?? window.__TAURI_INTERNALS__.invoke)(...args);
  const emit = (stage, info = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...info } });
  const query = '合成会员刷新';
  const initialScroll = 52;
  const panel = () => one('#search-result-panel');
  const card = () => one('[data-result-key="song:601"]');
  const heading = () => one('.search-results-page h1')?.textContent;
  const transition = () => one(
    '.page-enter-active,.page-leave-active,.search-results-enter-active,.search-results-leave-active',
  );
  const mv = () => card()?.querySelector('[data-result-action="mv"]')?.getAttribute('aria-label');
  const more = () => card()?.querySelector('[aria-label^="更多操作 "]');
  const labels = () => ({
    mv: mv() ?? null,
    vip: card()?.querySelector('.vip-tag')?.textContent ?? null,
    free: [...(card()?.querySelectorAll('.search-original-tag') ?? [])]
      .some((tag) => tag.textContent === '限免'),
  });
  const cardQuality = () => [...panel().querySelectorAll('.search-track-card[data-result-key]')]
    .map((row) => row.querySelector('[data-search-quality-badge]')?.dataset.searchQualityBadge ?? null);
  const coldQuality = ['hires', 'sq', 'jymaster', 'sky', null, null];
  const freshQuality = [null, 'sky', null, 'sq', 'hires', 'jyeffect'];
  const qualityMatches = (expected) => JSON.stringify(cardQuality()) === JSON.stringify(expected);
  const bar = () => one('.playerbar');
  const footer = (name) => bar()?.querySelector(`button[aria-label="${name}"]`);
  const played = (epoch) => invoke('player_snapshot', { epoch });
  const membership = (epoch) => invoke('member_rights', { epoch, refresh: true });
  const clock = (view) => ({
    epoch: view.epoch, revision: view.revision, state: view.state,
    playId: view.playId, entryId: view.entryId, trackId: view.track?.id ?? null,
    position: view.position, nativeSeeking: view.nativeSeeking, queueRevision: view.queueRevision,
    queueLen: view.queueLen, error: view.error,
  });
  const sameClock = (left, right) =>
    left.epoch === right.epoch && left.state === right.state && left.playId === right.playId &&
    left.entryId === right.entryId && left.trackId === right.trackId && left.position === right.position &&
    left.nativeSeeking === right.nativeSeeking && left.queueRevision === right.queueRevision &&
    left.queueLen === right.queueLen && left.error === right.error;
  // The actor's regular native poll is every 200 ms. The pause reply can
  // precede a final clock sample for already queued audio output.
  const pauseQuietMs = 450;
  let step = 'startup', count = 0, epoch, baseline, focused, original, scrollTop, targetPanel;
  let domCheckpoint, pausedClock, pauseSettlement;
  const refreshes = [];

  function layoutFrames(count) {
    return new Promise((resolve) => {
      let frame;
      const deadline = setTimeout(() => finish(false), 1500);
      function finish(painted) {
        clearTimeout(deadline);
        cancelAnimationFrame(frame);
        resolve(painted);
      }
      function next() {
        if (--count === 0) finish(true);
        else frame = requestAnimationFrame(next);
      }
      frame = requestAnimationFrame(next);
    });
  }

  function observeDom() {
    const current = panel(), active = document.activeElement;
    const viewport = current ? {
      clientHeight: current.clientHeight, scrollHeight: current.scrollHeight,
      capacity: current.scrollHeight - current.clientHeight, overflowY: getComputedStyle(current).overflowY,
    } : null;
    return {
      sameCard: !!original && original === card(),
      sameFocus: !!focused && focused === active,
      samePanel: !!targetPanel && targetPanel === current,
      activeTag: active?.tagName ?? null,
      activeLabel: active?.getAttribute('aria-label') ?? null,
      expectedScrollTop: scrollTop ?? null,
      scrollTop: current?.scrollTop ?? null,
      viewport,
      heading: heading() ?? null,
      transitionClass: transition()?.className ?? null,
      panelBusy: current?.getAttribute('aria-busy') ?? null,
    };
  }
  const routeReady = (view) => view.sameCard && view.samePanel &&
    view.heading === query && view.transitionClass === null && view.panelBusy === 'false';
  const domReady = (view) => routeReady(view) && view.sameFocus;
  const canSetInitialScroll = (view) => routeReady(view) &&
    view.viewport?.capacity >= initialScroll && ['auto', 'scroll', 'overlay'].includes(view.viewport.overflowY);

  async function capture(label) {
    step = label;
    domCheckpoint = { before: observeDom() };
    const native = await played(epoch);
    const live = domCheckpoint.after = observeDom();
    const sameDom = domReady(live) && Math.abs(scrollTop - live.scrollTop) < 1;
    assert(sameDom,
      'native refresh preserves live search card, focus, scroll and route');
    pausedClock = { baseline, observed: clock(native) };
    assert(sameClock(baseline, pausedClock.observed), 'search refresh must not change paused native playback');
    await emit(`ux_${String(++count).padStart(2, '0')}_${label}`, {
      labels: labels(), focused: document.activeElement.getAttribute('aria-label'),
      native, sameDom, scrollTop: panel().scrollTop, pausedClock, pauseSettlement,
      quality: cardQuality(),
      rowKeys: [...panel().querySelectorAll('.search-track-card[data-result-key]')]
        .map((row) => row.dataset.resultKey),
      refreshes,
    });
  }

  try {
    await wait(() => one('.track-row') && footer('列表循环')?.disabled === false, 'ready signed-in desktop');
    await emit('resize', { width: 1040, height: 610 });
    await wait(() => innerWidth === 1040 && innerHeight === 610, 'test window resized');
    ({ epoch } = await invoke('account_snapshot'));
    await invoke('play_selection', {
      epoch, selection: { kind: 'songs', ids: ['701', '702'], index: 0, quality: 'standard' },
    });
    const originalClock = await wait(async () => {
      const value = clock(await played(epoch));
      const pause = footer('暂停');
      return value.state === 'playing' && value.trackId === '701' && value.queueLen === 2 &&
        value.playId && value.entryId && value.position > .2 && value.error === null &&
        pause?.isConnected && !pause.disabled && value;
    }, 'actual native playback is advancing and pause is available');
    footer('暂停').click();
    step = 'paused_baseline';
    let quietSince = 0;
    let firstQuiet;
    baseline = await wait(async () => {
      const observed = clock(await played(epoch));
      const expectedPause = observed.state === 'paused' && observed.nativeSeeking === false &&
        observed.error === null && observed.epoch === originalClock.epoch && observed.trackId === '701' &&
        observed.playId === originalClock.playId && observed.entryId === originalClock.entryId &&
        observed.queueRevision === originalClock.queueRevision && observed.queueLen === originalClock.queueLen &&
        Number.isFinite(observed.position) && observed.position > .2;
      const now = performance.now();
      if (!expectedPause || !firstQuiet || !sameClock(firstQuiet, observed)) {
        firstQuiet = expectedPause ? observed : undefined;
        quietSince = now;
      }
      pausedClock = { baseline: firstQuiet ?? null, observed, quietMs: now - quietSince };
      return firstQuiet && now - quietSince >= pauseQuietMs &&
        BigInt(observed.revision) - BigInt(firstQuiet.revision) >= 2n && observed;
    }, 'native paused clock did not settle across two actor polls');
    pauseSettlement = { first: pausedClock.baseline, quietMs: pausedClock.quietMs };

    // Initial authoritative observation occurs before the search listener is attached.
    await membership(epoch);
    const provides = one('#app').__vue_app__._context.provides;
    const core = provides[Reflect.ownKeys(provides)
      .find((key) => typeof key === 'symbol' && key.description === 'core-client')];
    const refresh = core.searchRefreshPrivileges.bind(core);
    core.searchRefreshPrivileges = async (owner, receipt, observation) => {
      try {
        const result = await refresh(owner, receipt, observation);
        refreshes.push({
          ok: true, fromNativeObservation: !!observation,
          ids: result.value.rows.map((row) => row.id), mvId: result.value.rows[0]?.mvId,
        });
        return result;
      } catch (error) {
        refreshes.push({ ok: false, fromNativeObservation: !!observation });
        throw error;
      }
    };
    const field = one('[aria-label="搜索音乐"]');
    field.value = query;
    field.dispatchEvent(new Event('input', { bubbles: true }));
    field.form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await wait(() => heading() === query &&
      panel()?.getAttribute('aria-busy') === 'false' && more() &&
      !transition(),
      'cold comprehensive search and stable card');
    assert(mv()?.startsWith('查看替代 MV') && refreshes.length === 0,
      'restricted initial card comes from search, without extra privilege request');
    assert(qualityMatches(coldQuality), 'V3 track-only quality order, no bitrate-only Vivid, and VIP precedence');
    targetPanel = panel();
    original = card();
    focused = more();
    step = 'cold_layout';
    await wait(async () => {
      const before = observeDom();
      domCheckpoint = { before };
      if (!canSetInitialScroll(before)) return false;
      // The initial route can finish restoring its viewport after aria-busy
      // becomes false. Leave it untouched through both layout frame pairs.
      assert(await layoutFrames(4), 'initial search layout did not paint');
      await played(epoch);
      const after = domCheckpoint.after = observeDom();
      return canSetInitialScroll(after) && after.scrollTop === before.scrollTop &&
        after.viewport.clientHeight === before.viewport.clientHeight &&
        after.viewport.scrollHeight === before.viewport.scrollHeight;
    }, 'stable scrollable search layout');
    step = 'cold_baseline';
    domCheckpoint = { before: observeDom() };
    assert(canSetInitialScroll(domCheckpoint.before) && focused.isConnected, 'initial search control changed before assignment');
    focused.focus({ preventScroll: true });
    targetPanel.scrollTop = initialScroll;
    const assigned = domCheckpoint.assigned = observeDom();
    assert(domReady(assigned), 'initial scroll moved the active search card or More focus');
    assert(assigned.scrollTop > 0 && Math.abs(assigned.scrollTop - initialScroll) < 1,
      'initial scroll assignment was clamped');
    scrollTop = assigned.scrollTop;
    assert(await layoutFrames(2), 'initial search scroll did not paint');
    await played(epoch);
    const settled = domCheckpoint.after = observeDom();
    assert(domReady(settled) && settled.scrollTop === scrollTop,
      'initial scroll or focus changed after assignment');
    await capture('cold_restricted');

    await membership(epoch);
    await wait(() => refreshes.length === 1 && mv()?.startsWith('播放 MV') &&
      labels().vip === 'VIP' && labels().free && qualityMatches(freshQuality),
      'native rights event updates the ordinary preview and replaces/removes quality tags');
    assert(refreshes[0].fromNativeObservation && refreshes[0].ok &&
      refreshes[0].mvId === '951' && refreshes[0].ids.join(',') === '601,602,603,604,605,606',
      'precisely six ordinary rows and original MV were projected by Rust');
    await capture('changed_tuple');

    await membership(epoch);
    await sleep(480);
    assert(refreshes.length === 1 && mv()?.startsWith('播放 MV') && qualityMatches(freshQuality),
      'same tuple does not refresh');
    await capture('same_tuple');

    await membership(epoch);
    await wait(() => refreshes.length === 2, 'rights event read failure observed');
    await sleep(480);
    assert(refreshes.length === 2 && refreshes[1].fromNativeObservation && !refreshes[1].ok &&
      mv()?.startsWith('播放 MV') && labels().free && qualityMatches(freshQuality),
      'failed privilege read retains prior labels and does not retry');
    await capture('failed_privilege_read');

    const playAll = card().closest('.search-result-section')
      .querySelector('[aria-label="播放全部搜索单曲"]');
    assert(!playAll.disabled, 'explicit Play All remains available after the read failure');
    playAll.click();
    const playing = await wait(async () => {
      const value = await played(epoch);
      return value.state === 'playing' && value.track?.id === '601' &&
        value.playId !== baseline.playId && value.position > .15 && value;
    }, 'explicit Play All starts the first authorized result');
    const queue = await invoke('player_queue', { epoch });
    const playAllIds = queue.tracks.map((row) => row.track.id);
    assert(playAllIds.join(',') === '601,602,603,604,605,606' && queue.current === playing.entryId,
      'explicit Play All uses the six refreshed rows, not the full raw block');
    await emit('ux_05_play_all_refreshed', { native: playing, playAllIds });
    count++;

    await emit('complete', {
      ok: true, stageCount: count, refreshes, realMusicRequests: 0, realAccountWrites: 0,
    });
  } catch (error) {
    await emit('failed', { step, count, error: String(error), refreshes, labels: labels(),
      domCheckpoint, currentDom: observeDom(), pausedClock });
  }
})();
