// Same live search visit, four assignments from the sealed signed PC full batch.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])];
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (value, message) => { if (!value) throw Error(message); };
  const wait = async (read, message) => {
    for (let n = 0; n < 240; n++) { if (await read()) return; await sleep(50); }
    throw Error(message);
  };
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, data = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...data } });
  const click = (node) => {
    assert(node && !node.disabled, 'enabled playback control');
    node.focus({ preventScroll: true });
    node.click();
  };
  const panel = () => one('#search-result-panel');
  const cover = () => one('[data-result-key="song:1"] .search-track-cover');
  const keys = (root) => root ? all('[data-result-key]', root).map((node) => node.dataset.resultKey) : [];
  const best = () => keys(one('.search-best-section'));
  const ready = () => panel()?.getAttribute('aria-busy') === 'false'
    && !one('.page-enter-active,.page-leave-active,.search-results-leave-active');
  const sectionFor = (kind) =>
    one(`[data-result-section^="search_block_${kind === 'mv' ? 'video' : 'artist'}:"]`);
  const cohorts = [
    { mode: 'c', expanded: false, dense: false, layout: null },
    { mode: 'density', expanded: false, dense: true, layout: 'compact_grid' },
    { mode: 'expanded', expanded: true, dense: false, layout: 'carousel' },
    { mode: 'both', expanded: true, dense: true, layout: 'compact_carousel' },
  ];
  const expectedKeys = (kind, start, length) =>
    Array.from({ length }, (_, index) => `${kind}:${start + index}`);
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  function preview(video) {
    const section = sectionFor(video ? 'mv' : 'artist');
    if (!section) return null;
    const carousel = one('.search-carousel', section);
    const grid = one('.search-result-grid[data-loaded-count]', section);
    if ((!carousel && !grid) || (carousel && grid)) return null;
    const rail = carousel ? one('.search-carousel-rail', carousel) : grid;
    const viewport = carousel ? one('.search-carousel-viewport', carousel) : grid;
    if (!rail || !viewport || !viewport.clientWidth) return null;
    const style = getComputedStyle(rail);
    const tracks = style.gridTemplateColumns.trim().split(/\s+/);
    const cells = all('.search-grid-cell', rail);
    const visible = cells.filter((cell) => !cell.hasAttribute('inert'));
    const top = visible.map((cell) => cell.getBoundingClientRect().top);
    return {
      carousel: !!carousel, compact: (carousel ?? grid).classList.contains('search-preview-compact'),
      rowPreview: grid?.dataset.preview === 'true', width: viewport.clientWidth,
      candidateCount: carousel ? cells.length : Number(grid.dataset.loadedCount),
      columns: carousel ? Number(carousel.dataset.previewColumns)
        : tracks.every((track) => /^\d+(?:\.\d+)?px$/.test(track)) ? tracks.length : 0,
      columnGap: style.columnGap, firstTrackPx: visible[0]?.getBoundingClientRect().width ?? 0,
      allKeys: keys(rail), visibleKeys: visible.flatMap(keys),
      singleRow: top.length > 0 && Math.max(...top) - Math.min(...top) < 1,
    };
  }
  function matchesPreview(view, { expanded, dense }, video) {
    if (!view) return false;
    const gap = dense ? 16 : 20;
    // M2118 BaseGrid and GridCarousel use different width units and numerators.
    const span = video ? expanded || dense ? 3 : 4 : dense ? 2 : 3;
    const unit = expanded || video || !dense ? 66 : 64;
    const columns = Math.max(1, Math.min(Math.floor(18 / span),
      Math.floor((view.width + (expanded ? 20 : gap)) / (span * unit))));
    const count = expanded ? 10 : 6;
    const visible = expectedKeys(video ? 'mv' : 'artist', video ? 300 : 200, Math.min(count, columns));
    return view.carousel === expanded && view.compact === dense && view.rowPreview === !expanded
      && view.candidateCount === count && view.columns === columns && view.columnGap === `${gap}px`
      && Math.abs(view.firstTrackPx - ((view.width + gap) / columns - gap)) < 1.1
      && same(view.visibleKeys, visible) && same(view.allKeys, expanded
        ? expectedKeys(video ? 'mv' : 'artist', video ? 300 : 200, count) : visible)
      && view.singleRow;
  }
  let epoch, paused, top, core, originalProjection, stopEvents, step = 'startup';
  const events = [], commands = [], projections = [], geometry = [], restoredViews = [];
  const native = () => invoke('player_snapshot', { epoch });
  const presentation = () => invoke('pc_presentation', { epoch });
  const paint = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const samePaused = (state) => state.state === 'paused' && state.playId === paused.playId
    && state.entryId === paused.entryId && state.queueRevision === paused.queueRevision
    && Math.abs(state.position - paused.position) < .04;
  const assigned = (view, cohort) => view.epoch === epoch && !view.searchFooter
    && view.searchExpanded === cohort.expanded && view.searchDense === cohort.dense;
  const bounds = (node) => {
    const rect = node?.getBoundingClientRect();
    return rect ? { left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom } : null;
  };
  function sectionFrame(kind) {
    const scroller = panel(), section = sectionFor(kind);
    if (!scroller || !section) return null;
    const { left, top } = scroller.getBoundingClientRect();
    const x = left + scroller.clientLeft, y = top + scroller.clientTop;
    const viewport = {
      left: Math.max(0, x), top: Math.max(0, y),
      right: Math.min(innerWidth, x + scroller.clientWidth),
      bottom: Math.min(innerHeight, y + scroller.clientHeight),
    };
    return {
      viewport, windowSize: { width: innerWidth, height: innerHeight }, scrollTop: scroller.scrollTop,
      section: bounds(section), heading: bounds(one(':scope > header', section)),
      cards: all('.search-grid-cell:not([inert]) [data-result-key]', section)
        .map((node) => ({ key: node.dataset.resultKey, bounds: bounds(node) })),
    };
  }
  function inside(rect, viewport) {
    return !!rect && rect.left < rect.right && rect.top < rect.bottom
      && rect.left >= viewport.left - .5 && rect.right <= viewport.right + .5
      && rect.top >= viewport.top + 1 && rect.bottom <= viewport.bottom - 1;
  }
  function fullyVisible(frame, expected) {
    return !!frame && inside(frame.section, frame.viewport) && inside(frame.heading, frame.viewport)
      && same(frame.cards.map(({ key }) => key), expected)
      && frame.cards.every(({ bounds }) => inside(bounds, frame.viewport));
  }
  async function captureSection(stage, kind, expected) {
    step = stage + '_' + kind;
    const scroller = panel(), frame = sectionFrame(kind);
    assert(frame?.section, 'preview section available for screenshot ' + step);
    const center = ({ top, bottom }) => (top + bottom) / 2;
    scroller.scrollTop = frame.scrollTop + center(frame.section) - center(frame.viewport);
    await paint();
    await wait(() => fullyVisible(sectionFrame(kind), expected), 'whole preview inside the WebView ' + step);
    const visible = sectionFrame(kind);
    assert(document.activeElement === cover(), 'scrolling to preview does not steal song focus');
    assert(samePaused(await native()), 'scrolling to preview leaves paused native playback intact');
    window.__searchFooterRecorded = '';
    await emit(step, { kind, layoutStage: stage, frame: visible, focusPreserved: true });
    await wait(() => window.__searchFooterRecorded === step, 'native actor and PNG receipt ' + step);
    const after = sectionFrame(kind);
    assert(document.activeElement === cover() && fullyVisible(after, expected)
      && Math.abs(after.scrollTop - visible.scrollTop) < 1,
      'preview stayed within the same painted viewport until capture receipt');
  }
  async function refresh(cohort, nonce) {
    const before = await presentation();
    await emit('pc-presentation-fixture', { nonce, mode: cohort.mode });
    await wait(() => window.__pcPresentationFixtureAck?.nonce === nonce, 'native signed refresh acknowledgement');
    const ack = window.__pcPresentationFixtureAck;
    assert(ack.beforeFull === nonce && ack.afterFull === nonce + 1 && ack.result?.Ok,
      'exactly one successful full assignment per cohort transition');
    let after;
    await wait(async () => {
      after = await presentation();
      return after.revision !== before.revision && assigned(after, cohort)
        && events.some((event) => event.epoch === epoch && event.revision === after.revision);
    }, 'native event and local snapshot agree on independent experiment groups');
    commands.push({ mode: cohort.mode, before, after, beforeFull: ack.beforeFull, afterFull: ack.afterFull });
    await wait(() => projections.some((item) => item.revision === after.revision
      && item.receiptPresent && item.replyRevision === after.revision && item.layout === cohort.layout),
    'same-page production receipt returns the expected layout');
  }
  async function capture(cohort, index) {
    step = cohort.mode;
    await wait(() => ready() && matchesPreview(preview(false), cohort, false)
      && matchesPreview(preview(true), cohort, true)
      && same(best(), expectedKeys('artist', 100, cohort.expanded ? 3 : 2)),
    'official one-row or carousel geometry, candidates, and best-match count');
    await paint();
    const artist = preview(false), mv = preview(true);
    assert(matchesPreview(artist, cohort, false) && matchesPreview(mv, cohort, true),
      'official geometry remains after paint');
    assert(document.activeElement === cover() && Math.abs(panel().scrollTop - top) < 1,
      'surviving song cover focus and scroll stay in the same visit');
    const state = await native();
    assert(samePaused(state) && state.track?.id === '1' && state.position > 2,
      'cohort update leaves the real paused native entry, clock and queue unchanged');
    const bar = one('.playerbar');
    assert(bar?.dataset.state === 'paused' && bar.dataset.playId === state.playId
      && bar.dataset.trackId === '1', 'rendered player still matches the paused native entry');
    const publicPresentation = await presentation();
    assert(assigned(publicPresentation, cohort) && !one('.search-track-hint,.search-track-reasons'),
      'footer remains independently in control');
    const stage = `ux_${String(index + 1).padStart(2, '0')}_${cohort.mode}`;
    geometry.push({ mode: cohort.mode, artist, mv });
    window.__searchFooterRecorded = '';
    await emit(stage, { mode: cohort.mode, artist, mv, bestKeys: best(), publicPresentation,
      focusPreserved: true, scrollTop: panel().scrollTop, noOverflow: document.documentElement.scrollWidth <= innerWidth });
    await wait(() => window.__searchFooterRecorded === stage, 'native actor and PNG receipt ' + stage);
    try {
      // A single window need not fit both sections. Capture each full section
      // before restoring the original visit for the next signed update.
      await captureSection(stage, 'artist', artist.visibleKeys);
      await captureSection(stage, 'mv', mv.visibleKeys);
    } finally {
      panel().scrollTop = top;
      if (document.activeElement !== cover()) cover()?.focus({ preventScroll: true });
      await paint();
    }
    step = stage + '_restored';
    assert(document.activeElement === cover() && Math.abs(panel().scrollTop - top) < 1,
      'restore the original focus and scroll before the next cohort');
    restoredViews.push({ stage, scrollTop: panel().scrollTop, focusKey: cover().closest('[data-result-key]').dataset.resultKey });
  }
  try {
    await wait(async () => {
      const account = await invoke('account_snapshot');
      if (account.status !== 'authenticated' || !one('.track-row')) return false;
      epoch = account.epoch;
      return true;
    }, 'authoritative synthetic account restore');
    const provides = one('#app').__vue_app__._context.provides;
    core = provides[Reflect.ownKeys(provides).find((key) => typeof key === 'symbol' && key.description === 'core-client')];
    assert(core, 'production typed adapter');
    stopEvents = await core.onPcPresentationChanged((event) => events.push(event));
    const input = one('[aria-label="搜索音乐"]');
    input.value = '合成 V3';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    input.form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await wait(() => ready() && cover() && preview(false) && preview(true), 'the one comprehensive search');
    click(cover());
    await wait(async () => {
      const state = await native();
      return state.state === 'playing' && state.position > 2
        && cover()?.getAttribute('aria-label')?.startsWith('暂停') && Number(one('.playerbar')?.dataset.position) > 2;
    }, 'real libmpv null-output clock');
    click(cover());
    await wait(async () => {
      const state = await native();
      if (state.state !== 'paused' || one('.playerbar')?.dataset.state !== 'paused') return false;
      paused = state;
      return true;
    }, 'pause the loaded native entry');
    cover().focus({ preventScroll: true });
    panel().scrollTop = 70;
    await wait(() => panel().scrollTop === 70, 'nonzero search scroll checkpoint');
    top = panel().scrollTop;
    await capture(cohorts[0], 0);
    originalProjection = core.searchPresentation;
    core.searchPresentation = async (...args) => {
      const reply = await originalProjection.apply(core, args);
      projections.push({ epoch: args[0], receiptPresent: !!args[1], revision: args[2],
        replyRevision: reply.value.revision, layout: reply.value.previewLayout });
      return reply;
    };
    for (let index = 1; index < cohorts.length; index++) {
      step = 'refresh_' + cohorts[index].mode;
      await refresh(cohorts[index], index);
      await capture(cohorts[index], index);
    }
    await emit('complete', { ok: true, previewDensityVerified: true, stageCount: cohorts.length,
      sectionStageCount: 2 * cohorts.length, restoredViews,
      refreshCommands: commands, presentationEvents: events, receiptProjections: projections,
      realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', { ok: false, step, error: String(error), stack: error.stack,
      current: { artist: preview(false), mv: preview(true), bestKeys: best() },
      sectionFrames: { artist: sectionFrame('artist'), mv: sectionFrame('mv') }, restoredViews,
      geometry, refreshCommands: commands, receiptProjections: projections });
  } finally {
    stopEvents?.();
    if (core && originalProjection) core.searchPresentation = originalProjection;
  }
})();
