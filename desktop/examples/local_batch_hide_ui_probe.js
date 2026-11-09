// Native local batch removal from two pages, with a real paused decoder.
(async () => {
  const one = (selector, root = document) => root.querySelector(selector);
  const all = (selector, root = document) => [...root.querySelectorAll(selector)];
  const button = (label, root = document) => all('button', root).find(
    (item) => item.getAttribute('aria-label') === label || item.textContent.trim() === label,
  );
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'local-proof', payload: { stage, ...data },
  });
  const assert = (value, label) => { if (!value) throw Error(label); };
  async function wait(test, label, timeout = 18000) {
    const stop = performance.now() + timeout;
    while (performance.now() < stop) {
      const value = await test();
      if (value) return value;
      await new Promise((done) => setTimeout(done, 100));
    }
    throw Error(label);
  }
  function click(item) {
    assert(item?.isConnected && !item.disabled, 'control is missing or disabled');
    item.focus({ preventScroll: true });
    item.click();
  }
  const query = (keyword = '') => ({
    keyword, sort: 'title', descending: true, group: 'list', groupKey: null,
    includeMissing: true, offset: 0, limit: 200, revision: null,
  });
  const nativePage = (keyword = '') => invoke('local_page', { query: query(keyword) });
  const filter = 'qx-local-selection';
  const toolbar = () => button('更多本地音乐操作', one('.local-page') || document);
  const menu = () => one('.action-menu[role="menu"]');
  const batch = () => one('[data-local-batch]');
  const rows = () => batch() ? all('[data-local-batch-id]', batch()) : [];
  const firstCheck = () => {
    const row = rows()[0];
    return row?.matches('input') ? row : row?.querySelector('input[type="checkbox"]');
  };
  const selected = (count) => new RegExp(`(^|[^0-9])${count}([^0-9]|$)`).test(
    one('[data-local-batch-selection]')?.textContent ?? '',
  );
  const remove = () => one('[data-local-batch-remove]');
  const localSearch = () => one('[aria-label="搜索本地音乐"]');
  const localReady = () => one('.local-page')?.dataset.localLoading === 'false';
  function visible(target) {
    if (!target?.isConnected) return false;
    const bounds = target.getBoundingClientRect();
    return bounds.width > 0 && bounds.height > 0 && target.contains(
      document.elementFromPoint(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2));
  }
  async function capture(stage, target, data = {}) {
    if (!window.__localProof.capture) return;
    await wait(async () => {
      const before = target();
      if (one('.menu-enter-active,.menu-leave-active') || !visible(before)) return false;
      await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));
      return target() === before && visible(before);
    }, `${stage}: expected current painted control`);
    await emit(stage, data);
    await wait(() => window.__folderCaptureAck === stage, 'capture acknowledgement');
  }
  try {
    await wait(() => one('.welcome'), 'guest startup');
    const { epoch } = await wait(async () => {
      const state = await invoke('account_snapshot');
      return ['signed_out', 'expired'].includes(state.status) && state;
    }, 'settled guest');
    click(button('本地音乐'));
    await wait(() => one('.local-page'), 'local page');
    await invoke('local_command', {
      command: { kind: 'watch', path: window.__localProof.root + '/media' },
    });
    const initial = await wait(async () => {
      const page = await nativePage(filter);
      return page.total === 105 && page;
    }, '105 generated files indexed', 40000);
    click(await wait(() => one('[aria-label="播放本地歌曲 雨声"]:not(:disabled)'), 'unrelated local playback'));
    await wait(async () => {
      const state = await invoke('player_snapshot', { epoch });
      return state.state === 'playing' && state.position > .25 && state.track?.name === '雨声';
    }, 'native clock moves before batch action');
    const paused = await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    assert(paused.state === 'paused' && paused.playId, 'native player paused');
    const search = localSearch();
    search.value = filter;
    search.dispatchEvent(new Event('input', { bubbles: true }));
    search.form.requestSubmit();
    await wait(() => localReady() && all('.local-tracks tbody tr').length === 105, 'ordinary filter applied');
    click(await wait(() => toolbar() && !toolbar().disabled && toolbar(), 'ordinary toolbar menu'));
    const batchAction = () => menu() && button('批量操作', menu());
    await wait(batchAction, 'official batch menu entry');
    await capture('ux_01_batch_menu', batchAction);
    click(batchAction());
    await wait(() => batch() && rows().length === 100 && firstCheck(), 'first 100 batch rows');
    const first = rows()[0].dataset.localBatchId;
    click(firstCheck());
    await wait(() => selected(1), 'first checked row');
    click(one('[data-local-batch-next]'));
    await wait(() => rows().length === 5 && firstCheck() && selected(1), 'page 2 retains first selection');
    const second = rows()[0].dataset.localBatchId;
    click(firstCheck());
    await wait(() => selected(2) && remove() && !remove().disabled, 'two checked across pages');
    assert(first && second && first !== second, 'distinct native selection IDs');
    assert(!button('从本地磁盘删除', batch()), 'no physical deletion action');
    await capture('ux_02_batch_cross_page', remove, { ids: [first, second] });

    click(remove());
    const hidden = await wait(async () => {
      const page = await nativePage(filter);
      return page.total === 103 && batch() && selected(0) && page;
    }, 'batch visibility committed');
    assert(hidden.revision !== initial.revision &&
      hidden.tracks.every((row) => ![first, second].includes(row.track.id)), 'exact two chosen rows hidden');
    const stable = await invoke('player_snapshot', { epoch });
    assert(stable.state === 'paused' && stable.playId === paused.playId &&
      stable.entryId === paused.entryId && stable.queueRevision === paused.queueRevision &&
      Math.abs(stable.position - paused.position) < .05, 'native paused clock and queue retained');
    const back = () => button('返回上一页');
    await capture('ux_03_batch_hidden', back, { remaining: hidden.total });
    click(back());
    await wait(() => localReady() && localSearch()?.value === filter &&
      all('.local-tracks tbody tr').length === 103, 'Back preserves original keyword and fresh result');
    await wait(() => document.activeElement === toolbar(), 'Back restores the local menu opener focus');
    const scan = () => button('重新扫描', one('.local-page') || document);
    click(await wait(() => scan() && !scan().disabled && scan(), 'explicit restore command'));
    const restored = await wait(async () => {
      const page = await nativePage(filter);
      return localReady() && scan() && !scan().disabled && page.total === 105 &&
        all('.local-tracks tbody tr').length === 105 && page;
    }, 'explicit rescan restores both rows');
    assert([first, second].every((id) => restored.tracks.some((row) => row.track.id === id)),
      'exact IDs restored');
    await capture('ux_04_batch_restored', localSearch);
    await emit('complete', {
      ok: true, counts: [initial.total, hidden.total, restored.total], selected: [first, second],
      pausedDecoderUnchanged: true, retainedFilterOnBack: true, restoredMenuFocus: true,
      realMusicRequests: 0,
    });
  } catch (error) {
    await emit('failed', { ok: false, error: error.message, stack: error.stack });
  }
})();
