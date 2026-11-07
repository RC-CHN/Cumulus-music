// Actual comprehensive search overlays; the backend and PCM are sealed fixtures.
(async () => {
  const query = '合成播单续播';
  const invoke = (command, args) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, extra = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...extra } });
  const one = (selector) => document.querySelector(selector);
  const play = (block, id = '7') =>
    one(`[data-result-section^="search_block_${block}:"] [data-result-key="podcast:${id}"] .search-card-play`);
  const best = () => play('best_match') || play('hot_result_core_resources');
  const bar = () => one('.playerbar');
  const visible = () =>
    one('.search-results-page h1')?.textContent === query &&
    one('#search-result-panel')?.getAttribute('aria-busy') === 'false';
  const ready = (node) => node?.isConnected && !node.disabled && node;
  const assert = (value, message) => { if (!value) throw Error(message); };
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  async function wait(read, message) {
    for (let attempt = 0; attempt < 250; attempt++) {
      const value = await read();
      if (value) return value;
      await sleep(60);
    }
    throw Error(message);
  }
  let epoch, step = 'startup', count = 0, last;
  const snapshot = async () => (last = await invoke('player_snapshot', { epoch }));
  const synced = (state) => bar()?.dataset.playId === state.playId &&
    bar().dataset.trackId === '1001' && bar().dataset.state === state.state;
  const stateIs = (state, from) => async () => {
    const native = await snapshot();
    if (native.state !== state || !synced(native)) return false;
    assert(native.playId && native.track.voiceId === '1' && native.resourceKind === 'voice',
      'the selected native media must be episode 1, not song or directory playback');
    assert(native.queueLen === 251 && native.mode === 'sequential' &&
      native.origin?.kind === 'podcast' && native.origin.id === '7', 'full ordered native podcast queue');
    if (from) {
      assert(native.playId === from.playId && native.entryId === from.entryId &&
        native.queueRevision === from.queueRevision, 'same-source toggle replaced native identity');
    }
    return native;
  };
  async function observedPlaying(from) {
    const read = stateIs('playing', from);
    const first = await wait(read, 'actual decoder starts or continues');
    assert(first.position >= 18.8 && first.position < 27, 'starts at saved 19s checkpoint');
    const advanced = await wait(async () => {
      const current = await read();
      return current && current.position > first.position + .12 && current;
    }, 'same real decoder advances its clock');
    return { first, current: advanced };
  }
  async function capture(label, observed) {
    assert(visible(), 'playback overlay must stay on comprehensive search');
    const stage = `ux_${String(++count).padStart(2, '0')}_${label}`;
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await sleep(120);
    await emit(stage, { observed, query, matchingBest: best()?.getAttribute('aria-label'),
      bestSection: best()?.closest('[data-result-section]')?.dataset.resultSection,
      matchingRegular: play('voicelist')?.getAttribute('aria-label') });
    await wait(() => window.__searchPodcastPlaybackRecorded === stage, `native sample ${stage}`);
  }
  try {
    ({ epoch } = await wait(async () => {
      const account = await invoke('account_snapshot');
      return account.status === 'authenticated' && one('.track-row') &&
        !one('.page-enter-active,.page-leave-active') &&
        ready(one('[aria-label="搜索音乐"]')) && account;
    }, 'synthetic authenticated desktop'));
    step = 'search';
    const input = one('[aria-label="搜索音乐"]');
    input.value = query;
    input.dispatchEvent(new Event('input', { bubbles: true }));
    input.form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await wait(() => visible() && ready(best()) && ready(play('voicelist')) &&
      ready(play('voicelist', '8')), 'best match, regular and failing radio overlays');
    const startsRegular = !play('best_match');
    const firstOverlay = startsRegular ? () => play('voicelist') : best;
    const secondOverlay = startsRegular ? best : () => play('voicelist');
    const firstName = startsRegular ? 'regular' : 'best';
    const secondName = startsRegular ? 'hot_core' : 'regular';
    step = `${firstName}-play`;
    firstOverlay().focus();
    firstOverlay().click();
    const started = await observedPlaying();
    await capture(`${firstName}_resumes`, started);
    step = `${secondName}-pause`;
    (await wait(() => ready(secondOverlay())?.getAttribute('aria-label')?.startsWith('暂停') &&
      secondOverlay(), 'other overlay for the same radio pauses')).click();
    const paused = await wait(stateIs('paused', started.current), 'same loaded decoder paused');
    await capture(`${secondName}_pauses`, { current: paused });
    step = 'different-radio-denied';
    (await wait(() => ready(play('voicelist', '8')), 'other radio overlay')).click();
    const error = await wait(() => [...document.querySelectorAll('.playback-message[role="status"]')]
      .find((node) => node.querySelector('span')?.textContent === '网络请求失败，请稍后重试' &&
        ready(node.querySelector('button[data-retry-key]'))), 'mapped network failure and explicit retry are visible');
    const held = await wait(stateIs('paused', paused), 'old paused queue after denied selection');
    assert(held.position === paused.position, 'denied radio moved the old clock');
    await capture('denied_keeps_queue', { current: held, message: error.querySelector('span').textContent });
    step = `${secondName}-resume`;
    (await wait(() => ready(secondOverlay())?.getAttribute('aria-label')?.startsWith('播放') &&
      secondOverlay(), 'other overlay for the same radio resumes')).click();
    await capture(`${secondName}_continues`, await observedPlaying(held));
    step = 'stop';
    (await wait(() => ready(bar()?.querySelector('[aria-label="停止播放"]')), 'explicit stop')).click();
    await wait(async () => (await snapshot()).state === 'idle', 'native stopped');
    await emit('complete', { ok: true, stageCount: count });
  } catch (error) {
    await emit('failed', { step, count, message: String(error), last, currentPage:
      one('.main-content')?.textContent.trim().slice(0, 800), bar: bar()?.outerHTML.slice(0, 500) });
  }
})();
