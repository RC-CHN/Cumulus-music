(async () => {
  const q = (selector, root = document) => root?.querySelector(selector);
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])];
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...data } });
  const text = node => node?.textContent.trim();
  const ready = node => node?.isConnected && node.getClientRects().length &&
    !node.matches(':disabled,[aria-disabled="true"]') && !node.closest('[inert]');
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const assert = (ok, message) => { if (!ok) throw Error(message); };
  const page = () => q('.history-page:not(.page-leave-active)');
  const trigger = () => q('.history-download-selected', page());
  const modal = () => q('dialog[data-search-group-download-picker][open]');
  const quality = () => q('dialog[data-search-group-purchased][open]');
  const destinations = () => all('[data-playlist-id]', modal() ?? null).map(node => node.dataset.playlistId);
  const rows = () => all('.history-window input[type="checkbox"]', page());
  const selected = () => rows().filter(node => node.checked).map(node => node.getAttribute('aria-label').slice(-2));
  const feedback = () => text(q('.track-action-feedback')) ?? null;
  const rowProof = window.__historySongRowDownloadProof;
  const songRow = id => q(`[data-history-row-id="${id}"]`, page());
  const more = id => q(`[data-history-row-more="${id}"]`, page());
  const rowMenu = () => q('[role="menu"][aria-label="播放记录操作"]:not([inert])');
  const rowItem = key => q(`[data-menu-key="${key}"]`, rowMenu());
  const keys = () => all('[data-menu-key]', rowMenu()).map(item => item.dataset.menuKey);
  const browseIds = () => all('[data-history-row-id][aria-selected="true"]', page())
    .map(node => node.dataset.historyRowId).sort();
  const browsing = () => page()?.dataset.historyView === 'browse' &&
    page()?.dataset.historyCategory === '歌曲' && page()?.getAttribute('aria-busy') === 'false';
  const rowError = () => text(q('.history-row-menu-error[role="alert"]', page()));
  const stages = [];
  let epoch, step = 'startup';
  async function wait(observe, label) {
    step = label;
    const deadline = performance.now() + 18_000;
    do {
      const result = await observe();
      if (result) return result;
      await new Promise(resolve => setTimeout(resolve, 65));
    } while (performance.now() < deadline);
    throw Error(label);
  }
  function click(node) {
    assert(ready(node), 'clickable production control');
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    node.click();
  }
  const list = tab => invoke('downloads', { epoch, query: {
    tab, category: 'song', keyword: '', sort: 'none', descending: false, offset: 0, limit: 20,
  } });
  async function facts() {
    const active = await list('active'), complete = await list('completed');
    return { active: active.tasks.map(row => ({ id: row.track.id, status: row.status, error: row.error })),
      completed: complete.tasks.map(row => ({ id: row.track.id, source: row.source, quality: row.actualQuality,
        encrypted: row.encrypted, bytes: row.bytes, path: row.path })).sort((a, b) => a.id.localeCompare(b.id)) };
  }
  async function capture(name, rowFacts = {}) {
    stages.push(name);
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await emit(`ux_${String(stages.length).padStart(2, '0')}_${name}`, {
      selected: selected(), title: text(q('h2', modal() ?? null)) ?? null,
      feedback: feedback(), destinations: destinations(), quality: !!quality(),
      returnFocus: document.activeElement === trigger(), ...await facts(), ...rowFacts,
    });
  }
  async function openPicker() {
    click(await wait(() => ready(trigger()) && trigger(), 'selected songs enable Download'));
    await wait(() => text(q('h2', modal() ?? null)) === '下载到歌单' &&
      ready(q('[data-playlist-id="81"]', modal())) && same(destinations(), ['81']),
    'original unliked first song opens owned playlist picker');
    assert(!quality(), 'ordinary downloads do not show a quality chooser');
  }
  async function published(ids) {
    await wait(async () => {
      const { active, completed } = await facts();
      return active.length === 0 && same(completed.map(row => row.id), ids);
    }, `Rust published ${ids.join(',')} with no active task`);
  }
  async function openSongRow(ids, clicked, gesture, expectedKeys) {
    for (const [index, id] of ids.entries()) {
      const target = songRow(id);
      assert(ready(target), 'mounted song row ' + id);
      target.dispatchEvent(new MouseEvent('click', {
        bubbles: true, cancelable: true, button: 0, ctrlKey: index > 0,
      }));
    }
    const anchor = more(clicked);
    assert(ready(anchor), 'enabled song More for ' + clicked);
    anchor.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'instant' });
    anchor.focus({ preventScroll: true });
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    let previous, stableAt;
    await wait(() => {
      if (!browsing() || more(clicked) !== anchor || document.activeElement !== anchor) return;
      const bounds = anchor.getBoundingClientRect();
      const current = [bounds.left, bounds.top, bounds.width, bounds.height];
      if (!same(previous, current)) {
        previous = current;
        stableAt = performance.now();
        return;
      }
      return performance.now() - stableAt >= 105;
    }, 'settled song row anchor ' + clicked);
    if (gesture === 'ctrl-context') {
      const target = songRow(clicked);
      const bounds = target.getBoundingClientRect();
      assert(!target.dispatchEvent(new MouseEvent('contextmenu', {
        bubbles: true, cancelable: true, button: 2, ctrlKey: true,
        clientX: bounds.left + 18, clientY: bounds.top + 12,
      })), 'Ctrl+right-click handled by song history');
    } else if (gesture === 'keyboard') {
      assert(!anchor.dispatchEvent(new KeyboardEvent('keydown', {
        bubbles: true, cancelable: true, key: 'F10', shiftKey: true,
      })), 'Shift+F10 handled by song history');
    } else anchor.click();
    await wait(() => rowMenu() && anchor.getAttribute('aria-expanded') === 'true',
      'visible song history row menu');
    assert(same(keys(), expectedKeys), 'official song history row menu keys ' + keys().join(','));
    assert(same(browseIds(), [...ids].sort()), 'retains exact selected song rows');
    if (rowItem('download')) assert(text(rowItem('download')) === '下载', 'official Download label');
    if (rowItem('reveal')) assert(ready(rowItem('reveal')) &&
      text(rowItem('reveal')) === '打开文件所在目录', 'official exact-file reveal label');
    return { anchor, receipt: { clicked, selectedIds: ids, gesture, menuKeys: keys() } };
  }
  async function rowCapture(name, target, moreFacts = {}) {
    assert(browsing() && same(browseIds(), [...target.receipt.selectedIds].sort()),
      'download preserves History route and exact browse selection');
    let menuPaint = {};
    if (['row_blocked_group', 'row_completed_menu', 'row_ambiguous_menu'].includes(name) ||
        name.endsWith('_reveal_offer') || name === 'row_stale_offer') {
      const panel = await wait(() => {
        const current = rowMenu();
        if (current?.getClientRects().length && !current.classList.contains('menu-enter-active')
            && Number(getComputedStyle(current).opacity) >= .99) return current;
      }, 'row menu transition settled');
      const box = panel.getBoundingClientRect();
      menuPaint = { screenX, screenY, ratio: devicePixelRatio,
        paintSamples: [{ x: box.left + box.width / 2, y: box.top + 2,
          color: getComputedStyle(panel).backgroundColor }] };
    }
    await capture(name, { ...target.receipt, stayedInHistory: true,
      returnFocus: document.activeElement === target.anchor, ...menuPaint, ...moreFacts });
  }
  async function dismissRow(target) {
    rowMenu().dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await wait(() => !rowMenu() && document.activeElement === target.anchor, 'menu restores clicked More');
  }
  async function rowPicker() {
    click(rowItem('download'));
    await wait(() => text(q('h2', modal())) === '下载到歌单' &&
      ready(q('[data-playlist-id="81"]', modal())) && same(destinations(), ['81']),
    'clicked unliked anchor opens owned playlist picker');
    assert(!quality(), 'ordinary row downloads have no quality chooser');
  }
  const catalogReveal = ['play', 'next', 'comments', 'collect', 'copy', 'reveal', 'remove'];
  const groupedReveal = ['play', 'next', 'collect', 'reveal', 'remove'];
  const history = async () => (await invoke('history_page', { epoch, kind: 'song', refresh: false })).value;
  async function seedFileRows() {
    const before = await history();
    window.__historySongFileFixture = null;
    await emit('history-song-file-fixture', { action: 'seed' });
    const fixture = await wait(() => window.__historySongFileFixture?.action === 'seed' &&
      window.__historySongFileFixture, 'seed closed native song-file fixture');
    assert(/^local:[0-9a-f]{64}$/.test(fixture.localId), 'exact native synthetic local history ID');
    click(await wait(() => {
      const refresh = q('button[aria-label="刷新播放记录"]', page());
      return ready(refresh) && refresh;
    }, 'actual History refresh control'));
    const after = await wait(async () => {
      if (!browsing() || !ready(more(fixture.localId))) return;
      const result = await history();
      return result.revision !== before.revision && result.remoteIncluded &&
        result.entries.some(entry => entry.id === fixture.localId) && result;
    }, 'refreshed History includes the exact seeded local row');
    assert(before.entries.length === 6 && after.entries.length === 7,
      'only the fixture local history record was added');
    assert(text(q('.history-name', songRow(fixture.localId))) === '历史原始音频',
      'the local row visibly identifies the sealed audio fixture');
    return fixture.localId;
  }
  async function reveal(name, target) {
    assert(!rowItem('download') && ready(rowItem('reveal')), 'file menu replaces Download with Reveal');
    await rowCapture(`row_${name}_reveal_offer`, target);
    if (rowProof.fileManager) click(rowItem('reveal'));
    else await dismissRow(target);
    await wait(() => !rowMenu() && !modal() && !quality() && !rowError() &&
      browsing() && document.activeElement === target.anchor, 'file reveal restores clicked More');
    // Linux waits for the exact native FileManager1 dispatch before acknowledging
    // each receipt. Windows checks the offer/focus and never invokes Explorer.
    await rowCapture(`row_${name}_revealed`, target, { fileManagerDispatched: rowProof.fileManager });
    assert(!rowError(), 'file reveal produced no row error');
  }
  async function revealRows() {
    const localId = await seedFileRows();
    const ambiguous = await openSongRow(['1'], '1', 'button',
      ['play', 'next', 'comments', 'collect', 'remove']);
    assert(!rowItem('reveal') && !rowItem('download'),
      'ambiguous local matches expose neither Reveal nor a duplicate Download');
    await rowCapture('row_ambiguous_menu', ambiguous, { revealVisible: false, downloadVisible: false });
    await dismissRow(ambiguous);

    // Song 2 has both a verified completed publication and a native local
    // match. The group retains gesture-first song 3, but the exact clicked
    // song 2 publication must win over both alternatives.
    const downloaded = await openSongRow(['3', '2'], '2', 'keyboard', groupedReveal);
    await reveal('download', downloaded);
    const local = await openSongRow([localId], localId, 'button', groupedReveal);
    await reveal('local', local);
    const mapped = await openSongRow(['6'], '6', 'button', catalogReveal);
    await reveal('mapped', mapped);

    if (rowProof.fileManager) {
      const stale = await openSongRow(['2'], '2', 'button', catalogReveal);
      // The sealed runner changes one byte of the exact download *after*
      // capturing this already-open menu, before acknowledging the receipt.
      await rowCapture('row_stale_offer', stale);
      click(rowItem('reveal'));
      const failure = await wait(() => !rowMenu() && browsing() &&
        document.activeElement === stale.anchor && rowError(), 'changed publication is refused');
      assert(failure === '播放记录或本机文件已更新，请重新打开菜单。',
        'stale-file warning identifies the changed history/file');
      await rowCapture('row_stale_rejected', stale, { staleError: failure });
    }
  }
  async function runRows() {
    await wait(() => browsing() && ['1', '2', '3', '4', '5', '6'].every(id => ready(more(id))),
      'six synthetic SONG browse rows ready');
    const blocked = await openSongRow(['1', '5'], '6', 'ctrl-context',
      ['play', 'next', 'collect', 'download']);
    const item = rowItem('download');
    assert(item.disabled && item.title === '暂无版权', 'all copyright-blocked songs disable Download');
    item.click();
    assert(!modal() && !quality(), 'disabled action cannot mount any dialog');
    await rowCapture('row_blocked_group', blocked, { downloadDisabled: item.disabled, copyrightHint: item.title });
    await dismissRow(blocked);

    const ids = ['3', '1', '2'];
    const pickerKeys = ['play', 'next', 'collect', 'download'];
    const first = await openSongRow(ids, '5', 'ctrl-context', pickerKeys);
    assert(ready(rowItem('download')), 'mixed group enables Download even when clicked anchor is blocked');
    await rowPicker();
    await rowCapture('row_picker', first);
    click(q('[aria-label="关闭歌曲下载"]', modal()));
    await wait(() => !modal() && document.activeElement === first.anchor,
      'cancel restores More on the original clicked outside row');
    await rowCapture('row_cancelled', first);
    const second = await openSongRow(ids, '5', 'ctrl-context', pickerKeys);
    await rowPicker();
    await rowCapture('row_reopened', second);
    click(q('[data-playlist-id="81"]', modal()));
    await wait(() => !modal() && feedback() === '已添加 2 首到下载列表，跳过 1 首' &&
      document.activeElement === second.anchor, 'confirmed row group stays on History and reports the skip');
    await published(['2', '3']);
    await rowCapture('row_published', second);

    const single = await openSongRow(['4'], '4', 'button',
      ['play', 'next', 'comments', 'collect', 'download', 'copy', 'remove']);
    let chooserMounted = false;
    const watchChooser = new MutationObserver(records => {
      const chooser = 'dialog[data-search-group-download-picker],dialog[data-search-group-purchased]';
      chooserMounted ||= records.some(record => record.target instanceof Element &&
        record.target.matches(chooser) || [...record.addedNodes].some(node =>
        node instanceof Element && (node.matches(chooser) || q(chooser, node))));
    });
    watchChooser.observe(document.body, { childList: true, subtree: true, attributes: true,
      attributeFilter: ['open'] });
    try {
      click(rowItem('download'));
      await wait(() => feedback() === '已添加 1 首到下载列表' && !modal() && !quality() &&
        document.activeElement === single.anchor, 'liked singleton directly starts a download');
      await published(['2', '3', '4']);
      assert(!chooserMounted, 'liked singleton never mounts a chooser');
    } finally { watchChooser.disconnect(); }
    await rowCapture('row_liked_singleton', single, { chooserMounted });

    const completed = await openSongRow(['2'], '2', 'keyboard', catalogReveal);
    assert(!rowItem('download'), 'completed song no longer exposes Download');
    await rowCapture('row_completed_menu', completed, { downloadVisible: false, revealVisible: true });
    await dismissRow(completed);
    await revealRows();
  }
  try {
    epoch = await wait(async () => {
      const value = await invoke('account_snapshot');
      return value.status === 'authenticated' && value.epoch;
    }, 'synthetic account ready');
    await invoke('download_command', { epoch, command: {
      kind: 'configure', root: window.__historyProofDownloadRoot, path_type: 0, file_type: 1,
    } });
    const saved = await invoke('download_quality_preferences', { epoch, refresh: false });
    await invoke('download_command', { epoch, command: {
      kind: 'preferences', revision: saved.revision,
      options: { ...saved.options, quality: 'standard', container: 'audio', tags: false, cover: false, lyrics: false },
    } });
    click(await wait(() => all('.sidebar button').find(node => ready(node) && text(node) === '最近播放'), 'Recently Played sidebar'));
    if (rowProof) {
      assert(typeof rowProof.fileManager === 'boolean', 'sealed file-manager mode');
      await runRows();
      await emit('complete', { ok: true, stages, row: true });
      return;
    }
    click(await wait(() => {
      const menu = q('[data-history-menu] button.menu-trigger', page());
      return page()?.dataset.historyCategory === '歌曲' && ready(menu) && menu;
    }, 'song history loaded'));
    click(await wait(() => {
      const item = q('[role="menuitem"][data-menu-key="batch"]');
      return ready(item) && item;
    }, 'song batch menu entry'));
    await wait(() => page()?.dataset.historyView === 'batch' && rows().length === 4 && trigger()?.disabled,
      'new batch has no selected song and Download is disabled');
    for (const index of [2, 0, 1]) click(rows()[index]);
    await wait(() => same(selected(), ['01', '02', '03']), 'out-of-order clicks select native row order');
    await openPicker();
    await capture('original_anchor_picker');
    click(q('[aria-label="关闭歌曲下载"]', modal()));
    await wait(() => !modal() && document.activeElement === trigger(), 'cancel restores original Download focus');
    await capture('cancel_no_write');

    await openPicker();
    await capture('reopened_read_only');
    click(q('[data-playlist-id="81"]', modal()));
    await wait(() => !modal() && feedback() === '已添加 2 首到下载列表，跳过 1 首' && document.activeElement === trigger(),
      'owned playlist add continues and reports original omitted song');
    await published(['2', '3']);
    await capture('published_ordered');

    for (const index of [0, 1, 2, 3]) click(rows()[index]);
    await wait(() => same(selected(), ['04']) && ready(trigger()), 'only liked song 4 selected');
    click(trigger());
    await wait(() => feedback() === '已添加 1 首到下载列表' && !modal() && !quality(), 'liked singleton direct admission');
    await published(['2', '3', '4']);
    await capture('direct_liked_singleton');
    await emit('complete', { ok: true, stages });
  } catch (error) {
    await emit('failed', { ok: false, step, error: error?.message ?? String(error),
      diagnostics: { selected: selected(), browse: browseIds(), menu: keys(),
        feedback: feedback(), rowError: rowError(), destinations: destinations(),
        pickerMessage: text(q('.collect-load', modal() ?? null)), facts: epoch ? await facts().catch(() => null) : null },
    });
  }
})();
