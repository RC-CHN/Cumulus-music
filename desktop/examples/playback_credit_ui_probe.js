// Optional catalog fixture script. All tracks, identities and responses are synthetic.
(async () => {
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, payload = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...payload } });
  const one = (selector) => document.querySelector(selector);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  async function wait(read, label) {
    for (let attempt = 0; attempt < 180; attempt++) {
      const value = await read();
      if (value) return value;
      await sleep(75);
    }
    throw Error(label);
  }
  try {
    const play = await wait(() => one('.track-row .track-play:not(:disabled)'), 'synthetic track');
    play.click();
    const { epoch } = await invoke('account_snapshot');
    const read = () => invoke('player_snapshot', { epoch });
    const started = await wait(async () => {
      const state = await read();
      return state.state === 'playing' && state.position > .3 && state;
    }, 'native playback');
    for (const [kind, expected] of [['album', '测试专辑'], ['artist', '测试歌手']]) {
      one('[aria-label="打开播放详情"]').click();
      const button = await wait(
        () => one(`.lyrics-panel button[aria-label="查看${kind === 'album' ? '专辑' : '歌手'}：${expected}"]`),
        'current playback credit',
      );
      await wait(() => {
        const rect = button.getBoundingClientRect();
        return rect.width > 0 && document.elementFromPoint(
          rect.left + rect.width / 2, rect.top + rect.height / 2,
        ) === button;
      }, 'credit painted in the viewport');
      await sleep(300);
      button.focus();
      if (document.activeElement !== button) throw Error('credit cannot receive keyboard focus');
      await emit(`ux_${kind}_credit`, { playing: true });
      button.click();
      await wait(() => !one('.lyrics-panel') && one('.catalog-heading h1')?.textContent === expected,
        'catalog destination with playback detail closed');
      await sleep(300);
      const state = await read();
      if (state.state !== 'playing' || state.playId !== started.playId ||
          state.queueRevision !== started.queueRevision || state.position < started.position)
        throw Error('credit navigation changed native playback');
      await emit(`ux_${kind}_opened`, { playing: true, title: expected });
    }
    await sleep(400); // Older local probe hosts poll for the last screenshot.
    await emit('complete', { ok: true, samePlayback: true, syntheticCatalog: true });
  } catch (error) {
    await emit('failed', { error: String(error?.message ?? error) });
  }
})();
