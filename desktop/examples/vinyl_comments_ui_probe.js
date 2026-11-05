// Closed fixture using the actual Tauri WebView, account, catalog player and IPC.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const assert = (pass, label) => { if (!pass) throw new Error(label); };
  async function wait(test, label) {
    for (let index = 0; index < 170; index++) {
      const result = await test();
      if (result) return result;
      await sleep(65);
    }
    throw new Error(label);
  }
  const click = (node) => {
    assert(node && !node.disabled, 'available control');
    node.focus({ preventScroll: true });
    node.click();
  };
  const wheel = (node, deltaY) => node.dispatchEvent(new WheelEvent('wheel', { deltaY, bubbles: true }));
  const footer = (label) => one(`.playerbar [aria-label="${label}"]`);
  const pane = () => one('.lyrics-panel');
  const tab = () => pane()?.dataset.vinylTab;
  const back = () => one('.vinyl-comments-back');
  const scroller = () => one('.vinyl-comments-scroll');
  const rows = (group) => all(`[data-vinyl-comment-group="${group}"] .vinyl-comment-row`);
  let stages = 0;

  try {
    const { epoch } = await wait(async () => {
      const account = await invoke('account_snapshot');
      return account.status === 'authenticated' && account;
    }, 'synthetic authenticated profile ready');
    const player = () => invoke('player_snapshot', { epoch });
    async function capture(label, baseline, extras = {}) {
      await sleep(550);
      const actual = await player();
      assert(actual.playId === baseline.playId && actual.queueRevision === baseline.queueRevision &&
        actual.state === baseline.state, 'comment UI preserves native transport');
      if (actual.state === 'paused') assert(Math.abs(actual.position - baseline.position) < .1, 'paused position unchanged');
      const stage = `ux_${String(++stages).padStart(2, '0')}_${label}`;
      if (window.__vinylComposerRun) window.__vinylComposerRecorded = '';
      await emit(stage, {
        tab: tab() ?? null, playId: actual.playId, trackId: actual.track?.id,
        latest: rows('latest').length, hot: rows('hot').length, captureHandshake: true, ...extras,
      });
      if (window.__vinylComposerRun)
        await wait(() => window.__vinylComposerRecorded === stage, 'native stage recorded: ' + stage);
    }
    async function pause() {
      click(footer('暂停'));
      return wait(async () => {
        const value = await player();
        return value.state === 'paused' && value;
      }, 'native pause');
    }

    await emit('resize', { width: 1000, height: 680 });
    await wait(() => innerWidth === 1000 && innerHeight === 680, 'bounded viewport');
    await wait(() => one('.daily-track-actions') && one('.track-row') && footer('列表循环') &&
      !footer('列表循环').disabled, 'ordinary recommendations ready');
    one('.track-row').dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    await wait(async () => {
      const now = await player();
      return now.state === 'playing' && now.track?.id === '1' && now.position > .16;
    }, 'ordinary native playback');
    const first = await pause();
    const comment = () => footer('当前歌曲评论');
    await wait(() => one('.player-comment-count')?.textContent.trim() === '63', 'authoritative current count');
    click(footer('歌词'));
    await wait(() => tab() === 'lyrics' && one('.lyric-line.current'), 'standard vinyl details');
    if (window.__vinylComposerRun) {
      await window.__vinylComposerRun({ epoch, first, player, pause, comment, capture, tab, scroller,
        rows, footer, pane, wait, click, invoke, assert, one, all });
      await emit('complete', { ok: true, stageCount: stages, realAccountWrites: 0, realMusicRequests: 0 });
      return;
    }
    if (window.__vinylCommentControl) {
      await sleep(400);
      assert(!one('.vinyl-scroll-comment') && comment().getAttribute('aria-pressed') === null,
        'control group does not get the embedded entry');
      click(comment());
      await wait(() => !pane() && one('.comments-page [data-comment-sort="latest"][aria-selected="true"]'),
        'control uses the existing independent route after closing vinyl');
      await capture('control_route', first);
      await emit('complete', { ok: true, stageCount: stages });
      return;
    }
    await wait(() => one('.vinyl-scroll-comment') && comment().getAttribute('aria-pressed') === 'false',
      't1 enables outer wheel and current footer toggle');
    await capture('standard_before_read', first);

    const viewport = one('.vinyl-tab-viewport');
    wheel(one('.lyric-scroll'), 600);
    assert(tab() === 'lyrics', 'lyrics scroller consumes wheel');
    wheel(viewport, 220);
    await sleep(55);
    assert(tab() === 'lyrics', 'below the official threshold');
    wheel(viewport, 260);
    await wait(() => tab() === 'comments' && document.activeElement === back(), 'outer wheel enters and focuses return');
    await sleep(230);
    assert(!rows('latest').length && !one('.vinyl-mode-entry'), 'first read is deferred and mode button hidden');
    await wait(() => rows('latest').length === 60 && rows('hot').length === 5, 'single combined sixty-row and five-hot read');
    await capture('embedded_first', first);

    scroller().scrollTop = 150;
    wheel(scroller(), -500);
    await sleep(100);
    assert(tab() === 'comments', 'comment scroller consumes wheel away from its top');
    scroller().scrollTop = scroller().scrollHeight;
    await wait(() => rows('latest').length === 63, 'sentinel appended the exact next offset');
    await capture('embedded_append', first);

    scroller().scrollTop = 0;
    wheel(scroller(), -480);
    await wait(() => tab() === 'lyrics' && one('.vinyl-mode-entry'), 'top-edge wheel returns to lyrics');
    window.__motionProof.system(true);
    await wait(() => document.documentElement.dataset.motion === 'reduced' &&
      getComputedStyle(one('.vinyl-tab-stack')).transitionDuration === '0s',
      'reduced motion removes the vertical transition');
    click(comment());
    await wait(() => tab() === 'comments' && document.activeElement === back(), 'footer toggles inside vinyl');
    click(back());
    await wait(() => tab() === 'lyrics' && document.activeElement === comment(), 'compact track returns focus to footer');
    await capture('return_focus', first);

    click(comment());
    await wait(() => tab() === 'comments' && one('.vinyl-more-hot'), 'retained same-thread contents on reentry');
    click(one('.vinyl-more-hot'));
    await wait(() => !pane() && one('.comments-page [data-comment-sort="hot"][aria-selected="true"]'),
      'more hot comments opens the independent hot lane');
    await capture('hot_route', first);

    click(footer('歌词'));
    await wait(() => tab() === 'comments' && rows('latest').length === 63, 'reopened same thread without list reread');
    click(footer('下一首'));
    await wait(async () => {
      const now = await player();
      return now.state === 'playing' && now.track?.id === '2';
    }, 'new song under active comment tab');
    const second = await pause();
    await wait(() => tab() === 'comments' && scroller()?.textContent.includes('暂无评论'),
      'known zero clears old thread with no list request');
    assert(!rows('latest').length && !rows('hot').length, 'old comments are not rendered');
    await capture('zero_count', second);
    await emit('complete', { ok: true, stageCount: stages });
  } catch (error) {
    await emit('failed', { error: error instanceof Error ? String(error) : JSON.stringify(error),
      stack: error?.stack, tab: tab() ?? null, stageCount: stages,
      focus: document.activeElement?.getAttribute('aria-label') });
  }
})();
