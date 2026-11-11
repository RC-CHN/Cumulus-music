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
  async function capture(name) {
    stages.push(name);
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await emit(`ux_${String(stages.length).padStart(2, '0')}_${name}`, {
      title: text(q('h2', modal() ?? null)) ?? null, choices: choices(), status: status(),
      focusRestored: !!trigger() && document.activeElement === trigger(),
      canConfirm: !!ready(submit()), selected: rows().filter(node => node.checked).length,
      compact: !!modal()?.classList.contains('history-voice-quality-dialog'),
      showsDirectory: !!q('[aria-label="下载目录"]', modal() ?? null),
      showsMetadata: !!q('.download-checkboxes', modal() ?? null),
      ...await facts(),
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
    await emit('failed', { ok: false, step, error: error?.message ?? String(error) });
  }
})();
