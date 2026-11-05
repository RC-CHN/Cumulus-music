// A synthetic click on the actual rendered control; no substitute playback IPC.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const bar = () => one('.playerbar');
  const next = () => one('.playerbar button[aria-label="下一首"]');
  const primary = () => one('.playerbar .play-toggle');
  const unread = () => Boolean(one('[aria-label="消息"] .message-drawer-unread-dot'));
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, data) => invoke('plugin:event|emit', { event: 'ux-proof',
    payload: { stage, captureHandshake: true, ...data } });
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (condition, message) => { if (!condition) throw new Error(message); };
  async function wait(read, message) {
    for (let index = 0; index < 180; index += 1) {
      const result = await read();
      if (result) return result;
      await sleep(65);
    }
    throw new Error(message);
  }
  function visible(button) {
    if (!button || button.disabled) return false;
    const box = button.getBoundingClientRect();
    return box.width > 0 && box.height > 0;
  }
  function click(button) {
    assert(visible(button), 'expected a visible enabled control');
    button.focus({ preventScroll: true });
    button.click();
  }
  const group = window.__minibarProofGroup;
  const target = group === 't1' ? '3' : '1';
  let core;
  let original;
  let acknowledgement;
  const actions = [];
  try {
    assert(group === 't1' || group === 'c', 'fixture cohort');
    const account = await wait(async () => {
      const state = await invoke('account_snapshot');
      return state.status === 'authenticated' && state;
    }, 'synthetic authenticated account');
    const { epoch } = account;
    await wait(() => navigator.onLine === true && unread(), 'real browser online and native count read');
    const player = () => invoke('player_snapshot', { epoch });
    const queue = () => invoke('player_queue', { epoch });
    const current = await wait(async () => {
      const state = await player();
      return state.queueLen === 3 && state.track?.id === '1' && visible(primary()) &&
        ['idle', 'paused', 'playing'].includes(state.state) && bar()?.dataset.state === state.state && state;
    }, 'initial native selection and real PlayerBar');
    if (current.state !== 'playing') {
      assert(primary().getAttribute('aria-label') === '播放', 'initial control can start playback');
      click(primary());
    }
    const readPlaying = (id) => async () => {
      const state = await player();
      return state.track?.id === id && state.state === 'playing' && state.position > .15 &&
        bar()?.dataset.playId === state.playId && bar()?.dataset.trackId === id &&
        bar()?.dataset.state === 'playing' && visible(next()) && state;
    };
    const before = await wait(readPlaying('1'), 'first libmpv clock and matching PlayerBar');
    const beforeQueue = await queue();
    const ids = beforeQueue.tracks.map((item) => [item.track.id, item.entryId]);
    assert(ids.map(([id]) => id).join(',') === '1,2,3', 'seeded ordinary queue');
    assert(before.mode === 'single_loop' && beforeQueue.current === ids[0][1], 'initial mode and entry');
    const render = () => ({
      trackId: bar().dataset.trackId, playId: bar().dataset.playId, state: bar().dataset.state,
      title: one('.playerbar .now-copy button')?.textContent.trim(),
      nextLabel: next().getAttribute('aria-label'), nextEnabled: visible(next()), unread: unread(),
    });
    const state = (view) => ({
      trackId: view.track.id, playId: view.playId, entryId: view.entryId, mode: view.mode,
      queueRevision: view.queueRevision, state: view.state, position: view.position,
      duration: view.timeline?.total, epoch: view.epoch, error: view.error,
    });
    await emit('ux_01_before', { actual: true, group, online: navigator.onLine,
      player: state(before), dom: render(), ids, current: beforeQueue.current });
    const provided = one('#app').__vue_app__._context.provides;
    core = provided[Reflect.ownKeys(provided).find((key) =>
      typeof key === 'symbol' && key.description === 'core-client')];
    original = core.playerAction;
    // Observe/delegate the real bridge. Do not change its request or result.
    core.playerAction = (owner, action) => {
      actions.push({ epoch: owner, action: JSON.parse(JSON.stringify(action)) });
      return original.call(core, owner, action).then((reply) => {
        acknowledgement = state(reply);
        return reply;
      });
    };
    click(next());
    const after = await wait(async () => {
      if (!acknowledgement) return false;
      const value = await readPlaying(target)();
      return value && (group === 't1' || value.position > before.position + .05) && value;
    }, 'bottom-bar IPC acknowledgment and expected native clock');
    const afterQueue = await queue();
    assert(actions.length === 1 && actions[0].epoch === epoch, 'exact one real button action');
    const sent = actions[0].action;
    assert(sent.kind === 'step_mini_bar' && sent.direction === 'next' &&
      sent.play_id === before.playId && sent.expected_queue_revision === before.queueRevision,
    'bottom-bar IPC keeps the previous play and queue fence');
    assert(afterQueue.current === ids[Number(target) - 1][1] &&
      JSON.stringify(afterQueue.tracks.map((item) => [item.track.id, item.entryId])) === JSON.stringify(ids),
    'native step does not replace queue entries');
    assert(acknowledgement.playId === after.playId && acknowledgement.entryId === after.entryId &&
      acknowledgement.trackId === target, 'step acknowledgment matches the observed native player');
    assert(group === 't1' ? after.playId !== before.playId :
      after.playId === before.playId && after.queueRevision === before.queueRevision &&
        beforeQueue.current === afterQueue.current, 'treatment steps and control keeps the playing source');
    await emit('ux_02_after', { actual: true, group, online: navigator.onLine,
      player: state(after), dom: render(), ids, current: afterQueue.current, actions, acknowledgement });
    await emit('complete', { ok: true, group, target, stageCount: 2 });
  } catch (error) {
    await emit('failed', { ok: false, group, error: String(error), online: navigator.onLine,
      state: bar()?.dataset.state, trackId: bar()?.dataset.trackId, unread: unread(), actions });
  } finally {
    if (core && original) core.playerAction = original;
  }
})();
