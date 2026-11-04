// Closed desktop probe: libmpv stops without a UI Stop request.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const invoke = (...args) =>
    (window.__CUMULUS_PROOF_INVOKE__ ?? window.__TAURI_INTERNALS__.invoke)(...args);
  const emit = (stage, data = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...data } });
  const assert = (value, message) => { if (!value) throw new Error(message); };
  const wait = async (read, message) => {
    for (let attempt = 0; attempt < 190; attempt++) {
      const result = await read();
      if (result) return result;
      await sleep(65);
    }
    throw new Error(message);
  };
  const bar = () => one('.playerbar');
  const primary = () => one('.playerbar .play-toggle');
  const click = (button) => {
    assert(button && !button.disabled, 'expected an enabled player control');
    button.focus({ preventScroll: true });
    button.click();
  };
  window.__cardSourceAck = {};
  let request = 0;
  async function audio(mode) {
    const id = ++request;
    await emit('card-source-fixture', { id, mode });
    const report = await wait(() => window.__cardSourceAck[id], 'native fixture acknowledgement');
    delete window.__cardSourceAck[id];
    assert(!report.error, 'native fixture refused its closed command');
    return report;
  }
  async function capture(stage, state) {
    await emit(stage, { captureHandshake: true, player: {
      state: state.state, position: state.position, entryId: state.entryId, playId: state.playId,
      trackId: state.track?.id, action: primary()?.getAttribute('aria-label'),
    }, audio: await audio('read') });
  }
  try {
    const account = await wait(async () => {
      const view = await invoke('account_snapshot');
      return view.status === 'authenticated' && view;
    }, 'isolated account');
    const { epoch } = account;
    const player = () => invoke('player_snapshot', { epoch });
    await invoke('play_selection', { epoch, selection: {
      kind: 'songs', ids: ['1', '2'], index: 0, quality: 'standard',
    } });
    await wait(() => primary()?.getAttribute('aria-label') === '暂停' && !primary().disabled,
      'ordinary native playback');
    click(primary());
    await wait(() => bar()?.dataset.state === 'paused', 'pause before seeking');
    const seek = one('.playerbar [aria-label="播放进度"]');
    seek.value = '9';
    seek.dispatchEvent(new Event('change', { bubbles: true }));
    await wait(() => Math.abs(Number(bar()?.dataset.position) - 9) < .15 && !primary()?.disabled,
      'native seek');
    click(primary());
    const before = await wait(async () => {
      const value = await player();
      return value.state === 'playing' && value.position >= 9 && value;
    }, 'clock advances after seek');
    const original = await audio('read');
    assert(original.loads === 1 && original.songAuthorizations === 1, 'single initial decoder/auth');
    await capture('ux_01_playing', before);

    await audio('stop-in-native');
    const stopped = await wait(async () => {
      const value = await player();
      return value.state === 'idle' && primary()?.getAttribute('aria-label') === '播放' &&
        !primary().disabled && value;
    }, 'native stop retires the transport');
    assert(!stopped.error && !stopped.playId && stopped.entryId === before.entryId &&
      stopped.track?.id === before.track?.id && stopped.queueRevision === before.queueRevision,
    'stop is neutral and does not advance the queue');
    await sleep(480);
    const idle = await audio('read');
    assert(idle.observedNativeStops === original.observedNativeStops + 1 &&
      idle.loads === original.loads && idle.songAuthorizations === original.songAuthorizations &&
      (await player()).state === 'idle', 'the observed native event triggers no autoplay or authorization');
    await capture('ux_02_stopped', stopped);

    click(primary());
    const resumed = await wait(async () => {
      const value = await player();
      return value.state === 'playing' && primary()?.getAttribute('aria-label') === '暂停' &&
        value.position >= before.position - .12 && value;
    }, 'main Play resumes from the accepted clock');
    assert(resumed.position < before.position + 4 && resumed.entryId === before.entryId &&
      resumed.playId !== before.playId && resumed.queueRevision === before.queueRevision,
    'explicit resume keeps the same selection, with a new decoder instance');
    await capture('ux_03_resumed', resumed);

    click(one('.playerbar [aria-label="停止播放"]'));
    await wait(() => bar()?.dataset.state === 'idle' && primary()?.getAttribute('aria-label') === '播放' && !primary().disabled,
      'explicit Stop reaches idle');
    click(primary());
    const restarted = await wait(async () => {
      const value = await player();
      return value.state === 'playing' && primary()?.getAttribute('aria-label') === '暂停' &&
        value.position !== null && value;
    }, 'Play after explicit Stop');
    assert(restarted.position < 2 && restarted.entryId === before.entryId, 'explicit Stop still restarts at zero');
    await capture('ux_04_restarted', restarted);
    await emit('complete', { ok: true, nativeStopVerified: true, original, idle, final: await audio('read') });
  } catch (error) {
    await emit('failed', { error: String(error), state: bar()?.dataset.state, position: bar()?.dataset.position });
  }
})();
