// Real library, guest player and native queue with synthetic files only.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const button = (label) => [...document.querySelectorAll('button')].find(
    (item) => item.getAttribute('aria-label') === label || item.textContent.trim() === label,
  );
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, value = {}) => invoke('plugin:event|emit', { event: 'local-proof', payload: { stage, ...value } });
  const assert = (value, label) => { if (!value) throw Error(label); };
  async function wait(test, label) {
    for (let i = 0; i < 240; i++) {
      const value = await test();
      if (value) return value;
      await new Promise((done) => setTimeout(done, 100));
    }
    throw Error(label);
  }
  function click(item) {
    assert(item && !item.disabled, 'missing or disabled control');
    item.focus({ preventScroll: true });
    item.click();
  }
  async function filter(word, count) {
    const input = one('[aria-label="搜索本地音乐"]');
    input.value = word;
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await wait(() => one('.local-page')?.dataset.localLoading === 'false' &&
      document.querySelectorAll('.local-tracks tbody tr').length === count, 'complete filter');
  }
  async function add() {
    click(button('更多本地音乐操作'));
    await wait(() => button('添加全部至播放列表') && !button('添加全部至播放列表').disabled, 'enabled queue action');
    click(button('添加全部至播放列表'));
    await wait(() => one('.local-page .notice')?.textContent.includes('已添加至播放列表') &&
      !one('.action-menu'), 'local queue accepted');
    assert(document.activeElement === button('更多本地音乐操作'), 'queue menu restores focus');
  }
  try {
    await wait(() => one('.welcome'), 'guest startup');
    const { epoch } = await wait(async () => {
      const state = await invoke('account_snapshot');
      return ['signed_out', 'expired'].includes(state.status) && state;
    }, 'settled guest snapshot');
    click(button('本地音乐'));
    await wait(() => one('.local-page'), 'local page');
    await invoke('local_command', { command: { kind: 'watch', path: window.__localProof.root + '/media' } });
    await wait(() => document.querySelectorAll('.local-tracks tbody tr').length === 4, 'native file scan');
    await filter('夜行 · 下篇', 1);
    await add();
    await wait(() => one('.playerbar [aria-label="暂停"]') && Number(one('[aria-label="播放进度"]')?.value) > 0.1, 'empty queue starts native CUE');
    const before = await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    assert(before.queueLen === 1 && before.timeline.mode === 'cue', 'one local item playing');
    await emit('local_queue_started', { queue: before.queueLen, cue: true });
    await filter('夜行', 2);
    await add();
    await filter('雨声', 1);
    await add();
    const after = await invoke('player_snapshot', { epoch });
    const queue = await invoke('player_queue', { epoch });
    const names = queue.tracks.map((entry) => entry.track.name);
    assert(names.join(',') === '夜行 · 下篇,雨声,夜行 · 上篇', 'insert after the current song, preserve prior suffix');
    assert(queue.tracks.every((entry) => entry.track.local), 'all rows retain offline identity');
    assert(after.playId === before.playId && after.entryId === before.entryId &&
      after.state === before.state && Math.abs(after.position - before.position) < 0.05, 'retain paused playback');
    assert(document.documentElement.scrollWidth <= innerWidth &&
      one('.local-page').scrollWidth <= one('.local-page').clientWidth + 1, 'no horizontal clipping');
    await emit('complete', { ok: true, queue: names, retainedPlayback: true, realMusicRequests: 0, audioOutput: 'null', width: innerWidth });
  } catch (error) {
    await emit('failed', { message: String(error) });
  }
})();
