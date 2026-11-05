// Real App subscription and native receipt projection; sealed signed PC server.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const all = (selector, root = document) => [...root.querySelectorAll(selector)];
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (value, message) => { if (!value) throw Error(message); };
  const wait = async (read, message) => {
    for (let n = 0; n < 240; n++) { if (await read()) return; await sleep(50); }
    throw Error(message);
  };
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...data } });
  const click = (node) => { assert(node && !node.disabled, 'enabled UI control'); node.focus({ preventScroll: true }); node.click(); };
  const panel = () => one('#search-result-panel');
  const card = (id) => one(`[data-result-key="song:${id}"]`);
  const cover = (id) => card(id)?.querySelector('.search-track-cover');
  const footerCount = () => all('.search-track-hint,.search-track-reasons').length;
  const ready = () => panel()?.getAttribute('aria-busy') === 'false' && !one('.page-enter-active,.page-leave-active');
  const best = () => one('.search-best-section');
  const regular = () => one('[data-result-section^="search_block_artist:"]');
  const keys = (root) => root ? all('[data-result-key]', root).map((node) => node.dataset.resultKey) : [];
  const bar = () => {
    const node = one('.playerbar');
    return node && { state: node.dataset.state, playId: node.dataset.playId ?? null,
      trackId: node.dataset.trackId ?? null, position: Number(node.dataset.position),
      title: one('.playerbar .now-copy button')?.textContent.trim() ?? null };
  };
  let step = 'startup', epoch, count = 0, nonce = 0, paused;
  let core, originalProjection, stopEvents, holdProjection = false, heldProjection = false, releaseProjection;
  const events = [], commands = [], projections = [];
  const native = () => invoke('player_snapshot', { epoch });
  const presentation = () => invoke('pc_presentation', { epoch });
  const samePaused = (state) => state.state === 'paused' && state.playId === paused.playId
    && state.entryId === paused.entryId && state.queueRevision === paused.queueRevision
    && Math.abs(state.position - paused.position) < .04;
  function layout(treatment) {
    assert(footerCount() === (treatment ? 3 : 0), 'all three footer lanes match the native cohort');
    assert(keys(best()).join(',') === (treatment ? 'artist:100,artist:101,artist:102' : 'artist:100,artist:101'), 'best-match projection keeps official first three/two');
    const expected = Array.from({ length: treatment ? 10 : 6 }, (_, index) => `artist:${200 + index}`);
    assert(keys(regular()).join(',') === expected.join(','), 'ordinary preview retains ten/six bounded candidates');
    const rail = regular()?.querySelector('.search-carousel-rail');
    assert(treatment ? rail && getComputedStyle(rail).gap === '16px' : !rail, 'dense carousel is derived from the actual global assignment');
  }
  async function refresh(mode) {
    const before = await presentation();
    const command = ++nonce;
    await emit('pc-presentation-fixture', { nonce: command, mode });
    await wait(() => window.__pcPresentationFixtureAck?.nonce === command, 'native signed refresh acknowledgement');
    const ack = window.__pcPresentationFixtureAck;
    assert(ack.afterFull === ack.beforeFull + 1, 'one actual full assignment per native refresh');
    let after;
    if (mode === 'error') {
      after = await presentation();
      assert(JSON.stringify(after) === JSON.stringify(before), 'official catch retains the previous public view');
    } else {
      await wait(async () => {
        after = await presentation();
        return after.revision !== before.revision && after.searchFooter === (mode === 't1')
          && after.searchExpanded === (mode === 't1') && after.searchDense === (mode === 't1')
          && events.some((event) => event.epoch === epoch && event.revision === after.revision);
      }, 'real native presentation event and local public snapshot agree');
    }
    commands.push({ mode, before, after, beforeFull: ack.beforeFull, afterFull: ack.afterFull,
      nativeRefreshOk: !!ack.result?.Ok });
    return after;
  }
  async function capture(label) {
    step = label;
    await wait(() => !one('.page-enter-active,.page-leave-active'), 'page settled ' + label);
    const state = await native();
    if (paused) assert(samePaused(state), 'presentation leaves paused native playback untouched');
    await wait(() => {
      const current = bar();
      return current?.state === state.state && (!state.track || current.playId === state.playId
        && current.trackId === state.track.id && current.title === state.track.name && current.position > 2
        && (!paused || Math.abs(current.position - state.position) < .04));
    }, 'typed DOM player bar agrees with native state');
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await sleep(120);
    const stage = `ux_${String(++count).padStart(2, '0')}_${label}`;
    window.__searchFooterRecorded = '';
    const focused = document.activeElement;
    await emit(stage, { label, footerCount: footerCount(), bestKeys: keys(best()), regularKeys: keys(regular()),
      denseGap: regular()?.querySelector('.search-carousel-rail')?.style.gap ?? null,
      scrollTop: panel()?.scrollTop ?? null, renderedTracks: all('.search-track-card').length,
      focusCard: focused?.closest('[data-result-key]')?.dataset.resultKey ?? null,
      focusAction: focused?.dataset.resultAction ?? null, focusCover: focused?.classList.contains('search-track-cover') ?? false,
      focusSearch: focused === one('[aria-label="搜索音乐"]'), reasonTitle: one('.search-reason-page h1')?.textContent ?? null,
      publicPresentation: await presentation(), presentationEvents: [...events], refreshCommands: [...commands],
      receiptProjections: [...projections], playerbar: bar(), heldProjection,
      noOverflow: document.documentElement.scrollWidth <= innerWidth });
    await wait(() => window.__searchFooterRecorded === stage, 'native actor and PNG receipt recorded ' + stage);
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
    originalProjection = core.searchPresentation;
    core.searchPresentation = async (...args) => {
      const reply = await originalProjection.apply(core, args);
      projections.push({ epoch: args[0], receiptPresent: !!args[1], revision: args[2],
        replyRevision: reply.value.revision, sections: reply.value.sections.length });
      if (holdProjection) {
        holdProjection = false;
        heldProjection = true;
        await new Promise((resolve) => { releaseProjection = resolve; });
        heldProjection = false;
      }
      return reply;
    };
    await wait(async () => { const current = await presentation(); return current.searchFooter && current.searchExpanded && current.searchDense; }, 'native startup signed full assignment');
    const input = one('[aria-label="搜索音乐"]');
    input.value = '合成 V3';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    input.form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await wait(() => ready() && regular() && keys(regular()).length === 10 && footerCount() === 3, 'single comprehensive search in treatment');
    layout(true);
    await capture('initial');
    click(cover('1'));
    await wait(async () => { const state = await native(); return state.state === 'playing' && state.position > 2
      && cover('1')?.getAttribute('aria-label')?.startsWith('暂停') && bar()?.position > 2; }, 'real null-output clock and rendered song cover');
    click(cover('1'));
    await wait(async () => { const state = await native(); if (state.state !== 'paused') return false; paused = state; return true; }, 'pause the real decoder');
    panel().scrollTop = 70;
    await wait(() => panel().scrollTop === 70, 'nonzero search viewport');
    const top = panel().scrollTop;
    await capture('paused');
    const reason = card('3').querySelector('[data-result-action="reason:0"]');
    reason.focus({ preventScroll: true });
    await refresh('c');
    await wait(() => footerCount() === 0 && keys(best()).length === 2 && keys(regular()).length === 6
      && document.activeElement === cover('3'), 'removed reason falls back to its own surviving cover');
    layout(false);
    assert(panel().scrollTop === top, 'control update retains scroll');
    await capture('current_control');
    input.focus({ preventScroll: true });
    await refresh('t1');
    await wait(() => footerCount() === 3 && keys(best()).length === 3 && keys(regular()).length === 10, 'native receipt restores all three presentation branches');
    layout(true);
    assert(document.activeElement === input && panel().scrollTop === top, 'new focus and viewport are not stolen');
    await capture('current_treatment');
    await refresh('error');
    layout(true);
    assert(document.activeElement === input && panel().scrollTop === top, 'failed optional read leaves current browsing alone');
    await capture('temporary_failure');
    click(card('3').querySelector('[data-result-action="reason:0"]'));
    await wait(() => one('.search-reason-page h1')?.textContent === '合成推荐详情', 'native recommendation detail');
    await capture('detail');
    await refresh('c');
    assert(one('.search-reason-page h1')?.textContent === '合成推荐详情', 'global assignment does not navigate the current route');
    await capture('detail_control');
    holdProjection = true;
    click(one('[aria-label="返回上一页"]'));
    await wait(() => heldProjection && one('.search-results-page'), 'Back awaits actual native local receipt projection');
    assert(footerCount() === 0 && all('.search-track-card').length === 0, 'old granted rows are never flashed during Back projection');
    await capture('back_pending');
    releaseProjection();
    await wait(() => ready() && all('.search-track-card').length === 3 && keys(regular()).length === 6
      && document.activeElement === cover('3'), 'Back publishes the current control projection and scoped fallback focus');
    layout(false);
    assert(panel().scrollTop === top, 'Back restores the same scrolled visit');
    await capture('back_control');
    await refresh('t1');
    await wait(() => footerCount() === 3 && keys(best()).length === 3 && keys(regular()).length === 10, 'same cached visit receives treatment without another query');
    layout(true);
    assert(document.activeElement === cover('3') && panel().scrollTop === top, 'returning footer does not move surviving focus or scroll');
    await capture('back_treatment');
    await emit('complete', { ok: true, pcPresentationVerified: true, stageCount: count,
      refreshCommands: commands, presentationEvents: events, receiptProjections: projections,
      realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', { ok: false, step, error: String(error), stack: error.stack,
      page: one('.main-content')?.innerText.slice(0, 1500), refreshCommands: commands, receiptProjections: projections });
  } finally {
    releaseProjection?.();
    stopEvents?.();
    if (core && originalProjection) core.searchPresentation = originalProjection;
  }
})();
