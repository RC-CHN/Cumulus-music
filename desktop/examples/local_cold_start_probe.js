// Real local page + native player while a closed synthetic account request waits.
(async () => {
  const one = (s) => document.querySelector(s);
  const button = (text) => [...document.querySelectorAll('button')].find((n) => n.textContent.trim() === text);
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, facts = {}) => invoke('plugin:event|emit', { event: 'local-proof', payload: { stage, ...facts } });
  const assert = (ok, reason) => { if (!ok) throw Error(reason); };
  async function wait(check, reason) {
    for (let i = 0; i < 250; i++) {
      if (await check()) return;
      await new Promise((resolve) => setTimeout(resolve, 40));
    }
    throw Error(reason);
  }
  const clock = () => Number(one('.playerbar')?.dataset.position || 0);
  let step = 'waiting for native account request';
  try {
    await wait(async () => {
      await emit('inspect_restore');
      return window.__coldRestoreStarted === true;
    }, step);
    const account = one('.account-button');
    assert(account?.disabled && button('每日推荐')?.disabled, 'online startup gates must remain in place');
    assert(button('本地音乐') && !button('本地音乐').disabled, 'local navigation enabled');
    step = 'local navigation and native scan';
    button('本地音乐').click();
    await wait(() => one('.local-page'), step);
    await invoke('local_command', { command: { kind: 'watch', path: window.__localProof.root + '/media' } });
    const play = () => one('[aria-label="播放本地歌曲 夜行 · 下篇"]');
    await wait(() => play() && !play().disabled, 'local scan and player ready before restore');
    const localPage = one('.local-page');
    step = 'guest local audio while account request is held';
    play().click();
    await wait(() => one('.playerbar')?.dataset.state === 'playing', step);
    const start = clock();
    await wait(() => clock() > start + 0.3, 'local clock advances while restore is held');
    assert(account.disabled, 'local audio must precede restore completion');
    const before = clock();
    await emit('local_cold_playing', { pending: true, advancing: true });
    step = 'survive account timeout';
    await emit('release_restore');
    await wait(() => !account.disabled && clock() > before + 0.2, step);
    assert(localPage === one('.local-page') && one('.playerbar')?.dataset.state === 'playing', 'same local page and player after timeout');
    await emit('complete', { ok: true, localWhilePending: true, keptLocalPage: true, keptPlayback: true, realMusicRequests: 0 });
  } catch (error) {
    await emit('failed', { ok: false, step, reason: String(error) });
  }
})();
