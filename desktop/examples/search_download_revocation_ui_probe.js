// Revoke a synthetic, already held download while its real PCM graph is paused.
(async () => {
  const query = '合成下载优先';
  const track = '881';
  const invoke = (command, args) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, info = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...info } });
  const one = (selector) => document.querySelector(selector);
  const cover = () => one(`[data-result-key="song:${track}"] .search-track-cover`);
  const assert = (condition, message) => { if (!condition) throw Error(message); };
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  async function wait(read, message) {
    for (let attempt = 0; attempt < 240; attempt++) {
      const value = await read();
      if (value) return value;
      await sleep(65);
    }
    throw Error(message);
  }
  let epoch, step = 'startup', count = 0, last;
  const snapshot = async () => (last = await invoke('player_snapshot', { epoch }));
  const action = (value) => invoke('player_action', { epoch, action: value });
  async function capture(label, view) {
    step = label;
    const stage = `ux_${String(++count).padStart(2, '0')}_${label}`;
    await emit(stage, { view, captureHandshake: true });
    await wait(() => window.__searchDownloadStageRecorded === stage, `native receipt for ${stage}`);
  }
  try {
    ({ epoch } = await wait(async () => {
      const account = await invoke('account_snapshot');
      return account.status === 'authenticated' && one('[aria-label="搜索音乐"]') && account;
    }, 'synthetic account and real search ready'));
    await emit('search-download-fixture', { mode: 'publish' });
    const receipt = await wait(() => window.__searchDownloadFixtureAck, 'isolated publication receipt');
    assert(receipt.ok, receipt.message);
    const prior = await snapshot();
    const saved = await action({ kind: 'equalizer', expected_revision: prior.equalizer.revision,
      settings: { enabled: true, gains: [3, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        tone_enabled: true, bass_db: 2, presence: 3, stereoizer: 4 } });
    assert(saved.equalizer.revision !== prior.equalizer.revision &&
      saved.equalizer.settings.presence === 3, 'native local controls committed');
    await action({ kind: 'mode', mode: 'sequential' });

    step = 'search';
    const field = one('[aria-label="搜索音乐"]');
    field.value = query;
    field.dispatchEvent(new Event('input', { bubbles: true }));
    field.form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    const play = await wait(() =>
      one('.search-results-page h1')?.textContent === query &&
      one('#search-result-panel')?.getAttribute('aria-busy') === 'false' &&
      cover()?.getAttribute('aria-label') === '播放 合成下载歌曲' && cover(),
    'real search row');
    play.click();
    const playing = await wait(async () => {
      const state = await snapshot();
      assert(!state.error, 'native graph did not fail before revocation');
      return state.state === 'playing' && state.position >= .18 && state.catalogFileSource &&
        state.equalizer?.spatial_status === 'processing' && state;
    }, 'the native clock and processed PCM both advance');
    // Pause before the screenshot handshake; the proof must not race the
    // short fixture's EOF. This is still the active loaded PCM generation.
    await action({ kind: 'pause', paused: true });
    const paused = await wait(async () => {
      const state = await snapshot();
      return state.state === 'paused' && state.playId === playing.playId &&
        state.entryId === playing.entryId && state.catalogFileSource && state;
    }, 'same held PCM generation is paused');
    await capture('revoke_ready', { playing, paused });
    // The harness removes only this synthetic publication's ledger before
    // releasing the capture. The ordinary actor must stop it on its own.
    const revoked = await wait(async () => {
      const state = await snapshot();
      return state.state === 'error' && state.error && state;
    }, 'the active source retires on durable revocation');
    assert(!revoked.playId && !revoked.catalogFileSource && revoked.position == null &&
      revoked.queueRevision === paused.queueRevision, 'revocation clears only transport');
    await sleep(350);
    const settled = await snapshot();
    assert(settled.state === 'error' && settled.error && !settled.playId &&
      settled.queueRevision === paused.queueRevision, 'no automatic EOF, retry or next play');
    await capture('revoked', { revoked, settled });
    await emit('complete', { ok: true, stageCount: count, realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', { step, count, message: String(error), last });
  }
})();
