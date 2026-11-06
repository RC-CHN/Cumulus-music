// Actual local menu and typed native IPC; system side is a private fake shell.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, value = {}) =>
    invoke('plugin:event|emit', { event: 'local-proof', payload: { stage, ...value } });
  const assert = (value, label) => { if (!value) throw Error(label); };
  const button = (label, root = document) => [...root.querySelectorAll('button')].find(
    (item) => item.getAttribute('aria-label') === label || item.textContent.trim() === label,
  );
  async function wait(test, label) {
    for (let i = 0; i < 160; i++) {
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
  const menu = () => one('.action-menu[role="menu"]');
  const reveal = () => menu() && button('打开文件所在目录', menu());
  const tab = (label) => button(label, one('.local-tabs'));
  const folder = () => all('[data-local-folder-key]').find((item) => item.title.endsWith('Bonus'));
  const rows = () => all('.local-tracks tbody tr').map((row) => row.dataset.localId).join(',');
  const key = (target, value, shiftKey = false) => target.dispatchEvent(new KeyboardEvent('keydown', {
    key: value, shiftKey, bubbles: true, cancelable: true,
  }));
  async function context(target, mouse = false) {
    target.focus({ preventScroll: true });
    if (mouse)
      target.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    else key(target, 'F10', true);
    await wait(() => menu()?.querySelector('[role="menuitem"]'), 'real group menu');
  }
  async function close(target) {
    await wait(() => !menu() && document.activeElement === target, 'menu returns focus');
  }
  async function rejected(query, revision) {
    try {
      await invoke('reveal_local_folder', { query, revision });
    } catch (error) {
      assert(['invalid_input', 'snapshot_changed'].includes(error?.code), JSON.stringify(error));
      return error.code;
    }
    throw Error('unauthorized group request reached shell');
  }
  try {
    await wait(() => one('.welcome'), 'guest startup');
    click(button('本地音乐'));
    await wait(() => one('.local-page'), 'local page');
    await invoke('local_command', { command: { kind: 'watch', path: window.__localProof.root + '/media' } });
    await wait(() => all('.local-tracks tbody tr').length === 5, 'native local scan');
    for (const kind of ['歌手', '专辑']) {
      click(tab(kind));
      const node = await wait(() => tab(kind)?.getAttribute('aria-pressed') === 'true' &&
        one('.local-page')?.dataset.localLoading === 'false' && one('[data-local-group-key]'),
        'ready nonfolder group');
      await context(node);
      assert(!reveal(), kind + ' context does not expose native folder action');
      key(document.activeElement, 'Escape');
      await close(node);
    }
    click(tab('文件夹'));
    const expand = await wait(() => tab('文件夹')?.getAttribute('aria-pressed') === 'true' &&
      one('.local-page')?.dataset.localLoading === 'false' && one('.local-folder-toggle'),
      'nested folder disclosure');
    click(expand);
    const target = await wait(folder, 'actual nested folder');
    const query = {
      keyword: '', sort: 'title', descending: true, group: 'folder', groupKey: null,
      includeMissing: true, offset: 0, limit: 200, revision: null,
    };
    const page = await invoke('local_page', { query });
    const scoped = { ...query, groupKey: target.dataset.localFolderKey };
    assert(page.groups.some((group) => group.key === scoped.groupKey), 'native scope includes target');
    const negative = [
      await rejected({ ...scoped, group: 'artist' }, page.revision),
      await rejected({ ...scoped, groupKey: window.__localProof.root }, page.revision),
      await rejected(scoped, '0'.repeat(64)),
    ];
    const before = rows();
    assert(target.getAttribute('aria-pressed') === 'false', 'folder not currently selected');
    for (const [index, outcome] of ['reveal', 'fallback', 'deny', 'timeout'].entries()) {
      await context(target, index === 0);
      const entry = await wait(() => reveal() && !reveal().disabled && reveal(), 'folder reveal enabled');
      const sentAt = performance.now();
      click(entry);
      await close(target);
      if (index >= 2)
        await wait(() => one('.local-page [role="alert"]')?.textContent.includes('无法定位文件夹'), outcome);
      if (index === 3)
        assert(performance.now() - sentAt >= 1600, 'timeout must follow this request, not a previous alert');
      assert(target.getAttribute('aria-pressed') === 'false' && before === rows(), 'reveal modified local selection');
    }
    await emit('complete', { ok: true, outcomes: ['reveal', 'fallback', 'deny', 'timeout'],
      negative, realMusicRequests: 0, focusRestored: true, selectedFolderChanged: false });
  } catch (error) {
    await emit('failed', { ok: false, error: error instanceof Error ? error.message : error, stack: error?.stack });
  }
})();
