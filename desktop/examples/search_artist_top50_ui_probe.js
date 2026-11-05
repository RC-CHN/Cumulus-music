// Opt-in scenario for the existing search WebView host and synthetic API.
window.__searchArtistTop50Run = async function ({
  one, wait, button, click, ready, choose, capture, fresh, sleep, assert, fixture,
}) {
  const query = '合成歌手热歌';
  const selectedCategory = () => one('[data-category][aria-selected="true"]')?.dataset.category;
  const panel = () => one('#search-result-panel');
  const best = () => one('.search-best-section [data-result-key="artist:101"]');
  const regular = () => one('#search-result-panel [data-result-key="artist:102"]');
  const broken = () => one('#search-result-panel [data-result-key="artist:103"]');
  const play = (card) => card?.querySelector('.search-card-play');
  const footer = (label) => button(label, one('.playerbar'));
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  let epoch;
  const player = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  const trackOrder = (snapshot) => snapshot.tracks.map((row) => row.track.id).join(',');

  function sameRoute(category) {
    return one('.search-results-page h1')?.textContent === query && selectedCategory() === category &&
      ready() && !one('.artist-header,.lyrics-panel');
  }
  async function native(predicate, label) {
    for (let index = 0; index < 160; index++) {
      const value = await player();
      if (predicate(value)) return value;
      await sleep(70);
    }
    throw Error(label);
  }
  async function audible(id, artist, prior) {
    return native((p) => p.state === 'playing' && p.track?.id === id &&
      p.resourceKind === 'catalog' && p.origin?.kind === 'artist' && p.origin?.id === artist &&
      p.playId !== prior && p.position > .14, 'authorized native artist playback ' + id);
  }
  async function cover(card) {
    await wait(() => play(card()) && !play(card()).disabled, 'independent artist play overlay');
    const target = play(card());
    const title = card()?.querySelector('.search-card-open');
    assert(title && title !== target && !title.disabled, 'artist title remains a distinct details action');
    target.scrollIntoView({ block: 'nearest' });
    target.focus({ preventScroll: true });
    await wait(() => target.isConnected && target.getClientRects().length &&
      getComputedStyle(target).visibility === 'visible', 'artist overlay visible in the real WebView');
    click(target);
  }
  async function state(name, category, view, expected, extra = {}) {
    await wait(() => sameRoute(category), 'no navigation from artist cover: ' + category);
    const snapshot = await queue();
    assert(trackOrder(snapshot) === expected &&
      snapshot.current === view.entryId && snapshot.revision === view.queueRevision,
      'native selected entry and ordered songs match the source: ' + name);
    await capture(name, { query, category, playId: view.playId, trackId: view.track?.id,
      position: view.position, queueRevision: snapshot.revision,
      entryIds: snapshot.tracks.map((row) => row.entryId), scroll: panel()?.scrollTop, ...extra });
  }

  await wait(() => one('.track-row') && footer('列表循环') && !footer('列表循环').disabled,
    'main window ready');
  const account = await invoke('account_snapshot');
  assert(account.status === 'authenticated', 'sealed account initialized');
  epoch = account.epoch;
  await fixture('good');
  await fresh(query);
  await wait(() => sameRoute('all') && best(), 'best-match artist card available');
  await cover(best);
  const first = await audible('92', '101');
  await state('artist_best_play', 'all', first, '92,91,93');

  click(footer('下一首'));
  const next = await audible('91', '101', first.playId);
  await state('artist_best_next', 'all', next, '92,91,93');

  await choose('artists');
  await wait(() => sameRoute('artists') && regular(), 'regular artist card in artist tab');
  await cover(regular);
  const second = await audible('96', '102', next.playId);
  await state('artist_regular_play', 'artists', second, '96,94,95');
  await wait(() => play(regular())?.getAttribute('aria-label')?.startsWith('暂停 '),
    'selected artist card offers pause');
  await cover(regular);
  const paused = await native((p) => p.state === 'paused' && p.playId === second.playId,
    'same card pauses the current decoder');
  await state('artist_regular_pause', 'artists', paused, '96,94,95');
  await wait(() => play(regular())?.getAttribute('aria-label')?.startsWith('播放 '),
    'paused artist card offers resume');
  await cover(regular);
  const resumed = await native((p) => p.state === 'playing' && p.playId === second.playId &&
    p.position >= paused.position, 'same card resumes the existing decoder');
  await state('artist_regular_resume', 'artists', resumed, '96,94,95');

  await cover(broken);
  await wait(() => one('.playback-message') && sameRoute('artists') && !play(broken()).disabled,
    'explicit artist load error and ready retry');
  const retained = await player();
  assert(retained.state === 'playing' && retained.playId === resumed.playId,
    'failed alternate artist does not interrupt native playback');
  await state('artist_failure_keeps_queue', 'artists', retained, '96,94,95',
    { error: one('.playback-message')?.textContent.trim() });

  click(button('返回上一页'));
  await wait(() => sameRoute('all') && best(), 'one Back restores comprehensive search directly');
  await state('artist_back_to_search', 'all', await player(), '96,94,95');
};
