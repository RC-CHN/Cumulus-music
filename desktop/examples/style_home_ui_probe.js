// Production WebView, isolated official-shape synthetic style feed.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const wait = async (predicate, label) => {
    for (let index = 0; index < 160; index++) {
      if (await predicate()) return;
      await new Promise((resolve) => setTimeout(resolve, 65));
    }
    throw new Error(label);
  };
  try {
    await wait(() => one('.daily-style-entry') && one('.track-row'), 'daily ready');
    await emit('resize', { width: 760, height: 540 });
    await wait(() => innerWidth === 760 && innerHeight === 540, 'compact window');
    one('.daily-style-entry').click();
    await wait(() => one('.style-home-entry') && one('.track-row'), 'default style ready');
    const { epoch } = await invoke('account_snapshot');
    const player = () => invoke('player_snapshot', { epoch });
    one('.track-row[data-track-id="71"] .track-play').click();
    await wait(async () => {
      const snapshot = await player();
      return snapshot.track?.id === '71' && snapshot.state === 'playing';
    }, 'default style starts a queue');
    const original = await player();
    one('.style-home-entry').focus();
    one('.style-home-entry').click();
    await wait(() => one('.style-home-card')?.textContent.includes('合成首页风格'), 'homepage card ready');
    await emit('ux_01_style_home_cards');
    one('.style-home-card').click();
    await wait(() => one('.page-heading h1')?.textContent.trim() === '首页风格推荐' &&
      one('.page-heading p')?.textContent.includes('合成首页标签') &&
      one('.track-row[data-track-id="81"]') && document.activeElement === one('.page-heading h1'),
    'seeded recommendation and focus ready');
    if (one('.style-tags-entry,.style-home-entry')) throw new Error('seeded page exposes default editor');
    await emit('ux_02_style_home_seeded');
    one('.track-row[data-track-id="81"] .track-more').click();
    await wait(() => one('.song-action-menu'), 'seeded song menu');
    const next = [...one('.song-action-menu').querySelectorAll('button')]
      .find((button) => button.textContent.trim() === '下一首播放');
    if (!next || next.disabled) throw new Error('seeded next playback unavailable');
    next.click();
    await wait(async () => {
      const queue = await invoke('player_queue', { epoch });
      return queue.tracks[0]?.track.id === '71' && queue.tracks[1]?.track.id === '81';
    }, 'seeded song inserted after the active entry');
    if ((await player()).playId !== original.playId) throw new Error('insertion interrupted playback');
    await emit('ux_03_style_home_queued');
    one('.track-row[data-track-id="81"] .track-play').click();
    await wait(async () => {
      const snapshot = await player();
      return snapshot.track?.id === '81' && snapshot.state === 'playing';
    }, 'seeded style reaches native playback');
    await emit('ux_04_style_home_playing');
    one('[aria-label="返回上一页"]').click();
    await wait(() => one('.style-tags-entry') && one('.track-row[data-track-id="71"]') &&
      document.activeElement === one('.style-home-entry'), 'restored default style and launcher focus');
    await emit('complete', { ok: true, stageCount: 4 });
  } catch (error) {
    await emit('failed', { message: String(error), stack: error?.stack });
  }
})();
