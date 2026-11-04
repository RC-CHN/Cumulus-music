// Actual production WebView and native player with a closed, versioned style fixture.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
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
  const row = (id) => one(`.track-row[data-track-id="${id}"]`);
  const listed = (...ids) => all('.track-row').map((node) => Number(node.dataset.trackId)).join() === ids.join();
  const click = (node, label) => {
    if (!node || node.disabled) throw new Error(`unavailable: ${label}`);
    node.focus({ preventScroll: true });
    node.click();
  };
  const back = () => click(one('[aria-label="返回上一页"]'), 'Back');
  const refresh = () => click(one('[aria-label="刷新当前页面"]'), 'style refresh');
  const homeCard = () => one('[aria-label="打开 合成首页风格 B"]');
  const seeded = (tag) => one('.page-heading h1')?.textContent.trim() === '首页风格推荐' &&
    one('.page-heading p')?.textContent.includes(`合成${tag}标签`);
  try {
    await wait(() => one('.daily-style-entry') && one('.track-row'), 'daily ready');
    await emit('resize', { width: 760, height: 540 });
    await wait(() => innerWidth === 760 && innerHeight === 540, 'compact window');
    click(one('.daily-style-entry'), 'daily style entry');
    await wait(() => one('.style-home-entry') && listed(71, 72), 'default style version 1');
    const { epoch } = await invoke('account_snapshot');
    const player = () => invoke('player_snapshot', { epoch });
    const queue = async () => (await invoke('player_queue', { epoch })).tracks.map(({ track }) => track.id);
    const play = async (id, expectedQueue) => {
      click(row(id)?.querySelector('.track-play'), `play ${id}`);
      await wait(async () => {
        const snapshot = await player();
        return snapshot.track?.id === String(id) && snapshot.state === 'playing' &&
          (await queue()).join() === expectedQueue.join();
      }, `native player and exact queue for ${id}: ${expectedQueue}`);
      return { track: (await player()).track.id, queue: await queue() };
    };
    const original = await play(71, [71, 72]);
    const originalPlayId = (await player()).playId;
    click(one('.style-home-entry'), 'style browser');
    const popupCard = () => all('.style-home-card').find((node) => node.textContent.includes('合成首页风格 A'));
    await wait(popupCard, 'homepage A card in style popup');
    if (popupCard().outerHTML.includes('secret=synthetic')) throw new Error('raw seed URL exposed in popup');
    await emit('ux_01_style_default_and_cards', original);
    click(popupCard(), 'homepage A card');
    await wait(() => seeded('A') && listed(81, 82) &&
      document.activeElement === one('.page-heading h1'), 'seed A version 1 and heading focus');
    if (one('.style-tags-entry,.style-home-entry')) throw new Error('seeded page exposes default editor');
    click(row(81)?.querySelector('.track-more'), 'seed A song menu');
    await wait(() => one('.song-action-menu'), 'seed A song menu open');
    const next = [...one('.song-action-menu').querySelectorAll('button')]
      .find((button) => button.textContent.trim() === '下一首播放');
    click(next, 'add seed A song next');
    await wait(async () => (await queue()).join() === '71,81,72', 'seed A song inserted after active entry');
    if ((await player()).playId !== originalPlayId) throw new Error('insertion interrupted playback');
    await emit('ux_02_style_seed_a', { track: (await player()).track.id, queue: await queue() });

    // A seeded page cannot open the style chooser. The real Home sidebar supplies B.
    click(all('.sidebar button').find((button) => button.textContent.trim() === '首页'), 'Home sidebar');
    await wait(homeCard, 'homepage B card');
    if (homeCard().outerHTML.includes('secret=synthetic')) throw new Error('raw seed URL exposed in Home');
    click(homeCard(), 'homepage B card');
    await wait(() => seeded('B') && listed(91, 92), 'seed B version 1');
    await emit('ux_03_style_seed_b', await play(91, [91, 92]));

    back();
    await wait(() => homeCard() && document.activeElement === homeCard(), 'Home and seed B card focus after Back');
    back();
    await wait(() => seeded('A') && listed(81, 82), 'Back to seed A must restore the original 81/82 list');
    await emit('ux_04_style_back_seed_a', await play(82, [81, 82]));

    refresh();
    await wait(() => seeded('A') && listed(86, 87), 'explicit seed A refresh updates songs');
    await emit('ux_05_style_refresh_seed_a', await play(87, [86, 87]));

    back();
    await wait(() => one('.style-tags-entry') && listed(71, 72) &&
      document.activeElement === one('.style-home-entry'), 'Back to default original 71/72 list and launcher focus');
    await emit('ux_06_style_back_default', await play(72, [71, 72]));

    refresh();
    await wait(() => one('.style-tags-entry') && listed(76, 77), 'explicit default refresh updates songs');
    await emit('ux_07_style_refresh_default', await play(77, [76, 77]));
    await emit('complete', { ok: true, stageCount: 7 });
  } catch (error) {
    await emit('failed', { message: String(error), stack: error?.stack });
  }
})();
