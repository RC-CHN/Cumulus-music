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
      if (!one('.lyrics-panel .vinyl-source')?.textContent.includes('未知') ||
          one('.lyrics-panel .vinyl-source button')) throw Error('unattributed selection gained a source');
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
    const profile = await wait(() => one('.artist-profile-link'), 'artist profile link');
    profile.focus();
    if (document.activeElement !== profile) throw Error('profile link cannot receive keyboard focus');
    profile.click();
    await wait(() => one('.playlist-creator-page h1')?.textContent === '合成歌手个人页',
      'the artist account ID opens the public profile');
    await sleep(300);
    const profileState = await read();
    if (profileState.state !== 'playing' || profileState.playId !== started.playId ||
        profileState.queueRevision !== started.queueRevision) throw Error('profile changed native playback');
    await emit('ux_artist_profile', { playing: true, title: '合成歌手个人页' });
    one('[aria-label="打开播放详情"]').click();
    (await wait(() => one('.lyrics-panel button[aria-label="查看专辑：测试专辑"]'), 'album credit')).click();
    await wait(() => !one('.lyrics-panel') && one('.catalog-heading h1')?.textContent === '测试专辑', 'album ready');
    (await wait(() => one('.album-actions button.primary:not(:disabled)'), 'album play all')).click();
    const albumState = await wait(async () => {
      const state = await read();
      return state.state === 'playing' && state.playId !== started.playId &&
        state.origin?.kind === 'album' && state.originName === '测试专辑' && state;
    }, 'album playback with original source');
    const daily = [...document.querySelectorAll('button')].find((button) => button.textContent.trim() === '每日推荐');
    if (!daily) throw Error('daily navigation not found');
    daily.click();
    await wait(() => one('.daily-heading') && !one('.catalog-heading'), 'navigate away while album plays');
    one('[aria-label="打开播放详情"]').click();
    const source = await wait(() => one('.lyrics-panel .vinyl-source button[aria-label="查看播放来源：测试专辑"]'), 'linked album source');
    await wait(() => {
      const box = source.getBoundingClientRect();
      return document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2) === source;
    }, 'source button painted');
    await sleep(300);
    source.focus();
    if (document.activeElement !== source) throw Error('source button not keyboard accessible');
    await emit('ux_album_source', { playing: true, source: source.textContent.trim() });
    await sleep(350);
    source.click();
    await wait(() => !one('.lyrics-panel') && one('.catalog-heading h1')?.textContent === '测试专辑', 'source destination opened');
    const after = await read();
    if (after.state !== 'playing' || after.playId !== albumState.playId ||
        after.queueRevision !== albumState.queueRevision || after.origin?.id !== albumState.origin.id)
      throw Error('source navigation changed native playback');
    await sleep(300);
    await emit('ux_album_source_opened', { playing: true, title: '测试专辑' });
    await sleep(400); // Older local probe hosts poll for the last screenshot.
    await emit('complete', { ok: true, samePlayback: true, sourceNavigation: true, syntheticCatalog: true });
  } catch (error) {
    await emit('failed', { error: String(error?.message ?? error) });
  }
})();
