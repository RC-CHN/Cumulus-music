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
    const { epoch } = await invoke('account_snapshot');
    one('.track-row[data-track-id="81"] .track-play').click();
    await wait(async () => {
      const snapshot = await invoke('player_snapshot', { epoch });
      return snapshot.track?.id === '81' && snapshot.state === 'playing';
    }, 'seeded style reaches native playback');
    await emit('ux_03_style_home_playing');
    one('[aria-label="返回上一页"]').click();
    await wait(() => one('.style-tags-entry') && one('.track-row[data-track-id="71"]') &&
      document.activeElement === one('.style-home-entry'), 'restored default style and launcher focus');
    await emit('complete', { ok: true, stageCount: 3 });
  } catch (error) {
    await emit('failed', { message: String(error), stack: error?.stack });
  }
})();
