// Actual local group menu/IPC and null-output native player, synthetic files only.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
  const button = (label, root = document) => [...root.querySelectorAll('button')].find(
    (item) => item.getAttribute('aria-label') === label || item.textContent.trim() === label,
  );
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, value = {}) => invoke('plugin:event|emit', {
    event: 'local-proof', payload: { stage, ...value },
  });
  const assert = (value, label) => { if (!value) throw Error(label); };
  async function wait(test, label) {
    for (let i = 0; i < 180; i++) {
      const value = await test();
      if (value) return value;
      await new Promise((done) => setTimeout(done, 100));
    }
    throw Error(label);
  }
  function click(item) {
    assert(item && !item.disabled, 'control is missing or disabled');
    item.focus({ preventScroll: true });
    item.click();
  }
  function key(target, value, shiftKey = false) {
    target.dispatchEvent(new KeyboardEvent('keydown', {
      key: value, shiftKey, bubbles: true, cancelable: true,
    }));
  }
  const menu = () => one('.action-menu[role="menu"]');
  const hideAction = () => menu()?.querySelector('button[data-menu-key="hide_group"]:not(:disabled)');
  const rows = () => all('.local-tracks tbody tr');
  const ready = () => one('.local-page')?.dataset.localLoading === 'false';
  const bonus = () => all('[data-local-folder-key]').find((node) => /[/\\]Bonus$/.test(node.title));
  const nativePage = () => invoke('local_page', { query: {
    keyword: '', sort: 'title', descending: true, group: 'list', groupKey: null,
    includeMissing: true, offset: 0, limit: 200, revision: null,
  }});
  async function context(target, mouse = false) {
    assert(target?.isConnected, 'current group anchor is detached');
    target.focus({ preventScroll: true });
    if (mouse) target.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    else key(target, 'F10', true);
    return wait(hideAction, 'index-only group menu action');
  }
  function visible(target) {
    if (!target?.isConnected) return false;
    const bounds = target.getBoundingClientRect();
    return bounds.width > 0 && bounds.height > 0 && target.contains(
      document.elementFromPoint(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2));
  }
  async function capture(stage, current) {
    if (!window.__localProof.capture) return;
    const transitioning = () => one('.menu-enter-active,.menu-leave-active');
    await wait(async () => {
      const target = current();
      if (transitioning() || !visible(target)) return false;
      // A completed list read can still replace the group button on the next
      // render. Require the current, unobstructed control across both frames.
      await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));
      return current() === target && !transitioning() && visible(target);
    }, `${stage}: proof control did not become visible`);
    await emit(stage);
    await wait(() => window.__folderCaptureAck === stage, 'screenshot acknowledgement');
  }
  function media(page, filename) {
    return page.tracks.find((row) => row.fileName === filename)?.track.id;
  }
  try {
    await wait(() => one('.welcome'), 'guest startup');
    const { epoch } = await wait(async () => {
      const state = await invoke('account_snapshot');
      return ['signed_out', 'expired'].includes(state.status) && state;
    }, 'settled guest');
    click(button('本地音乐'));
    await wait(() => one('.local-page'), 'actual local page');
    await invoke('local_command', { command: { kind: 'watch', path: window.__localProof.root + '/media' } });
    await wait(() => ready() && rows().length === 7, 'native library scanned');
    const initial = await nativePage();
    const ids = ['Bonus Track.wav', 'Nested audible.wav', 'Neighbor audible.wav'].map(
      (file) => media(initial, file),
    );
    assert(ids.every(Boolean) && new Set(ids).size === 3, 'three distinct native file IDs');
    click(button('文件夹', one('.local-tabs')));
    click(await wait(() => ready() && one('.local-folder-toggle'), 'actual nested folder tree'));
    click(await wait(bonus, 'indexed Bonus node'));
    await wait(() => ready() && rows().length === 1 && rows()[0].dataset.localId === ids[0],
      'selected parent shows its direct track');
    await context(bonus(), true);
    click(button('播放', menu()));
    await wait(async () => {
      const value = await invoke('player_snapshot', { epoch });
      return value.state === 'playing' && value.track?.id === ids[0];
    }, 'actual native decoder playing');
    const before = await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    assert(before.state === 'paused' && before.playId, 'real decoder paused');
    const input = one('[aria-label="搜索本地音乐"]');
    input.value = 'Bonus Track';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    input.form.requestSubmit();
    const filteredBonus = await wait(() => {
      const target = bonus();
      // Search replaces the tree with a flat list. A collapsed pre-search
      // tree also has no visible Deep row, so absence alone is not readiness.
      return ready() && !one('.local-folder-rows') && target?.isConnected &&
        target.getAttribute('aria-pressed') === 'true' &&
        !all('[data-local-folder-key]').some((node) => /[/\\]Deep$/.test(node.title)) &&
        rows().length === 1 && rows()[0].dataset.localId === ids[0] && target;
    }, 'search commits the filtered group');
    const first = await context(filteredBonus, true);
    assert(first.textContent.trim() === '从列表中删除' &&
      !button('从本地磁盘删除', menu()), 'physical deletion is not offered');
    await capture('ux_01_group_hide_menu', hideAction);
    key(document.activeElement, 'Escape');
    await wait(() => !menu() && document.activeElement === bonus(), 'escape restores anchor');
    assert((await nativePage()).revision === initial.revision, 'dismissal is read-only');
    click(await context(bonus()));
    const allGroups = () => one('[data-local-all-groups]');
    await wait(() => ready() && !menu() && !bonus() && allGroups()?.getAttribute('aria-pressed') === 'true' &&
      document.activeElement === allGroups() && one('.local-page .notice')?.textContent.includes('源文件仍保留'),
    'fallback selection/focus and committed list-only notice');
    const hidden = await nativePage();
    assert(hidden.total === 5 && hidden.revision !== initial.revision &&
      !hidden.tracks.some((row) => [ids[0], ids[1]].includes(row.track.id)) &&
      hidden.tracks.some((row) => row.track.id === ids[2]), 'full subtree hidden, similar-prefix neighbor remains');
    await capture('ux_02_group_hidden', allGroups);
    click(button('清空本地搜索'));
    await wait(() => ready() && rows().length === 5, 'search clears without restoring hidden group');
    click(button('重新扫描'));
    await wait(() => ready() && rows().length === 7 && bonus(), 'explicit rescan restores group and child');
    const restored = await nativePage();
    const after = await invoke('player_snapshot', { epoch });
    assert(ids.every((id) => restored.tracks.some((row) => row.track.id === id)), 'exact IDs restored');
    assert(after.state === 'paused' && after.playId === before.playId && after.entryId === before.entryId &&
      after.queueRevision === before.queueRevision && Math.abs(after.position - before.position) < 0.05,
    'native decoder, queue and paused clock preserved');
    await capture('ux_03_group_restored', bonus);
    await emit('complete', { ok: true, counts: [initial.total, hidden.total, restored.total],
      stablePausedDecoder: true, similarPrefixNeighborRetained: true, realMusicRequests: 0 });
  } catch (error) {
    await emit('failed', { ok: false, error: error instanceof Error ? error.message : error, stack: error?.stack });
  }
})();
