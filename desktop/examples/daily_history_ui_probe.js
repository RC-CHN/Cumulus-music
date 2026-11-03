// Real daily-page navigation through the production member IPC.
(async () => {
  const one = (s) => document.querySelector(s);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const emit = (stage, data = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const wait = async (predicate, message) => {
    for (let index = 0; index < 200; index++) {
      const found = predicate();
      if (found) return found;
      await sleep(65);
    }
    throw new Error(message);
  };
  try {
    await wait(() => one('.daily-heading') && one('.track-row'), 'daily page ready');
    await emit('resize', { width: 760, height: 540 });
    await wait(() => innerWidth === 760 && innerHeight === 540, 'compact window');
    const shortcut = await wait(() => one('.daily-history-entry'), 'history shortcut');
    const actions = one('.daily-track-actions');
    const links = one('.daily-links');
    const playAll = actions?.querySelector('.primary');
    if (!actions?.contains(links) || !links.contains(shortcut) || !playAll ||
        links.getBoundingClientRect().height !== 36 ||
        links.getBoundingClientRect().left <= playAll.getBoundingClientRect().right)
      throw new Error('daily shortcuts are not grouped at the right of playback controls');
    const bounds = shortcut.getBoundingClientRect();
    if (!shortcut.contains(document.elementFromPoint(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2))) throw new Error('history shortcut clipped');
    await sleep(200);
    await emit('ux_01_daily_history_shortcut');
    shortcut.click();
    await wait(() => one('.member-page')?.getAttribute('aria-busy') === 'false' &&
      one('.member-page .track-row')?.textContent.includes('合成历史日推单曲'), 'selected history loaded');
    if (one('.member-tabs button[aria-pressed="true"]')?.textContent.trim() !== '历史日推') throw new Error('wrong member section');
    await sleep(250);
    await emit('ux_02_history_from_daily');
    await sleep(500);
    one('[aria-label="返回上一页"]').click();
    await wait(() => one('.daily-heading') && one('.daily-history-entry') && one('.track-row'), 'returned to daily');
    const style = one('.daily-style-entry');
    const styleBounds = style.getBoundingClientRect();
    if (!style.contains(document.elementFromPoint(styleBounds.x + styleBounds.width / 2, styleBounds.y + styleBounds.height / 2)))
      throw new Error('style shortcut clipped');
    style.click();
    await wait(() => [...document.querySelectorAll('.page-heading h1')].some((heading) =>
      heading.textContent.trim() === '风格推荐') && one('.track-row')?.textContent.includes('合成风格歌曲 71'), 'style recommendations loaded');
    await sleep(250);
    await emit('ux_03_style_recommendations');
    const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
    const { epoch } = await invoke('account_snapshot');
    await wait(() => one('.track-actions .primary:not(:disabled)'), 'style play available');
    one('.track-actions .primary').click();
    let player;
    for (let i = 0; i < 150; i++) {
      player = await invoke('player_snapshot', { epoch });
      if (player.track?.id === '71' && player.state === 'playing' && player.position > 0.15) break;
      await sleep(65);
    }
    if (player?.track?.id !== '71' || player.queueLen !== 2 || player.state !== 'playing' || player.position <= 0.15)
      throw new Error('style songs did not reach ordinary native playback');
    const tagEntry = one('.style-tags-entry');
    tagEntry.click();
    const tags = await wait(() => one('.style-tag-browser[open]')?.textContent.includes('合成轻音乐') && one('.style-tag-browser'), 'style tags loaded');
    await sleep(250);
    await emit('ux_04_style_tags');
    [...tags.querySelectorAll('nav button')].find((node) => node.textContent.trim() === '合成空分类').click();
    await wait(() => tags.textContent.includes('此分类暂无标签。'), 'style category changes without a write');
    tags.querySelector('[aria-label="关闭风格标签"]').click();
    await wait(() => !one('.style-tag-browser[open]') && document.activeElement === tagEntry, 'style tag focus restored');
    await emit('complete', { ok: true, stageCount: 4, stylePlayback: true });
  } catch (error) {
    await emit('failed', { message: String(error), body: document.body.innerText.slice(-2200) });
  }
})();
