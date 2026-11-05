// A short authorized WAV naturally completes in null libmpv. The only
// explicit controls before EOF are Pause and Resume. Each control cohort
// clicks the visible Next button after its finished state has been captured.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const bar = () => one('.playerbar');
  const primary = () => one('.playerbar .play-toggle');
  const next = () => one('.playerbar button[aria-label="下一首"]');
  const unread = () => Boolean(one('[aria-label="消息"] .message-drawer-unread-dot'));
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, payload) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, captureHandshake: true, ...payload },
  });
  const assert = (condition, message) => { if (!condition) throw new Error(message); };
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const twoFrames = () => new Promise((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(resolve)));
  async function wait(read, message) {
    const deadline = performance.now() + 18000;
    while (performance.now() < deadline) {
      const value = await read();
      if (value) return value;
      await sleep(55);
    }
    throw new Error(message);
  }
  function visible(button) {
    if (!button || button.disabled || button.closest('[inert]')) return false;
    const bounds = button.getBoundingClientRect();
    const hit = document.elementFromPoint(bounds.left + bounds.width / 2, bounds.top + bounds.height / 2);
    return bounds.width > 0 && bounds.height > 0 && button.contains(hit);
  }
  function click(label) {
    const button = primary();
    assert(visible(button) && button.getAttribute('aria-label') === label, 'expected visible ' + label);
    button.focus({ preventScroll: true });
    button.click();
  }
  function paintSample(selector) {
    const node = one(selector);
    assert(node?.isConnected && !node.closest('[inert]'), 'connected capture padding');
    const bounds = node.getBoundingClientRect();
    const pad = Number.parseFloat(getComputedStyle(node).paddingLeft);
    const x = bounds.left + Math.min(4, pad / 2), y = bounds.top + bounds.height / 2;
    assert(pad >= 4 && document.elementFromPoint(x, y) === node, 'uncovered capture padding');
    let color;
    for (let current = node; current; current = current.parentElement) {
      const style = getComputedStyle(current);
      assert(style.opacity === '1' && style.visibility === 'visible', 'visible paint ancestry');
      if (color) continue;
      assert(style.backgroundImage === 'none', 'solid capture padding');
      const rgb = /^rgb\((\d+),\s*(\d+),\s*(\d+)\)$/.exec(style.backgroundColor);
      if (rgb) color = 'rgb(' + rgb.slice(1).join(', ') + ')';
      else assert(style.backgroundColor === 'transparent' ||
        /^rgba\(\d+,\s*\d+,\s*\d+,\s*0\)$/.test(style.backgroundColor), 'nonopaque paint padding');
    }
    assert(color, 'expected opaque background color');
    return { x, y, color, source: selector };
  }
  const caseName = window.__ordinaryEofProofCase;
  const group = window.__minibarProofGroup;
  const scenario = {
    't1-skip': ['t1', 'playing', 'sequential', '1,2,3'],
    control: ['c', 'ended', 'sequential', '1,2,3'],
    't1-tail': ['t1', 'idle', 'sequential', '1,2,3'],
    'order-loop-skip': ['t1', 'playing', 'order_loop', '2,3,1'],
    'random-skip': ['t1', 'playing', 'random', '1,3,2'],
    'random-control': ['c', 'ended', 'random', '1,3,2'],
  }[caseName];
  const [expectedGroup, expectedState, expectedMode, expectedOrder] = scenario || [];
  const skip = expectedState === 'playing';
  const randomControl = caseName === 'random-control';
  const controlNext = caseName === 'control' || randomControl;
  const target = skip ? '3' : '1';
  const actions = [];
  const replies = [];
  const skipNotice = window.__observeOrdinarySkipNotice();
  let core;
  let original;
  try {
    assert(expectedState && group === expectedGroup, 'sealed EOF case and cohort');
    const account = await wait(async () => {
      const value = await invoke('account_snapshot');
      return value.status === 'authenticated' && value;
    }, 'synthetic authenticated account');
    const { epoch } = account;
    await wait(() => navigator.onLine === true && unread(), 'real browser online with native count');
    const player = () => invoke('player_snapshot', { epoch });
    const queue = () => invoke('player_queue', { epoch });
    const dom = () => ({
      state: bar()?.dataset.state, playId: bar()?.dataset.playId ?? null, trackId: bar()?.dataset.trackId,
      position: Number(bar()?.dataset.position), title: one('.playerbar .now-copy button')?.textContent.trim(),
      elapsed: one('.playerbar .seek-control time')?.textContent.trim(),
      primaryLabel: primary()?.getAttribute('aria-label'), unread: unread(),
      nextLabel: next()?.getAttribute('aria-label'), nextEnabled: visible(next()),
      endedStepKey: next()?.dataset.endedStepKey ?? null,
    });
    const matching = (view) => {
      const rendered = dom();
      return view.track?.id === rendered.trackId && view.playId === rendered.playId && view.state === rendered.state;
    };
    const state = (view) => ({
      state: view.state, epoch: view.epoch, mode: view.mode, trackId: view.track?.id, entryId: view.entryId,
      playId: view.playId, endedMinibarReceipt: view.endedMinibarReceipt ?? null,
      queueRevision: view.queueRevision, position: view.position ?? null,
      duration: view.timeline?.total ?? null, timelineMode: view.timeline?.mode ?? null, error: view.error,
    });
    const idsOf = (value) => value.tracks.map((item) => [item.track.id, item.entryId]);
    const stage = async (label, view, q) => {
      await twoFrames();
      assert(matching(view) && document.visibilityState === 'visible', 'visible native and DOM state for ' + label);
      const toast = skipNotice.capture(label === 'ux_02_after' && skip);
      const paintSamples = [...['.sidebar-caption', '.playerbar'].map(paintSample), ...toast.paint];
      assert(paintSamples.some((sample) => sample.color !== 'rgb(0, 0, 0)'), 'not an all-black frame');
      await emit(label, {
        actual: true, case: caseName, group, online: navigator.onLine, player: state(view),
        dom: dom(), ids: idsOf(q), current: q.current, actions, replies, skipNotices: toast.visible,
        screenX, screenY, ratio: devicePixelRatio, paintSamples,
      });
      if (toast.visible.length && window.__CUMULUS_PROOF_ACK__ === label)
        assert(skipNotice.visible(), 'PNG capture completed while the toast was still fully visible');
    };
    const initial = await wait(async () => {
      const view = await player();
      return view.track?.id === '1' && view.mode === expectedMode && view.queueLen === 3 &&
        ['idle', 'paused', 'playing'].includes(view.state) && matching(view) && visible(primary()) && view;
    }, 'ordinary queue on actual PlayerBar in ' + expectedMode);
    if (initial.state !== 'playing') click('播放');
    const running = await wait(async () => {
      const view = await player();
      return view.state === 'playing' && view.track?.id === '1' && view.position > .35 &&
        view.timeline?.mode === 'full' && view.timeline.total === 8 && matching(view) && view;
    }, 'first real libmpv clock and full short WAV');
    const provided = one('#app').__vue_app__._context.provides;
    core = provided[Reflect.ownKeys(provided).find((key) =>
      typeof key === 'symbol' && key.description === 'core-client')];
    original = core.playerAction;
    core.playerAction = (owner, action) => {
      actions.push({ epoch: owner, action: JSON.parse(JSON.stringify(action)) });
      return original.call(core, owner, action).then((reply) => {
        replies.push(state(reply));
        return reply;
      });
    };
    click('暂停');
    const before = await wait(async () => {
      const view = await player();
      return replies.length === 1 && view.state === 'paused' && view.playId === running.playId &&
        view.position > .2 && view.position < 7.5 && matching(view) && visible(primary()) && view;
    }, 'same actual decoder paused for the before capture');
    const beforeQueue = await queue();
    const ids = idsOf(beforeQueue);
    assert(ids.map(([id]) => id).join(',') === expectedOrder &&
      beforeQueue.current === ids.find(([id]) => id === '1')[1] &&
      beforeQueue.revision === before.queueRevision, 'preserve original displayed queue order and revision');
    await stage('ux_01_before', before, beforeQueue);

    click('播放');
    const resumed = await wait(async () => {
      const view = await player();
      return replies.length === 2 && view.state === 'playing' && view.playId === before.playId &&
        view.position > before.position + .08 && matching(view) && view;
    }, 'same old native clock resumes without reload or seek');
    assert(resumed.position < 8, 'old source still running before natural EOF');
    const after = await wait(async () => {
      const view = await player();
      if (view.error) throw new Error('unexpected playback error: ' + view.error);
      return view.state === expectedState && view.track?.id === target && matching(view) &&
        (skip ? view.position > .2 && view.timeline?.total === 30 && skipNotice.visible() : view.position === 8) &&
        (!randomControl || (view.playId === null &&
          /^[a-f0-9]{32}$/.test(view.endedMinibarReceipt) &&
          dom().endedStepKey === JSON.stringify([epoch, view.endedMinibarReceipt, view.entryId, view.queueRevision]))) && view;
    }, 'cohort-specific state after natural native EOF');
    const afterQueue = await queue();
    assert(JSON.stringify(idsOf(afterQueue)) === JSON.stringify(ids) &&
      afterQueue.current === ids.find(([id]) => id === target)[1] &&
      afterQueue.revision === after.queueRevision, 'natural transition keeps queue rows and order');
    assert(actions.length === 2 && actions.every(({ epoch: owner, action }, i) =>
      owner === epoch && action.kind === 'pause_for' && action.play_id === before.playId &&
      action.expected_queue_revision === before.queueRevision && action.paused === (i === 0)),
    'real UI sent pause/resume only, no next, seek, or stop');
    assert(skip ? after.playId && after.playId !== before.playId :
      after.queueRevision === before.queueRevision &&
        (caseName === 'control' ? after.playId === before.playId : after.playId === null),
    'native play identity and queue revision agree with the cohort outcome');
    await stage('ux_02_after', after, afterQueue);
    if (controlNext) {
      await twoFrames();
      const button = next();
      assert(visible(button) && matching(after), 'finished control has a visible enabled Next button');
      if (randomControl)
        assert(button.dataset.endedStepKey ===
          JSON.stringify([epoch, after.endedMinibarReceipt, after.entryId, after.queueRevision]),
        'random control Next is rendered for the exact retained native EOF');
      button.focus({ preventScroll: true });
      button.click();
      const retained = await wait(async () => {
        if (replies.length !== 3) return false;
        const view = await player();
        return JSON.stringify(state(view)) === JSON.stringify(state(after)) &&
          matching(view) && visible(next()) &&
          (!randomControl || document.activeElement === next()) && view;
      }, 'real scoped Next acknowledgment retains the finished control playback');
      const retainedQueue = await queue();
      const stepped = actions[2]?.action;
      assert(actions.length === 3 && actions[2].epoch === epoch &&
        stepped.direction === 'next' && stepped.expected_queue_revision === after.queueRevision &&
        (randomControl
          ? Object.keys(stepped).length === 5 && stepped.kind === 'step_ended_mini_bar' &&
            stepped.receipt === after.endedMinibarReceipt && stepped.entry_id === after.entryId
          : Object.keys(stepped).length === 4 && stepped.kind === 'step_mini_bar' &&
            stepped.play_id === after.playId),
      'one real Next with the exact finished play or retained EOF identity and queue revision');
      assert(JSON.stringify(replies[2]) === JSON.stringify(state(after)) &&
        retainedQueue.current === afterQueue.current &&
        retainedQueue.revision === afterQueue.revision &&
        JSON.stringify(idsOf(retainedQueue)) === JSON.stringify(ids),
      'native Next reply retains the same finished clock, play and queue');
      await stage('ux_03_control_next', retained, retainedQueue);
    }
    const notices = await skipNotice.finish(skip);
    await emit('complete', { ok: true, case: caseName, group, target, notices,
      stageCount: controlNext ? 3 : 2 });
  } catch (error) {
    await emit('failed', { ok: false, case: caseName, error: String(error),
      state: bar()?.dataset.state, trackId: bar()?.dataset.trackId, online: navigator.onLine, actions, replies });
  } finally {
    skipNotice.stop();
    if (core && original) core.playerAction = original;
  }
})();
