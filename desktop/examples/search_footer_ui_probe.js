// Four sealed cohorts; genuine PC assignment transport, WebView and null libmpv.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (value, message) => { if (!value) throw Error(message); };
  const wait = async (test, message) => {
    for (let n = 0; n < 200; n++) {
      if (await test()) return;
      await sleep(60);
    }
    throw Error(message);
  };
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const click = (node) => {
    assert(node && !node.disabled, 'missing or disabled control');
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    node.click();
  };
  const cards = () => all('.search-track-card');
  const footerCount = () => all('.search-track-card .search-track-hint, .search-track-card .search-track-reasons').length;
  const ready = () => one('#search-result-panel')?.getAttribute('aria-busy') === 'false'
    && !one('.page-enter-active,.page-leave-active,.search-results-leave-active');
  const mode = window.__searchFooterCase;
  let step = 'startup', count = 0, epoch, firstOrdinaryFooterCount, initialPresentation;
  const native = () => invoke('player_snapshot', { epoch });
  const presentation = () => invoke('pc_presentation', { epoch });
  const ordinaryReady = () => ready() && one('.search-results-page h1')?.textContent === '合成 V3'
    && one('[data-category="all"]')?.getAttribute('aria-selected') === 'true' && cards().length === 3;
  const bar = () => {
    const node = one('.playerbar');
    return node && { state: node.dataset.state, playId: node.dataset.playId ?? null,
      trackId: node.dataset.trackId ?? null, position: Number(node.dataset.position),
      title: one('.playerbar .now-copy button')?.textContent.trim() ?? null };
  };
  async function playerWhen(test, message) {
    let result;
    await wait(async () => { result = await native(); return test(result); }, message);
    return result;
  }
  function verifyFooters() {
    assert(cards().map((card) => card.dataset.resultKey).join(',') === 'song:1,song:2,song:3', 'same three ordinary V3 song cards');
    assert(!one('[href*="example.invalid/private"]'), 'opaque reason URL never reaches the DOM');
    if (mode !== 't1') {
      assert(footerCount() === 0, 'control, unknown or failed assignment has no song footer');
      return;
    }
    const [first, second, third] = cards();
    assert(footerCount() === 3, 'exactly three song footers');
    assert(first.querySelector('.search-track-hint')?.dataset.tooltip === '🎵合成歌词片段'
      && first.querySelector('.search-track-hint mark')?.textContent === '合成', 'UTF-16 lyric hint');
    assert(second.querySelector('.search-track-hint')?.textContent.trim() === '前缀合成 V3别名'
      && second.querySelector('.search-track-hint mark')?.textContent === '合成 V3', 'invalid lyric span falls back to authoritative alias');
    const reasons = third.querySelector('.search-track-reasons');
    assert(reasons?.querySelectorAll('button').length === 2
      && reasons.querySelector('span')?.textContent === '不可跳转', 'two supported reason actions and one inert label');
  }
  async function capture(label, expected) {
    step = label;
    await wait(() => !one('.page-enter-active,.page-leave-active')
      && all('.search-result-panel,.search-reason-page').every((node) => node.getAnimations().every((animation) => animation.playState !== 'running' && !animation.pending)), 'settled page ' + label);
    const observed = await native();
    if (expected) assert(expected(observed), 'native state before ' + label);
    await wait(() => {
      const rendered = bar();
      return rendered?.state === observed.state && (!observed.track ||
        rendered.playId === observed.playId && rendered.trackId === observed.track.id &&
        rendered.title === observed.track.name && rendered.position > .2);
    }, 'rendered player bar agrees with native playback before ' + label);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await sleep(120);
    const stage = `ux_${String(++count).padStart(2, '0')}_${label}`;
    window.__searchFooterRecorded = '';
    await emit(stage, {
      footerCase: mode, label, footerCount: footerCount(),
      cardKeys: cards().map((card) => card.dataset.resultKey),
      query: one('.search-results-page h1')?.textContent ?? null,
      category: one('[data-category][aria-selected="true"]')?.dataset.category ?? null,
      reasonTitle: one('.search-reason-page h1')?.textContent ?? null,
      focusAction: document.activeElement?.dataset.resultAction ?? null,
      noOverflow: document.documentElement.scrollWidth <= innerWidth,
      observedPlayId: observed.playId, observedState: observed.state,
      playerbar: bar(),
      ...(label === 'results' ? { firstOrdinaryFooterCount, initialPresentation } : {}),
    });
    // An emit reply precedes the spawned host actor snapshot. This receipt is
    // emitted only after native JSON persistence (and optional PNG capture).
    await wait(() => window.__searchFooterRecorded === stage, 'native recorded stage ' + stage);
  }
  try {
    assert(['t1', 'c', 'unknown', 'error'].includes(mode), 'explicit synthetic footer cohort');
    await wait(() => one('.track-row') && one('[aria-label="搜索音乐"]'), 'authenticated initial page');
    ({ epoch } = await invoke('account_snapshot'));
    const input = one('[aria-label="搜索音乐"]');
    input.value = '合成 V3';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    input.form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await wait(ordinaryReady, 'ordinary V3 search');
    firstOrdinaryFooterCount = footerCount();
    if (mode === 't1') {
      // aria-busy tracks the ordinary result read. The signed presentation
      // event can still reproject that result after the rows first render.
      step = 'initial_presentation';
      await wait(() => ordinaryReady() && footerCount() === 3, 'three signed song footers rendered');
    }
    verifyFooters();
    // Observe only after the DOM check so a snapshot cannot drive readiness.
    initialPresentation = await presentation();
    assert(initialPresentation.epoch === epoch && initialPresentation.searchFooter === (mode === 't1'),
      'rendered footer cohort agrees with the current native presentation');
    await capture('results', (view) => view.state === 'idle');

    const cover = () => one('[data-result-key="song:1"] .search-track-cover');
    click(cover());
    const playing = await playerWhen((view) => view.state === 'playing' && view.track?.id === '1'
      && view.position > .25, 'first card plays through native null output');
    await wait(() => cover()?.getAttribute('aria-label') === '暂停 综合 V3 合成歌曲', 'current search cover pauses');
    verifyFooters();
    await capture('playing', (view) => view.playId === playing.playId && view.state === 'playing' && view.position > .25);
    click(cover());
    const paused = await playerWhen((view) => view.playId === playing.playId && view.state === 'paused', 'same decoder pauses');
    assert(paused.entryId === playing.entryId && paused.queueRevision === playing.queueRevision, 'pause retains queue and entry');
    const stillPaused = (view) => view.state === 'paused' && view.playId === paused.playId
      && view.entryId === paused.entryId && view.queueRevision === paused.queueRevision
      && Math.abs(view.position - paused.position) < .04;
    verifyFooters();
    await capture('paused', stillPaused);

    if (mode === 't1') {
      click(one('[data-result-key="song:3"] [data-result-action="reason:0"]'));
      await wait(() => one('.search-reason-page h1')?.textContent === '合成推荐详情'
        && one('.search-reason-page .track-row')?.textContent.includes('合成推荐曲目'), 'native recommendation reason detail');
      await capture('recommendation', stillPaused);
      click(one('[aria-label="返回上一页"]'));
      await wait(() => ready() && cards().length === 3
        && document.activeElement?.dataset.resultAction === 'reason:0', 'Back restores exact reason focus and search');
      verifyFooters();
      await capture('back', stillPaused);
    }
    await emit('complete', {
      ok: true, footerCase: mode, searchFooterVerified: true, stageCount: count,
      nativeNullPlaybackVerified: true, realMusicRequests: 0, realAccountWrites: 0,
      historicalLayoutOnly: mode === 'error',
    });
  } catch (error) {
    let account, accountReadFailure, currentPresentation, presentationReadFailure;
    try {
      const value = await invoke('account_snapshot');
      account = { epoch: value.epoch, status: value.status, loginNotice: value.loginNotice ?? null };
    } catch (cause) {
      accountReadFailure = { code: cause?.code ?? null };
    }
    if (epoch) {
      try {
        currentPresentation = await presentation();
      } catch (cause) {
        presentationReadFailure = { code: cause?.code ?? null };
      }
    }
    await emit('failed', {
      ok: false, footerCase: mode, step, error: String(error), stack: error.stack,
      account, accountReadFailure, page: one('.main-content')?.innerText.slice(0, 1800) ?? null,
      firstOrdinaryFooterCount, footerCount: footerCount(), ordinaryReady: ordinaryReady(),
      cardKeys: cards().map((card) => card.dataset.resultKey), currentPresentation, presentationReadFailure,
    });
  }
})();
