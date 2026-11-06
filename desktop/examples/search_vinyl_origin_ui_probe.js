// Search row and vinyl attribution on the actual app over sealed libmpv media.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (condition, message) => { if (!condition) throw Error(message); };
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const query = '合成 V3';
  const playlistName = '合成歌单 7';
  const row = (id) => one(`[data-result-key="song:${id}"] .search-track-cover`);
  const search = () => one('.search-results-page h1')?.textContent === query &&
    one('#search-result-panel')?.getAttribute('aria-busy') === 'false' && !one('.page-leave-active');
  const panel = () => one('.lyrics-panel');
  const source = () => panel()?.querySelector('.vinyl-source');
  const lyrics = () => one('.playerbar [aria-label="打开播放详情"]');
  const ready = (node) => node?.isConnected && !node.disabled && !node.closest('[inert]') ? node : null;
  const click = (node) => { assert(ready(node), 'connected and enabled control'); node.click(); };
  let epoch, count = 0, step = 'startup', last;
  const native = async () => (last = await invoke('player_snapshot', { epoch }));
  const same = (a, b) => a.playId === b.playId && a.entryId === b.entryId && a.queueRevision === b.queueRevision;
  async function wait(read, message) {
    step = message;
    for (let i = 0; i < 175; i++) {
      const value = await read();
      if (value) return value;
      await sleep(65);
    }
    throw Error(message);
  }
  async function state(wanted, id, known) {
    return wait(async () => {
      const view = await native();
      const bar = one('.playerbar');
      assert(!view.error, `native playback error: ${view.error}`);
      return view.state === wanted && view.track?.id === id && !!view.playId &&
        view.position > .2 && bar?.dataset.playId === view.playId && bar.dataset.state === wanted &&
        (!known || same(view, known)) && view;
    }, `${wanted} native track ${id}`);
  }
  async function openVinyl() {
    click(await wait(() => ready(lyrics()), 'open vinyl control'));
    return wait(() => {
      const view = panel();
      const label = source();
      if (view?.dataset.vinylMode !== 'classic' || !label ||
        view.classList.contains('lyrics-panel-enter-active') || getComputedStyle(view).opacity !== '1') return false;
      const box = label.getBoundingClientRect();
      return box.width > 0 && box.height > 0 &&
        label.contains(document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2)) && label;
    }, 'classic vinyl source visibly settled');
  }
  async function closeVinyl() {
    click(await wait(() => ready(panel()?.querySelector('[aria-label="收起播放详情"]')), 'close vinyl control'));
    await wait(() => !panel() && search(), 'returned to the same search result');
  }
  async function capture(label, observed, extra = {}) {
    const stage = `ux_${String(++count).padStart(2, '0')}_${label}`;
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await emit(stage, { observed, label, sourceText: source()?.textContent.replace(/\s+/g, '') ?? null,
      sourceButton: source()?.querySelector('button')?.getAttribute('aria-label') ?? null,
      ...extra });
    await wait(() => window.__searchFooterRecorded === stage, `native capture ${label}`);
  }
  try {
    const field = await wait(async () => {
      const account = await invoke('account_snapshot');
      if (account.status !== 'authenticated' || !one('.track-row')) return false;
      const input = ready(one('[aria-label="搜索音乐"]'));
      if (!input) return false;
      epoch = account.epoch;
      return input;
    }, 'sealed account and search input');
    await invoke('play_selection', { epoch, selection: {
      kind: 'playlist', id: '7', start_id: '1', revision: null, quality: 'standard',
    } });
    const playlist = await state('playing', '1');
    assert(playlist.origin?.kind === 'playlist' && playlist.origin.id === '7' &&
      playlist.originName === playlistName && !playlist.searchOrigin, 'native name of real playlist origin');
    field.value = query;
    field.dispatchEvent(new Event('input', { bubbles: true }));
    field.form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    const original = await wait(() => search() && ready(row('1'))?.getAttribute('aria-label')?.startsWith('暂停 ') && row('1'),
      'current playlist song is playable from search');
    click(original);
    const retained = await state('paused', '1', playlist);
    assert(retained.origin?.kind === 'playlist' && retained.origin.id === '7' &&
      retained.originName === playlistName && !retained.searchOrigin,
      'same-row pause must retain the original playlist source');
    const linked = await openVinyl();
    const originalLink = linked.querySelector(`button[aria-label="查看播放来源：${playlistName}"]`);
    assert(ready(originalLink) && originalLink.tabIndex === 0, 'playlist source remains a real link control');
    await capture('retained_playlist', retained);
    await closeVinyl();
    click(await wait(() => search() && ready(row('2')), 'new search-origin song'));
    const played = await state('playing', '2');
    assert(played.playId !== retained.playId && played.searchOrigin === true &&
      played.origin === null && played.originName === undefined,
      'newly selected catalog song has search provenance only');
    await wait(() => ready(row('2'))?.getAttribute('aria-label')?.startsWith('暂停 '), 'search pause control');
    click(row('2'));
    const paused = await state('paused', '2', played);
    const label = await openVinyl();
    assert(label.textContent.replace(/\s+/g, '') === '来源：搜索' &&
      !label.querySelector('button,a,[tabindex],[role="link"]'), 'plain non-navigating search label');
    label.lastElementChild.click();
    assert(panel()?.contains(label) && search(), 'search source label does not navigate');
    assert(same(await native(), paused), 'vinyl label interaction keeps the native play');
    await capture('search_plain', paused);
    await closeVinyl();
    const returned = await state('paused', '2', paused);
    assert(returned.searchOrigin && search(), 'same search after closing vinyl');
    await capture('returned_search', returned, { sameSearch: true });
    await emit('complete', { ok: true, stageCount: count, realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', { step, count, error: String(error), last, heading: one('.search-results-page h1')?.textContent,
      source: source()?.outerHTML });
  }
})();
