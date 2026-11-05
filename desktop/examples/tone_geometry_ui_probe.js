// Real stock EQ UI, ordinary player IPC and native libmpv with sealed media.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, payload = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...payload },
  });
  const assert = (condition, message) => { if (!condition) throw Error(message); };
  const enabled = (node) => node?.isConnected && !node.disabled && node;
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  let step = 'account', epoch, last;
  const state = async () => (last = await invoke('player_snapshot', { epoch }));
  const action = (value) => invoke('player_action', { epoch, action: value });
  const select = (id) => invoke('play_selection', { epoch, selection: {
    kind: 'songs', ids: [String(id)], index: 0, quality: 'standard',
  } });
  const safe = (view) => {
    assert(!view.error, `player reported ${view.error}`);
    return view;
  };
  async function wait(read, message) {
    for (let n = 0; n < 150; n++) {
      const result = await read();
      if (result) return result;
      await sleep(65);
    }
    throw Error(message);
  }
  const same = (previous, current) => previous.playId === current.playId &&
    previous.entryId === current.entryId;
  const synced = (view) => one('.playerbar')?.dataset.playId === view.playId;
  async function capture(name, observed) {
    step = `ux_${name}`;
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await emit(step, { observed, captureHandshake: true });
    await wait(() => window.__toneGeometryRecorded === step, 'native tone stage receipt');
  }
  function edit(slider, db) {
    assert(enabled(slider), 'real EQ adjustment is usable');
    slider.value = String(db);
    slider.dispatchEvent(new Event('change', { bubbles: true }));
  }
  try {
    ({ epoch } = await wait(async () => {
      const account = await invoke('account_snapshot');
      return account.status === 'authenticated' && account;
    }, 'sealed account did not restore'));
    await action({ kind: 'mode', mode: 'sequential' });
    step = 'load_mono';
    safe(await select(1));
    const start = await wait(async () => {
      const view = safe(await state());
      return view.state === 'playing' && view.track?.id === '1' && view.position > .12 && synced(view) && view;
    }, 'ordinary mono source starts');
    (await wait(() => enabled(one('.equalizer-toggle')), 'equalizer button')).click();
    const dialog = await wait(() => one('.equalizer-dialog[open]'), 'real equalizer dialog');
    const bass = await wait(() => enabled(dialog.querySelector('[aria-label="低音增益"]')), 'bass slider');
    const treble = await wait(() => enabled(dialog.querySelector('[aria-label="高音增益"]')), 'treble slider');
    edit(bass, 4);
    const wet = await wait(async () => {
      const view = safe(await state());
      return same(start, view) && view.equalizer.settings.bass_db === 4 &&
        view.equalizer.status === 'processing' && view.position > start.position + .16 && view;
    }, 'mono EQ observes real PCM at bass +4');
    await capture('01_mono_active', wet);
    step = 'hot_material';
    edit(treble, -3);
    const hot = await wait(async () => {
      const view = safe(await state());
      return same(wet, view) && view.equalizer.settings.treble_db === -3 &&
        view.equalizer.status === 'processing' && view.position > wet.position + .16 && view;
    }, 'hot tone revision observed without a new play');
    await capture('02_mono_hot', hot);
    step = 'paused_seek';
    const paused = safe(await action({ kind: 'pause', paused: true }));
    assert(same(hot, paused) && paused.state === 'paused', 'same play pauses');
    safe(await action({ kind: 'seek', seconds: 1.37 }));
    const sought = await wait(async () => {
      const view = safe(await state());
      return same(hot, view) && view.state === 'paused' && !view.nativeSeeking &&
        Math.abs(view.position - 1.37) < .12 && view;
    }, 'same play native paused seek settles');
    await capture('03_mono_paused_seek', sought);
    step = 'resume';
    safe(await action({ kind: 'pause', paused: false }));
    const resumed = await wait(async () => {
      const view = safe(await state());
      return same(sought, view) && view.state === 'playing' && view.position > 1.56 &&
        view.equalizer.status === 'processing' && view;
    }, 'same mono play resumes after seek');
    await capture('04_mono_resumed', resumed);
    step = 'stereo';
    safe(await select(2));
    const stereo = await wait(async () => {
      const view = safe(await state());
      return !same(resumed, view) && view.track?.id === '2' && view.state === 'playing' &&
        view.position > .18 && synced(view) && view.equalizer.status === 'processing' && view;
    }, 'real stereo source advances on a new play');
    await capture('05_stereo_active', stereo);
    step = 'stereo_end';
    const ended = await wait(async () => {
      const view = safe(await state());
      return view.state === 'ended' && view.track?.id === '2' && view;
    }, 'stereo reaches natural sequential EOF');
    await emit('complete', { ok: true, stageCount: 5, observed: ended });
  } catch (error) {
    await emit('failed', { step, message: String(error), last });
  }
})();
