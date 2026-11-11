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
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const assert = (ok, message) => { if (!ok) throw Error(message); };
  const page = () => q('.history-page:not(.page-leave-active)');
  const menu = () => q('[data-history-menu] button.menu-trigger', page());
  const collectAll = () => q('.history-collect-all', page());
  const collect = () => q('.history-collect-selected', page());
  const opener = () => page()?.dataset.historyView === 'batch' ? collect() : collectAll();
  const search = () => q('[aria-label="筛选播放记录"]', page());
  const titles = () => all('.history-window .history-name', page()).map(text);
  const selections = () => all('.history-window input[type="checkbox"]', page());
  const selected = () => selections().filter(node => node.checked)
    .map(node => node.getAttribute('aria-label').slice(-2));
  const modal = () => q('dialog[data-history-collect][open]:not(.modal-leave-active)');
  const anyModal = () => q('dialog[data-history-collect][open]');
  const destinationIds = () => all('[data-playlist-id]', modal() ?? null).map(row => row.dataset.playlistId);
  const feedback = () => q('.history-collect-feedback', page());
  const stages = [];
  let step = 'startup';
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
  function filter(value) {
    const input = search();
    assert(ready(input), 'history filter is available');
    input.focus();
    input.value = value;
    input.dispatchEvent(new Event('input', { bubbles: true }));
  }
  async function chooseMenu(key) {
    click(await wait(() => ready(menu()) && menu(), 'history More menu trigger'));
    click(await wait(() => {
      const item = q(`[role="menuitem"][data-menu-key="${key}"]`);
      return ready(item) && item;
    }, `history menu action: ${key}`));
  }
  async function capture(name) {
    stages.push(name);
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await emit(`ux_${String(stages.length).padStart(2, '0')}_${name}`, {
      title: text(q('h2', modal() ?? null)) ?? null,
      summary: text(q('.collect-selection-summary', modal() ?? null)) ?? null,
      destinations: destinationIds(),
      query: search()?.value,
      visibleRows: titles(),
      selected: selected(),
      feedback: text(feedback()) ?? null,
      feedbackRole: feedback()?.getAttribute('role') ?? null,
      returnFocus: document.activeElement === opener(),
    });
  }
  async function picker(summary) {
    await wait(() => modal() && getComputedStyle(modal()).opacity === '1' &&
      same(destinationIds(), ['81', '82']) &&
      text(q('.collect-selection-summary', modal())) === summary, 'owned destinations and exact source summary');
    assert(text(q('h2', modal())) === '收藏到歌单', 'collection chooser title');
  }
  async function target(id) {
    click(q(`[data-playlist-id="${id}"]`, modal()));
    await wait(() => !anyModal() && text(feedback()) === '已收藏到歌单。' &&
      document.activeElement === opener(),
    'confirmed playlist add and original focus');
  }
  try {
    await wait(async () => (await invoke('account_snapshot')).status === 'authenticated', 'synthetic account ready');
    click(await wait(() => all('.sidebar button').find(node => ready(node) && text(node) === '最近播放'), 'recently played sidebar'));
    await wait(() => page()?.dataset.historyCategory === '歌曲' && ready(collectAll()) && titles().length === 6, 'six song history rows');
    filter('合成筛选目标');
    await wait(() => same(titles(), ['合成筛选目标 · 02']), 'one visible result after filter');
    const headerSummary = '所选播放记录共 6 项，将收藏 4 首歌曲。2 项无法收藏，已略过。';
    click(collectAll());
    await picker(headerSummary);
    await capture('filtered_header_picker');
    click(q('[aria-label="关闭歌单操作"]', modal()));
    await wait(() => !anyModal() && document.activeElement === collectAll(), 'cancel and restore 收藏全部 focus');
    await capture('header_cancel');

    click(collectAll());
    await picker(headerSummary);
    await target('81');
    await capture('header_confirmed');

    filter('');
    await chooseMenu('batch');
    await wait(() => page()?.dataset.historyView === 'batch' && selections().length === 6 && collect()?.disabled, 'batch collection disabled before selection');
    for (const index of [3, 2, 1, 0]) click(selections()[index]);
    click(await wait(() => same(selected(), ['01', '02', '03', '04']) && ready(collect()) && collect(),
      'reverse gesture selects exact native rows'));
    await picker('所选播放记录共 4 项，将收藏 2 首歌曲。2 项无法收藏，已略过。');
    await capture('batch_picker');
    await target('82');
    await capture('batch_confirmed');

    for (const index of [0, 1, 3]) click(selections()[index]);
    click(collect());
    await wait(() => !anyModal() && same(selected(), ['03']) &&
      text(feedback()) === '所选播放记录没有可收藏的歌曲。', 'copyright row has no eligible collection');
    await capture('batch_empty_eligibility');
    await emit('complete', { ok: true, stages });
  } catch (error) {
    await emit('failed', { ok: false, step, error: error?.message ?? String(error),
      diagnostics: { visibleRows: titles(), selected: selected(), destinations: destinationIds(),
        summary: text(q('.collect-selection-summary', modal() ?? null)),
        errors: all('[role="alert"]').map(text), feedback: text(feedback()) },
    });
  }
})();
