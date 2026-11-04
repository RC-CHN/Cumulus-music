// Actual player WebView and native libmpv/null, closed synthetic source.
(async () => {
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, facts = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...facts },
  });
  const one = selector => document.querySelector(selector);
  const input = () => one('.seek-rail input');
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const assert = (result, label) => { if (!result) throw Error(label); };
  async function wait(test, label) {
    for (let i = 0; i < 240; i++) {
      const result = await test();
      if (result) return result;
      await sleep(60);
    }
    throw Error(label);
  }
  const seeks = [];
  let restoreBufferFixture = () => {};
  const fetch = window.fetch;
  window.fetch = function(url, options) {
    if (String(url).split('/').at(-1) === 'player_action' && typeof options?.body === 'string') {
      const { action } = JSON.parse(options.body);
      if (action?.kind === 'seek_for') seeks.push(action.seconds);
    }
    return fetch.call(window, url, options);
  };
  function release(value) {
    const rail = input();
    rail.focus();
    rail.value = String(value);
    rail.dispatchEvent(new Event('input', { bubbles: true }));
    rail.dispatchEvent(new Event('change', { bubbles: true }));
  }
  try {
    const control = await wait(() => one('.track-row[data-track-id="1"] .track-play:not(:disabled)'),
      'synthetic song control');
    control.click();
    const { epoch } = await invoke('account_snapshot');
    const read = () => invoke('player_snapshot', { epoch });
    const started = await wait(async () => {
      const state = await read();
      return state.state === 'playing' && state.timeline?.mode === 'trial' && state;
    }, 'native partial trial source');
    await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    await wait(() => one('.playerbar')?.dataset.state === 'paused' && !input()?.disabled,
      'paused native trial');
    assert(input().min === '0' && input().max === '100', 'rail uses full song duration');
    assert(Number(input().step) === .1, 'trial uses the full visible clock for its step');
    assert(input().getAttribute('aria-valuetext').includes('可试听 00:20 至 00:40'),
      'announces authorized trial span');
    const lane = one('.trial-seek-track').getBoundingClientRect();
    for (const [selector, expected] of [['.trial-seek-start', .2], ['.trial-seek-end', .4]]) {
      const marker = one(selector).getBoundingClientRect();
      assert(Math.abs((marker.left + marker.width / 2 - lane.left) / lane.width - expected) < .01,
        'trial marker absolute position');
    }

    if (window.__trialBufferProof === true) {
      // Adjust only the advisory field on the real Tauri player's channel.
      // Its account/play id, clock, pause and seek replies remain native.
      const callbacks = window.__TAURI_INTERNALS__.callbacks;
      const get = callbacks.get;
      callbacks.get = function(id) {
        const callback = get.call(this, id);
        if (!callback) return callback;
        return (envelope) => {
          const update = envelope?.message;
          const view = update?.value;
          if (update?.kind === 'state' && view?.epoch === epoch &&
              view.playId === started.playId && view.timeline?.mode === 'trial') {
            callback({ ...envelope, message: { ...update, value: { ...view, bufferedThrough: 60 } } });
          } else {
            callback(envelope);
          }
        };
      };
      restoreBufferFixture = () => { delete callbacks.get; };
    }
    release(30);
    const inRange = await wait(async () => {
      const state = await read();
      return Math.abs(state.position - 30) < .2 && state;
    }, 'in-range native seek');
    assert(inRange.playId === started.playId && inRange.state === 'paused', 'seek preserves playback owner');
    assert(seeks.length === 1 && seeks[0] === 30, 'one fenced seek reaches IPC');
    await emit('ux_01_trial_inside', { bounds: [20, 40], total: 100, seekCount: seeks.length });
    await sleep(400);
    if (window.__trialBufferProof === true) {
      const segment = await wait(() => one('.seek-buffered'), 'buffered rail segment');
      const rect = segment.getBoundingClientRect();
      const left = (rect.left - lane.left) / lane.width;
      const right = (rect.right - lane.left) / lane.width;
      assert(Math.abs(left - .3) < .01 && Math.abs(right - .4) < .01 && rect.height > 0,
        'cache starts at actual seek clock and clips at trial boundary');
      assert(segment.parentElement.getAttribute('aria-hidden') === 'true' &&
        getComputedStyle(segment).pointerEvents === 'none', 'cache does not intercept seek');
      const rail = input();
      rail.value = '34';
      rail.dispatchEvent(new Event('input', { bubbles: true }));
      await wait(() => !one('.seek-buffered'), 'draft hides cache');
      rail.dispatchEvent(new Event('pointercancel', { bubbles: true }));
      await wait(() => one('.seek-buffered'), 'cancel restores cache');
      await emit('ux_03_trial_buffer', {
        observedSpan: [left, right], injectedAdvisoryEnd: 60, seekCount: seeks.length,
      });
      restoreBufferFixture();
      restoreBufferFixture = () => {};
    }

    for (const candidate of [10, 70]) {
      release(candidate);
      await wait(() => one('.playback-message')?.textContent.includes('只能在试听片段内调整播放进度'),
        'out-of-range explanation');
      await sleep(120);
      const after = await read();
      assert(after.state === 'paused' && after.playId === started.playId &&
        Math.abs(after.position - inRange.position) < .05 && Math.abs(Number(input().value) - 30) < .2,
      'out-of-range release restores thumb and retains native position');
      assert(document.activeElement === input(), 'out-of-range release retains keyboard focus');
      assert(seeks.length === 1, 'outside range must not issue IPC seek');
    }
    await emit('ux_02_trial_outside', { seekCount: seeks.length, blocked: [10, 70] });
    await sleep(400);
    await emit('complete', { ok: true, seekCount: seeks.length });
  } catch (error) {
    await emit('failed', { message: String(error?.message ?? error), seekCount: seeks.length });
  } finally {
    restoreBufferFixture();
    window.fetch = fetch;
  }
})();
