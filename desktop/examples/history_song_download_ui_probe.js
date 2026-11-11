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
  async function capture(name) {
    stages.push(name);
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await emit(`ux_${String(stages.length).padStart(2, '0')}_${name}`, {
      selected: selected(), title: text(q('h2', modal() ?? null)) ?? null,
      feedback: feedback(), destinations: destinations(), quality: !!quality(),
      returnFocus: document.activeElement === trigger(), ...await facts(),
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
      diagnostics: { selected: selected(), feedback: feedback(), destinations: destinations(),
        pickerMessage: text(q('.collect-load', modal() ?? null)), facts: epoch ? await facts().catch(() => null) : null },
    });
  }
})();
