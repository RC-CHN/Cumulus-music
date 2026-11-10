// Real WebView plus production IPC; only the native service/bytes are synthetic.
(async () => {
  const one = (query, root = document) => root?.querySelector(query);
  const all = (query, root = document) => [...(root?.querySelectorAll(query) ?? [])];
  const button = (name, root = document) => all('button', root).find((node) =>
    node.getAttribute('aria-label') === name || node.textContent.trim() === name);
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, facts = {}) => invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...facts } });
  const settle = () => !one('.page-enter-active,.page-leave-active,.modal-enter-active,.modal-leave-active');
  const ready = (node) => !!node?.getClientRects().length && !node.disabled && !node.closest('[inert]');
  const frame = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (pass, note) => { if (!pass) throw Error(note); };
  let checkpoint = 'bootstrap', stages = 0, epoch;
  async function wait(predicate, label, budget = 12000) {
    checkpoint = label;
    const until = performance.now() + budget;
    while (performance.now() < until) {
      const value = await predicate();
      if (value) return value;
      await sleep(30);
    }
    throw Error(label);
  }
  async function click(find, label) {
    const node = await wait(() => ready(find()) && settle() && find(), label);
    node.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    await frame();
    await wait(() => {
      if (!ready(node) || node !== find() || !settle()) return false;
      const r = node.getBoundingClientRect();
      return node.contains(document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2));
    }, label + ' hit target');
    node.focus({ preventScroll: true });
    node.click();
  }
  const list = () => one('[aria-label="已购播单和单集"]');
  const row = (kind, id) => one(`[data-purchased-kind="${kind}"][data-purchased-id="${id}"]`, list());
  const rowOrder = () => all('[data-purchased-kind]', list()).map((node) =>
    `${node.dataset.purchasedKind}:${node.dataset.purchasedId}`);
  const dialog = () => one('dialog.download-dialog[open]:not([inert])');
  const submit = () => one('form button.primary', dialog());
  const choices = () => all('input[name="voice-download-quality"]', dialog());
  const options = () => choices().map((node) => [node.value, node.closest('label').textContent.trim(), node.checked]);
  const noTasks = async () => {
    const views = await Promise.all([downloads('active'), downloads('completed')]);
    assert(views.every((view) => view.activeCount === 0 && view.completedCount === 0),
      'no program has been queued or published before explicit confirmation');
  };
  const downloads = (tab) => invoke('downloads', { epoch, query: {
    tab, category: tab === 'active' ? 'all' : 'voice', keyword: '',
    offset: 0, limit: 10, sort: 'completed', descending: true,
  } });
  const player = () => invoke('player_snapshot', { epoch });
  function samePlayer(before, after) {
    return ['state', 'playId', 'entryId', 'queueRevision'].every((key) => before[key] === after[key]);
  }
  async function stage(name, facts = {}) {
    await wait(settle, 'settle ' + name);
    await frame();
    const box = dialog()?.getBoundingClientRect();
    assert(!box || (box.left >= 0 && box.top >= 0 && box.right <= innerWidth && box.bottom <= innerHeight),
      'confirmation is inside the actual WebView');
    const label = `ux_${String(++stages).padStart(2, '0')}_${name}`;
    window.__podcastProgressStage = '';
    await emit(label, {
      ...facts, actual: document.visibilityState === 'visible', visibleRows: rowOrder(),
      screenX, screenY, ratio: devicePixelRatio,
    });
    await wait(() => window.__podcastProgressStage === label, 'host stage ' + name);
  }
  try {
    const account = await wait(async () => {
      const value = await invoke('account_snapshot');
      return value.status === 'authenticated' && value;
    }, 'synthetic account');
    epoch = account.epoch;
    const originalPlayer = await player();
    await click(() => button('我的播客', one('.sidebar-navigation[aria-label="我的"]')), 'open My Podcasts');
    await click(() => button('我的购买', one('.podcast-tabs')), 'open My Purchases');
    const originalOrder = 'radio:81,episode:81,episode:82';
    await wait(() => rowOrder().join(',') === originalOrder, 'mixed rows in server order');
    assert(button('加载更多', one('.podcast-page')) && !row('radio', '95') && !row('episode', '95'),
      'explicit continuation and no action for unknown product type');
    await noTasks();
    await stage('mixed');

    const episodeTitle = () => button('合成已购单集 81', row('episode', '81'));
    await click(episodeTitle, 'single program title selects the episode row');
    await wait(() => episodeTitle()?.getAttribute('aria-pressed') === 'true', 'selected purchased episode');
    assert(!dialog() && !one('.podcast-summary') && samePlayer(originalPlayer, await player()),
      'title selection neither navigates, plays, nor downloads');
    await stage('selected');

    await click(() => button('合成已购播单 81', row('radio', '81')), 'open purchased radio 81');
    await wait(() => one('.podcast-summary h2')?.textContent.trim() === '合成已购播单 81',
      'radio row reaches its native detail');
    await stage('radio', { heading: one('.podcast-summary h2').textContent.trim() });
    await click(() => button('返回播客列表'), 'Back to mixed purchases');
    await wait(() => rowOrder().join(',') === originalOrder, 'restored first purchase page');

    await click(() => one('[data-purchased-parent]', row('episode', '82')), 'open episode 82 parent');
    await wait(() => one('.podcast-summary h2')?.textContent.trim() === '合成所属播单 71',
      'episode parent is 71, not program 82');
    await stage('parent', { heading: one('.podcast-summary h2').textContent.trim() });
    await click(() => button('返回播客列表'), 'Back from parent');
    await wait(() => rowOrder().join(',') === originalOrder, 'restored exact purchase order');

    const download = (id) => button('下载单集', row('episode', id));
    await click(() => download('81'), 'explicitly request the denied program');
    await wait(() => dialog()?.textContent.includes('当前节目不支持普通下载。'), 'native program denial');
    assert(submit()?.disabled && options().length === 0, 'denied purchase cannot select or submit quality');
    await noTasks();
    await stage('denied', { submitDisabled: submit().disabled });
    await click(() => button('取消', dialog()), 'dismiss denied program');
    await wait(() => !dialog(), 'denied dialog closed');

    await click(() => button('加载更多', one('.podcast-page')), 'explicit purchased continuation');
    await wait(() => rowOrder().join(',') === originalOrder + ',radio:90', 'native offset appended in place');
    assert(!button('加载更多', one('.podcast-page')), 'server terminal cursor hides continuation');
    await stage('more');

    await click(() => download('82'), 'explicitly open permitted episode quality');
    await wait(() => choices().length === 2 && choices()[0].checked, 'first offered quality selected');
    assert(JSON.stringify(options()) === JSON.stringify([
      ['standard', '普通（1.0M）', true], ['higher', '较高（2.0M）', false],
    ]), 'native offered choices and sizes');
    await noTasks();
    await stage('ready', { choices: options() });
    await click(() => button('取消', dialog()), 'Cancel allowed download');
    await wait(() => !dialog(), 'allowed dialog closed');
    await noTasks();
    await stage('cancelled');

    await click(() => download('82'), 'reopen the exact allowed program');
    await wait(() => choices().length === 2 && choices()[0].checked, 'reopening resets selection');
    await click(() => choices()[1], 'choose offered higher tier');
    assert(choices()[1].checked, 'explicit second tier checked');
    const directory = one('[aria-label="下载目录"]', dialog());
    directory.value = window.__purchasedPodcastRoot;
    directory.dispatchEvent(new Event('input', { bubbles: true }));
    for (const text of ['嵌入专辑封面', '写入歌曲信息']) {
      const checkbox = () => all('.download-checkboxes label', dialog())
        .find((label) => label.textContent.trim() === text)?.querySelector('input');
      if (checkbox()?.checked) await click(checkbox, 'keep synthetic MP3 bytes: ' + text);
    }
    await noTasks();
    await stage('confirm', { choices: options(), submitEnabled: !submit().disabled });
    await click(submit, 'explicitly confirm exact allowed program and quality');
    const published = await wait(async () => {
      if (dialog()) return false;
      const result = await downloads('completed');
      return result.tasks.length === 1 && result.tasks[0].status === 'complete' &&
        !result.tasks[0].cleanupPending && result.tasks[0];
    }, 'one native published program');
    assert(published.source === 'voice' && published.track.voiceId === '82' &&
      published.track.id === '182' && published.actualQuality === 'higher' && !published.encrypted,
      'published native row preserves separate voice and media IDs');
    assert(samePlayer(originalPlayer, await player()), 'downloads and purchase navigation do not alter playback');
    await stage('published', { task: { id: published.id, voiceId: published.track.voiceId,
      mediaId: published.track.id, quality: published.actualQuality } });
    await emit('complete', { ok: true, stageCount: stages, realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', { checkpoint, error: error?.message || String(error), visibleRows: rowOrder(),
      body: document.body.innerText.slice(-1500) });
  }
})();
