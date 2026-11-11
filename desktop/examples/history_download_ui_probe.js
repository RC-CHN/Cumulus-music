// Exercise real History controls and the download dialog using closed services.
(async () => {
  const q = (selector, root = document) => root?.querySelector(selector);
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])];
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const text = node => node?.textContent.trim();
  const ready = node => node?.isConnected && node.getClientRects().length &&
    !node.matches(':disabled,[aria-disabled="true"]') && !node.closest('[inert]');
  const assert = (ok, label) => { if (!ok) throw Error(label); };
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const page = () => q('.history-page:not(.page-leave-active)');
  const modal = () => q('dialog.download-dialog[open]:not(.modal-leave-active)');
  const rows = () => all('.history-window input[type="checkbox"]', page());
  const trigger = () => q('.history-download-selected', page());
  const submit = () => q('footer button.primary', modal());
  const cancel = () => all('footer button', modal()).find(node => text(node) === '取消');
  const choices = () => all('input[name="voice-download-quality"]', modal()).map(node => node.value);
  const status = () => all('[role="status"]', modal()).map(text).join(' ');
  const rowProof = window.__historyVoiceRowDownloadProof;
  const row = id => q(`[data-history-row-id="${id}"]`, page());
  const more = id => q(`[data-history-row-more="${id}"]`, page());
  const rowMenu = () => q('[role="menu"][aria-label="播放记录操作"]:not([inert])');
  const rowItem = key => q(`[data-menu-key="${key}"]`, rowMenu());
  const menuKeys = () => all('[data-menu-key]', rowMenu()).map(item => item.dataset.menuKey);
  const stages = [];
  let epoch, step = 'startup';
  async function wait(observe, label) {
    step = label;
    const deadline = performance.now() + 18_000;
    do {
      const result = await observe();
      if (result) return result;
      await sleep(65);
    } while (performance.now() < deadline);
    throw Error(label);
  }
  function click(node) {
    assert(ready(node), 'clickable control');
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    node.click();
  }
  const button = (label, root) => all('button', root).find(node => ready(node) && text(node) === label);
  const list = tab => invoke('downloads', { epoch, query: {
    tab, category: 'voice', keyword: '', sort: 'none', descending: false, offset: 0, limit: 20,
  } });
  async function facts() {
    const active = await list('active'), complete = await list('completed');
    return { active: active.tasks.length, tasks: complete.tasks.map(row => ({
      voice: row.track.voiceId, media: row.track.id, quality: row.actualQuality,
      source: row.source, path: row.path, bytes: row.bytes, encrypted: row.encrypted,
    })).sort((a, b) => a.voice.localeCompare(b.voice)) };
  }
  async function capture(name, rowState = {}) {
    stages.push(name);
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await emit(`ux_${String(stages.length).padStart(2, '0')}_${name}`, {
      title: text(q('h2', modal() ?? null)) ?? null, choices: choices(), status: status(),
      focusRestored: !!trigger() && document.activeElement === trigger(),
      canConfirm: !!ready(submit()), selected: rows().filter(node => node.checked).length,
      compact: !!modal()?.classList.contains('history-voice-quality-dialog'),
      showsDirectory: !!q('[aria-label="下载目录"]', modal() ?? null),
      showsMetadata: !!q('.download-checkboxes', modal() ?? null),
      ...await facts(), ...rowState,
    });
  }
  async function enterBatch() {
    click(await wait(() => button('最近播放', q('.sidebar')), 'recently played sidebar'));
    const voiceTab = () => button('声音', q('[aria-label="播放记录类型"]', page()));
    const tab = await wait(voiceTab, 'voice history tab');
    if (!tab.getAttribute('aria-current')) click(tab);
    const menu = await wait(() => {
      const menu = q('[data-history-menu] button.menu-trigger', page());
      return page()?.dataset.historyCategory === '声音' && ready(menu) && menu;
    }, 'voice history is loaded');
    click(menu);
    click(await wait(() => {
      const item = q('[role="menuitem"][data-menu-key="batch"]');
      return ready(item) && item;
    }, 'batch action'));
    await wait(() => page()?.dataset.historyView === 'batch' && rows().length >= 3 &&
      trigger()?.disabled, 'blank batch selection with disabled download');
    assert(rows()[0].getAttribute('aria-label').includes('臺灣') &&
      rows()[1].getAttribute('aria-label').includes('合成记录 2'), 'native history first two rows');
  }
  async function close() {
    click(cancel());
    await wait(() => !modal() && document.activeElement === trigger(), 'dialog close restores toolbar focus');
  }
  async function open() {
    click(await wait(() => ready(trigger()) && trigger(), 'selection enables voice download'));
    await wait(() => text(q('h2', modal() ?? null)) === '选择下载音质' && modal(), 'official quality chooser opens');
  }
  const offered = ['play', 'next', 'comments', 'download', 'share', 'copy', 'remove'];
  const located = ['play', 'next', 'comments', 'share', 'copy', 'reveal', 'remove'];
  const inVoiceHistory = () => page()?.dataset.historyCategory === '声音' &&
    page()?.dataset.historyView === 'browse' && page()?.getAttribute('aria-busy') === 'false' &&
    !page()?.closest('.page-enter-active,.page-leave-active') &&
    !q('.history-content:is(.catalog-tab-enter-active,.catalog-tab-leave-active)');
  async function enterVoiceRows() {
    click(await wait(() => button('最近播放', q('.sidebar')), 'recently played sidebar'));
    const tab = await wait(() => button('声音', q('[aria-label="播放记录类型"]', page())),
      'voice history tab');
    if (!tab.getAttribute('aria-current')) click(tab);
    await wait(() => inVoiceHistory() && ['1', '2', '3'].every(id => ready(more(id))),
      'settled Recently Played voice browse rows');
  }
  async function openRow(ids, clicked, reveal = false, gesture = 'button') {
    for (const [index, id] of ids.entries()) {
      const target = row(id);
      assert(ready(target), 'mounted voice row ' + id);
      target.dispatchEvent(new MouseEvent('click', {
        bubbles: true, cancelable: true, button: 0, ctrlKey: index > 0,
      }));
    }
    const anchor = more(clicked);
    assert(ready(anchor), 'connected voice More');
    anchor.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'instant' });
    anchor.focus({ preventScroll: true });
    // In production a pending menu is cancelled when its scroll/focus anchor
    // moves. Complete fixture setup before issuing the one actual gesture.
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    let previous, stableSince;
    await wait(() => {
      if (!inVoiceHistory() || more(clicked) !== anchor || document.activeElement !== anchor) return;
      const bounds = anchor.getBoundingClientRect();
      const box = [bounds.left, bounds.top, bounds.width, bounds.height];
      if (!same(previous, box)) {
        previous = box;
        stableSince = performance.now();
        return;
      }
      return performance.now() - stableSince >= 105;
    }, 'stable voice row anchor before menu ' + clicked);
    if (gesture === 'keyboard') {
      assert(!anchor.dispatchEvent(new KeyboardEvent('keydown', {
        bubbles: true, cancelable: true, key: 'F10', shiftKey: true,
      })), 'Shift+F10 handled by the voice history row');
    } else if (gesture === 'ctrl-context') {
      const target = row(clicked);
      const bounds = target.getBoundingClientRect();
      assert(!target.dispatchEvent(new MouseEvent('contextmenu', {
        bubbles: true, cancelable: true, button: 2, ctrlKey: true,
        clientX: bounds.left + 18, clientY: bounds.top + 12,
      })), 'Ctrl+right-click preserves the preexisting group');
    } else anchor.click();
    const key = reveal ? 'reveal' : 'download';
    await wait(() => ready(rowItem(key)) && anchor.getAttribute('aria-expanded') === 'true',
      'visible voice row menu ' + key + ' for ' + clicked);
    const expected = reveal ? located : offered;
    assert(same(menuKeys(), gesture === 'ctrl-context' ? expected.filter(item => item !== 'remove') :
      expected), 'official menu and exclusive file action');
    assert(text(rowItem(key)) === (reveal ? '打开文件所在目录' : '下载'), 'file action label');
    assert(same(all('[data-history-row-id][aria-selected="true"]', page())
      .map(node => node.dataset.historyRowId).sort(), [...ids].sort()), 'exact voice row group selected');
    return { clicked, selectedIds: ids, menuKeys: menuKeys(), gesture };
  }
  async function rowQuality() {
    click(rowItem('download'));
    await wait(() => !rowMenu() && text(q('h2', modal())) === '选择下载音质' &&
      modal()?.classList.contains('history-voice-quality-dialog'), 'compact voice row quality chooser');
  }
  async function rowCancel(clicked) {
    click(cancel());
    await wait(() => !modal() && inVoiceHistory() && document.activeElement === more(clicked),
      'Cancel restores clicked voice More without leaving History');
  }
  async function runVoiceRows() {
    await enterVoiceRows();
    const first = await openRow(['1'], '1', false, 'keyboard');
    await capture('row_download_offered', first);
    await rowQuality();
    await wait(() => same(choices(), ['standard']) && ready(submit()), 'single available quality');
    await capture('row_single_choice', first);
    await rowCancel('1');
    await capture('row_cancelled', { ...first, focusRestored: true });

    const group = await openRow(['2', '1'], '1');
    await rowQuality();
    await wait(() => same(choices(), ['standard', 'higher', 'exhigh']) && ready(submit()),
      'multi-selection offers the three ordinary episode qualities');
    assert(text(submit()) === '确定', 'official confirmation label');
    await capture('row_group_choice', group);
    click(submit());
    await wait(async () => {
      if (modal() || !inVoiceHistory() ||
          text(q('.history-row-menu-notice[role="status"]', page())) !== '已添加 2 个单集到下载列表')
        return;
      const result = await facts();
      return !result.active && same(result.tasks.map(task => task.voice), ['1', '2']);
    }, 'two exact audio publications complete while staying in voice History');
    await capture('row_published', { ...group, stayedInHistory: true,
      notice: text(q('.history-row-menu-notice[role="status"]', page())) });

    const existing = await openRow(['2', '1'], '3', false, 'ctrl-context');
    let mountedQuality = false;
    const observeQuality = new MutationObserver(records => {
      mountedQuality ||= records.some(record =>
        record.target instanceof Element && record.target.matches('dialog.download-dialog') ||
        [...record.addedNodes].some(node => node instanceof Element &&
          (node.matches('dialog.download-dialog') || !!q('dialog.download-dialog', node))));
    });
    observeQuality.observe(document.body, {
      childList: true, subtree: true, attributes: true, attributeFilter: ['open'],
    });
    try {
      click(rowItem('download'));
      await wait(() => !rowMenu() && !modal() && inVoiceHistory() &&
        document.activeElement === more('3') &&
        text(q('.history-row-menu-notice[role="status"]', page())) === '资源已下载',
      'download of complete retained group displays no chooser');
      await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      assert(!mountedQuality && !modal(), 'zero eligible episodes never mount a quality chooser');
    } finally {
      observeQuality.disconnect();
    }
    await capture('row_already_downloaded', { ...existing, focusRestored: true, mountedQuality,
      notice: text(q('.history-row-menu-notice[role="status"]', page())) });

    const file = await openRow(['2', '1'], '1', true);
    await capture('row_reveal_offered', file);
    if (rowProof.fileManager) click(rowItem('reveal'));
    else rowMenu().dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await wait(() => !rowMenu() && !modal() && inVoiceHistory() &&
      document.activeElement === more('1') && !q('.history-row-menu-error', page()),
    'reveal action retains the clicked voice row focus and route');
    // In Linux the Python runner waits for the private D-Bus ShowItems receipt
    // before acknowledging this screenshot. Other platforms do not launch a shell.
    await capture('row_reveal_returned', { ...file, focusRestored: true,
      fileManagerDispatched: rowProof.fileManager, stayedInHistory: true });
    assert(!q('.history-row-menu-error', page()), 'exact file-manager handoff produced no row error');

    const restricted = await openRow(['1', '3'], '3');
    await rowQuality();
    await wait(() => status().includes('当前节目不支持普通下载') && !ready(submit()),
      'clicked unfinished row checks only its remaining denied episode');
    await capture('row_restricted', restricted);
    await rowCancel('3');
    await capture('row_restricted_closed', { ...restricted, focusRestored: true });
  }
  try {
    epoch = await wait(async () => {
      const state = await invoke('account_snapshot');
      return state.status === 'authenticated' && state.epoch;
    }, 'synthetic account ready');
    await invoke('download_command', { epoch, command: {
      kind: 'configure', root: window.__historyProofDownloadRoot, path_type: 0, file_type: 1,
    } });
    const saved = await invoke('download_quality_preferences', { epoch, refresh: false });
    await invoke('download_command', { epoch, command: {
      kind: 'preferences', revision: saved.revision,
      options: { ...saved.options, tags: false, cover: false, lyrics: false },
    } });
    if (rowProof) {
      assert(typeof rowProof.fileManager === 'boolean', 'sealed file-manager mode');
      await runVoiceRows();
      await emit('complete', { ok: true, stages, row: true });
      return;
    }
    await enterBatch();

    click(rows()[0]);
    await open();
    await wait(() => same(choices(), ['standard']) && ready(submit()), 'single permitted quality');
    await capture('single_choice');
    await close();
    await capture('cancel_no_task');

    click(rows()[0]);
    click(rows()[2]);
    await open();
    await wait(() => status().includes('当前节目不支持普通下载') && !ready(submit()), 'restricted program');
    await capture('restricted');
    await close();
    click(rows()[2]);
    click(rows()[1]);
    click(rows()[0]);
    await open();
    await wait(() => same(choices(), ['standard', 'higher', 'exhigh']) && ready(submit()), 'batch qualities');
    assert(text(submit()) === '确定', 'official confirmation label');
    await capture('batch_choice');
    click(submit());
    await wait(async () => {
      if (modal()) return false;
      const { active, tasks } = await facts();
      return active === 0 && same(tasks.map(row => row.voice), ['1', '2']);
    }, 'two distinct episodes are published by Rust');
    await capture('published');

    await enterBatch();
    click(rows()[0]);
    click(rows()[1]);
    await open();
    await wait(() => status().includes('无需重复下载') && !ready(submit()), 'existing tasks skipped');
    await capture('duplicates');
    await close();
    await emit('complete', { ok: true, stages });
  } catch (error) {
    await emit('failed', { ok: false, step, error: error?.message ?? String(error), stages,
      row: !!rowProof, menu: menuKeys(), modal: text(q('h2', modal())), status: status(),
      rowError: text(q('.history-row-menu-error', page())) });
  }
})();
