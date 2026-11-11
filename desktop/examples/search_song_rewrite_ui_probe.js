// A real signed Songs view, with native fixture reads and one explicit UI retry.
window.__searchSongRewriteRun = async ({
  one, all, wait, button, click, fresh, choose, capture, assert, ready, emit, hold, isHeld, releaseHeld,
}) => {
  const original = '合成改写原词', first = '合成改写乙', next = '合成改写丙';
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  await wait(() => one('.track-row'), 'daily ready before Songs rewrite');
  await emit('resize', { width: 760, height: 540 });
  await wait(() => innerWidth === 760 && innerHeight === 540, 'compact Songs rewrite window');
  const account = await invoke('account_snapshot');
  assert(account.status === 'authenticated', 'the proof uses the signed Songs view');
  const before = await invoke('player_snapshot', { epoch: account.epoch });
  await fresh(original);

  const rows = () => all('.search-song-panel .track-row[data-track-id]');
  const title = row => row.querySelector('.track-row-title-text')?.textContent.trim();
  const ids = () => rows().map(row => row.dataset.trackId);
  const loaded = () => Number(one('.search-song-panel .track-table')?.getAttribute('aria-rowcount')) - 1;
  const oldOnly = Array.from({ length: 9 }, (_, i) => String(31702 + i));
  const expectedFirst = ['31701', ...oldOnly];
  const expectedNext = ['31701', ...Array.from({ length: 9 }, (_, i) => String(31722 + i))];
  const shown = new Set();
  const observe = () => {
    for (const row of rows()) shown.add(row.dataset.trackId + ':' + title(row));
  };
  const observer = new MutationObserver(observe);
  observer.observe(one('.search-results-page'), {
    childList: true, subtree: true, characterData: true,
  });
  const matches = expected => loaded() === expected.length &&
    JSON.stringify(ids()) === JSON.stringify(expected);
  const heading = effective => {
    const hint = one('.search-correction');
    const stillSearch = button(`仍然搜索“${original}”`, hint ?? document);
    assert(one('.search-results-page h1')?.textContent === original &&
      hint?.textContent.includes(`正在显示“${effective}”的结果`) && stillSearch,
    'route heading and still-search remain original while the hint uses ' + effective);
    return { original, effective, stillSearch: stillSearch.textContent.trim() };
  };
  try {
    await choose('songs');
    await wait(() => ready() && matches(expectedFirst), 'replacement Songs head from first rewrite');
    assert(title(rows()[0]) === first + ' 31701' && !one('.search-result-pagination'),
      'signed continuous Songs uses only the first corrected page');
    await capture('search_song_rewrite_first_head', { ...heading(first), ids: ids() });

    const oldControl = rows()[0].querySelector('.track-album-link');
    assert(oldControl, 'old row has a real navigable album control');
    oldControl.focus({ preventScroll: true });
    assert(document.activeElement === oldControl, 'focus the old control before replacing the page');
    hold('songs', next);
    const more = button('加载更多');
    assert(more && more.getAttribute('aria-disabled') === 'false', 'old page can request continuation');
    more.click(); // Keep focus on the album; exercise the rendered continuation handler.
    await wait(() => isHeld() && one('.search-song-panel')?.getAttribute('aria-busy') === 'true' &&
      rows().length === 0 && !oldControl.isConnected, 'old rows retire while new head is pending');
    assert(document.activeElement !== oldControl && !one('.search-song-panel .track-table'),
      'the focused old control is retired before the corrected result arrives');
    await capture('search_song_rewrite_retired', {
      ...heading(next), ids: ids(), oldControlDetached: !oldControl.isConnected,
      oldControlBlurred: document.activeElement !== oldControl,
    });
    releaseHeld();

    await wait(() => ready() && one('.search-song-panel [role="alert"]') && button('重新加载'),
      'corrected head refusal surfaces main retry');
    assert(rows().length === 0 && !button('重试加载更多') && !button('加载更多'),
      'refusal cannot reactivate prior rows or their continuation');
    await capture('search_song_rewrite_refused', { ...heading(next), ids: ids() });

    click(button('重新加载'));
    await wait(() => ready() && matches(expectedNext), 'explicit retry publishes only the new head');
    const newControl = rows()[0].querySelector('.track-album-link');
    assert(title(rows()[0]) === next + ' 31701' && newControl?.isConnected &&
      newControl !== oldControl && !oldControl.isConnected && document.activeElement !== newControl,
    'reused Song ID does not reattach or focus the old control');
    await capture('search_song_rewrite_retry', {
      ...heading(next), ids: ids(), title: title(rows()[0]), newControl: newControl !== oldControl,
    });

    click(button('加载更多'));
    const expectedTail = [...expectedNext, '31731'];
    await wait(() => ready() && loaded() === expectedTail.length && !button('加载更多'),
      'next corrected cursor appends its own terminal page');
    const scroll = one('.search-song-panel .track-scroll');
    scroll.scrollTop = scroll.scrollHeight;
    await wait(() => ids().at(-1) === '31731', 'corrected tail enters the virtual table window');
    const visibleTail = ids();
    assert(JSON.stringify(visibleTail) === JSON.stringify(expectedTail.slice(-visibleTail.length)),
      'the rendered virtual window is an ordered suffix of the corrected rows');
    observe();
    const allowed = new Set([
      ...expectedFirst.map(id => id + ':' + first + ' ' + id),
      ...[...expectedNext, '31731'].map(id => id + ':' + next + ' ' + id),
    ]);
    assert(shown.size === allowed.size && [...shown].every(row => allowed.has(row)),
      'redirecting payload or wrong-term row rendered: ' + JSON.stringify([...shown]));
    const after = await invoke('player_snapshot', { epoch: account.epoch });
    const queueUnchanged = ['epoch', 'playId', 'entryId', 'queueRevision', 'queueLen']
      .every(key => before[key] === after[key]);
    assert(queueUnchanged, 'the correction sequence does not dispatch playback');
    await capture('search_song_rewrite_tail', {
      ...heading(next), ids: visibleTail, loaded: loaded(), queueUnchanged,
    });
    return { renderedRows: [...shown], queueUnchanged };
  } finally {
    observer.disconnect();
  }
};
