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
    await emit('complete', { ok: true, stageCount: 2 });
  } catch (error) {
    await emit('failed', { message: String(error), body: document.body.innerText.slice(-2200) });
  }
})();
