// Actual Vue/Tauri catalogue; the native proof host has only synthetic APIs.
(async () => {
  const one = selector => document.querySelector(selector);
  const all = selector => [...document.querySelectorAll(selector)];
  const page = () => one('[data-latest-releases]');
  const button = (label, root = page()) =>
    [...(root?.querySelectorAll('button') ?? [])].find(node => node.textContent.trim() === label);
  const wait = async (predicate, label) => {
    for (let i = 0; i < 180; i++) {
      if (predicate()) return;
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    throw Error(label);
  };
  const assert = (truth, label) => { if (!truth) throw Error(label); };
  const click = node => {
    assert(node && !node.disabled && node.getAttribute('aria-disabled') !== 'true', `inactive control: ${node?.outerHTML ?? '(missing)'}`);
    node.focus({ preventScroll: true });
    node.click();
  };
  const invoke = (name, args) => window.__TAURI_INTERNALS__.invoke(name, args);
  const send = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const scope = (label) => one(`[role=group][aria-label="${label}"]`);
  const back = () => click(one('[aria-label="返回上一页"]'));
  const count = () => Number(one('[data-latest-album-count]')?.dataset.latestAlbumCount ?? 0);
  const retry = () => button('重试');
  const albums = () => one('[data-latest-scroll]');
  const song = () => one('[data-latest-releases] .track-row');
  const tab = id => one(`[data-latest-focus="tab:${id}"]`);
  const snap = (name, data = {}) => send(`ux_${name}`, {
    tracksInDom: all('[data-latest-releases] .track-row').length,
    albumsInDom: all('[data-latest-releases] .catalog-album-card').length,
    albumTotal: count(), ...data,
  });
  try {
    const nav = () => button('新歌新碟', one('.sidebar') ?? document);
    await wait(() => nav() && !nav().disabled && nav().getAttribute('aria-disabled') !== 'true', 'discovery entry is available');
    click(nav());
    await wait(() => song()?.textContent.includes('新歌 0 · 1'), 'default song region');
    assert(tab('songs').getAttribute('aria-selected') === 'true', 'songs is the default tab');
    assert(all('[data-latest-releases] .track-row').length < 40, '100 songs remain virtualized');
    await snap('songs');

    const { epoch } = await invoke('account_snapshot', {});
    click(button('收藏全部'));
    await wait(() => one('dialog[open]')?.textContent.includes('收藏到歌单'), 'collection modal');
    await snap('collect');
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    const modal = one('dialog[open]');
    if (modal) {
      const close = modal.querySelector('[aria-label*="关闭"]');
      if (close) click(close);
    }
    await wait(() => !one('dialog[open]'), 'close collection without write');

    await wait(() => button('播放全部') && !button('播放全部').disabled, 'playback initialized');
    click(button('播放全部'));
    await wait(() => one('.playerbar')?.dataset.state === 'playing' &&
      Number(one('.playerbar').dataset.position) > .2, 'native audio is advancing');
    const player = await invoke('player_snapshot', { epoch });
    const queue = await invoke('player_queue', { epoch });
    const ids = queue.tracks.map(row => row.track.id);
    assert(player.track?.id === '1' && ids.length === 99 && !ids.includes('2'),
      'bulk playback keeps first ID and excludes the later denied track');
    await snap('playing', { playedId: player.track.id, queueLength: ids.length });
    const doubleClickCurrent = () => song().dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    doubleClickCurrent();
    await wait(() => one('.playerbar')?.dataset.state === 'paused', 'current double-click pauses');
    const paused = await invoke('player_snapshot', { epoch });
    assert(paused.playId === player.playId && paused.queueRevision === player.queueRevision,
      'current double-click preserves the instance and queue');
    await snap('current_double_click', { playId: paused.playId });
    doubleClickCurrent();
    await wait(() => one('.playerbar')?.dataset.state === 'playing', 'current double-click resumes');

    click(button('欧美', scope('地区')));
    await wait(retry, 'synthetic regional failure');
    await snap('song_error');
    click(retry());
    await wait(() => song()?.textContent.includes('新歌 96 · 1'), 'region retries successfully');
    click(button('播放全部'));
    await wait(() => one('[data-latest-playback-notice]'), 'first denied gives explicit notice');
    const preserved = await invoke('player_snapshot', { epoch });
    assert(preserved.playId === player.playId && preserved.queueRevision === player.queueRevision,
      'denied first does not silently start a different track');
    await snap('denied_first', { playId: preserved.playId });
    const third = one('[data-latest-releases] [data-track-id="3"]');
    assert(third, 'third online row is visible');
    third.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    let selected;
    for (let i = 0; i < 100; i++) {
      selected = await invoke('player_snapshot', { epoch });
      if (selected.track?.id === '3' && selected.state === 'playing') break;
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    assert(selected?.track?.id === '3' && selected.state === 'playing',
      'different row must play via the native scoped release receipt');
    await snap('different_double_click', { playedId: selected.track.id });
    click(button('小语种', scope('地区')));
    await wait(() => page()?.textContent.includes('暂无新歌'), 'empty song region');
    await snap('song_empty');

    click(tab('albums'));
    await wait(() => one('[data-latest-group="week"] h2'), 'weekly section');
    const card = () => one('[data-latest-group="week"] [data-catalog-key="album:7"]');
    await wait(card, 'weekly album entry');
    await snap('album_first');
    click(card());
    await wait(() => one('.album-header h1')?.textContent.includes('测试专辑'), 'weekly album details');
    await snap('album_detail');
    back();
    await wait(() => card() && document.activeElement === card(), 'album back focus');
    await snap('album_back');

    albums().scrollTop = albums().scrollHeight;
    await wait(retry, 'new album cursor failed');
    assert(count() >= 11, 'existing week/month preserved on error');
    await snap('album_error');
    click(retry());
    await wait(() => count() >= 21, 'same album cursor resumes');
    await snap('album_retry');
    albums().scrollTop = 0;
    await wait(() => button('全部', scope('新碟筛选'))?.getAttribute('aria-disabled') !== 'true',
      'album filter enabled');
    click(button('全部', scope('新碟筛选')));
    await wait(() => count() >= 70, 'unfiltered month drains both batches');
    assert(all('[data-latest-releases] .catalog-album-card').length < 28,
      'album cards are virtualized');
    await snap('album_all');

    back();
    await wait(() => tab('songs')?.getAttribute('aria-selected') === 'true', 'history returns to songs');
    assert(button('小语种', scope('地区'))?.getAttribute('aria-pressed') === 'true',
      'history preserved the last song area');
    await snap('tab_back');
    await send('complete', { ok: true, states: 15 });
  } catch (error) {
    await send('failed', { ok: false, error: String(error), stack: String(error.stack) });
  }
})();
