// Actual Vue controls and typed IPC in a single production WebView.
(async () => {
  if (window.__signedWindowStarted) return;
  window.__signedWindowStarted = true;
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, payload = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...payload },
  });
  const one = selector => [...document.querySelectorAll(selector)].find(node =>
    node.getClientRects().length && !node.closest('[inert],.page-leave-active,.menu-leave-active'));
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const assert = (ok, label) => { if (!ok) throw new Error(label); };
  const wait = async (test, label) => {
    for (let n = 0; n < 300; n++) {
      const result = await test();
      if (result) return result;
      await sleep(50);
    }
    throw new Error(label);
  };
  const click = async (selector, label) => {
    const node = await wait(() => {
      const button = one(selector);
      return button && !button.disabled && button.getAttribute('aria-disabled') !== 'true' && button;
    }, label);
    node.focus({ preventScroll: true });
    node.click();
  };
  const bar = () => one('.playerbar');
  const stage = async name => {
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await emit(`ux_${name}`, { playerState: bar()?.dataset.state ?? null });
  };
  try {
    const account = await wait(async () => {
      const result = await invoke('account_snapshot');
      return result.status === 'authenticated' && result.nickname === '合成窗口账号' &&
        one('.account-button')?.title === result.nickname && result;
    }, 'same window did not restore synthetic account');
    const own = await wait(() => [...document.querySelectorAll('button.nav-item')].find(node =>
      node.textContent.trim() === '我的歌单' && node.getClientRects().length && !node.disabled),
    'own playlist navigation missing');
    own.click();
    await wait(() => one('.playlist-library-page [aria-label="合成窗口歌单"]'),
      'own playlist did not render');
    await stage('account_resources');

    const word = '合成窗口检索';
    const input = await wait(() => one('.searchbox input'), 'search input missing');
    input.value = word;
    input.dispatchEvent(new Event('input', { bubbles: true }));
    one('.searchbox').dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await wait(() => one('.search-heading-copy h1')?.textContent === word, 'fixed search heading missing');
    await click('[data-category="songs"]', 'songs tab missing');
    const row = '[aria-labelledby="search-tab-songs"] [data-track-id="41"]';
    await wait(() => one(row)?.textContent.includes('合成窗口音频') &&
      one(`${row} [aria-label="取消喜欢 合成窗口音频"]`), 'fixed result or own liked state missing');
    await stage('search_song');

    await click(`${row} .track-more`, 'fixed row menu missing');
    await click('.song-action-menu [data-menu-key="next"]', 'search next-play action missing');
    const player = () => invoke('player_snapshot', { epoch: account.epoch });
    const playing = await wait(async () => {
      const result = await player();
      return result.track?.id === '41' && result.state === 'playing' && result.position > 0.25 &&
        result.queueLen === 1 && result.playId && bar()?.dataset.state === 'playing' && result;
    }, 'verified AAC did not advance on native clock');
    await stage('playing');

    await click('.playerbar [aria-label="暂停"]', 'pause missing');
    const paused = await wait(async () => {
      const result = await player();
      return result.state === 'paused' && bar()?.dataset.state === 'paused' && result;
    }, 'native pause missing');
    await sleep(360);
    const still = await player();
    assert(still.playId === playing.playId && still.entryId === playing.entryId &&
      Math.abs(still.position - paused.position) < 0.05, 'native pause did not hold the same clock');
    await stage('paused');

    await click('.playerbar [aria-label="播放"]', 'resume missing');
    await wait(async () => {
      const result = await player();
      return result.state === 'playing' && result.playId === paused.playId &&
        result.entryId === paused.entryId && result.position > still.position + 0.15 &&
        bar()?.dataset.state === 'playing';
    }, 'same native entry did not resume');
    await stage('resumed');

    await click('.playerbar [aria-label="停止播放"]', 'stop missing');
    await wait(async () => (await player()).state === 'idle' && bar()?.dataset.state === 'idle',
      'native stop missing');
    await stage('stopped');
    await emit('complete', { playerState: 'idle' });
  } catch (error) {
    await emit('failed', { error: error instanceof Error ? error.message : 'unknown renderer failure' });
  }
})();
