// Real native queue and playback; only delay the renderer receipt of queue rows.
(async () => {
  const invoke = (...args) =>
    (window.__CUMULUS_PROOF_INVOKE__ ?? window.__TAURI_INTERNALS__.invoke)(...args);
  const emit = (stage, data = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...data } });
  const one = (selector) => document.querySelector(selector);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  async function wait(test, label) {
    for (let i = 0; i < 250; i++) {
      const value = await test();
      if (value) return value;
      await sleep(65);
    }
    throw Error(label);
  }
  let restore = () => {};
  try {
    await wait(() => one('.track-row') && one('.playerbar button[aria-label="播放队列"]:not(:disabled)'),
      'synthetic account and track list');
    const provides = one('#app').__vue_app__._context.provides;
    const client = provides[Reflect.ownKeys(provides).find((key) =>
      typeof key === 'symbol' && key.description === 'core-client')];
    const { epoch } = await client.accountSnapshot();
    const queue = (ids, index) => client.playSelection(epoch, { kind: 'songs', ids, index, quality: 'standard' });
    const ids = Array.from({ length: 40 }, (_, index) => String(index + 1));
    await queue(ids, 0);
    await wait(() => one('.playerbar').dataset.state === 'playing', 'first playback');
    one('.playerbar button[aria-label="播放队列"]').click();
    const scroller = await wait(() => one('.queue-scroll'), 'queue mounted');
    await wait(() => one('.queue-row.current'), 'original current entry');
    const initial = await client.playerSnapshot(epoch);
    const read = client.playerQueue;
    const held = [];
    let blocking = true;
    client.playerQueue = async (...args) => {
      const snapshot = await read(...args);
      if (blocking) await new Promise((resolve) => held.push(resolve));
      return snapshot;
    };
    restore = () => {
      blocking = false;
      client.playerQueue = read;
      held.splice(0).forEach((resolve) => resolve());
    };
    await queue([...ids].reverse(), 34);
    const native = await wait(async () => {
      const view = await client.playerSnapshot(epoch);
      return view.playId !== initial.playId && view.track?.id === '6' && view;
    }, 'replacement native playback');
    await wait(() => held.length && one('.playerbar').dataset.playId === native.playId,
      'current entry reached renderer before new rows');
    await sleep(200);
    if (one('.queue-scroll') !== scroller || scroller.scrollTop > 10 || one('.queue-row.current'))
      throw Error('stale rows were repositioned as current');
    await emit('ux_01_queue_waiting_rows', { scrollTop: scroller.scrollTop });
    await sleep(350);
    const close = one('.queue-panel button[aria-label="关闭播放队列"]');
    close.focus({ preventScroll: true });
    restore();
    const row = await wait(() => {
      const current = one('.queue-row.current');
      return scroller.scrollTop > 1500 && current?.dataset.entryId === native.entryId && current;
    }, 'new authoritative queue centers current item');
    await sleep(250);
    const a = row.getBoundingClientRect(), b = scroller.getBoundingClientRect();
    if (a.top < b.top || a.bottom > b.bottom || document.activeElement !== close)
      throw Error('center did not reveal the new row or stole focus');
    await emit('ux_02_queue_current_located', {
      scrollTop: scroller.scrollTop, currentVisible: true, playId: native.playId,
    });
    await sleep(400);
    scroller.scrollTop = 0;
    scroller.dispatchEvent(new Event('scroll'));
    await wait(() => !one('.queue-row.current'), 'manual browsing elsewhere');
    await client.playerAction(epoch, { kind: 'mode', mode: 'single_loop' });
    await wait(() => one('.playerbar').dataset.mode === 'single_loop' &&
      !one('.queue-panel button.text-button[disabled]'), 'same entry queue refresh');
    await sleep(400);
    if (scroller.scrollTop > 10 || one('.queue-row.current') || document.activeElement !== close)
      throw Error('same-entry refresh reset manual browsing');
    await emit('complete', { ok: true, sameEntryRefreshPreservedScroll: true });
  } catch (error) {
    await emit('failed', { error: String(error?.message ?? error) });
  } finally {
    restore();
  }
})();
