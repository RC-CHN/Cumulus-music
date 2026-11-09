// Actual Downloads page, download commands, scheduler, native rates and publication.
// The fixture only supplies local media bytes and gates their delivery.
(async () => {
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, payload = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...payload } });
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (condition, label) => { if (!condition) throw Error(label); };
  let phase = 'start', nextId = 0, count = 0, taskId, epoch, last;
  async function wait(observe, label) {
    phase = label;
    const deadline = performance.now() + 16_000;
    do {
      const value = await observe();
      if (value) return value;
      await sleep(60);
    } while (performance.now() < deadline);
    throw Error(label);
  }
  window.__downloadSpeedAck = {};
  async function fixture(action) {
    const id = ++nextId;
    await emit('download-speed-fixture', { id, action });
    const reply = await wait(() => window.__downloadSpeedAck[id], 'fixture ' + action);
    delete window.__downloadSpeedAck[id];
    assert(reply.ok, reply.error || 'fixture failed');
    return reply;
  }
  const page = () => document.querySelector('.downloads-page');
  const row = () => page()?.querySelector(`[data-download-task="${taskId}"]`);
  const statusNode = () => row()?.querySelector('.download-progress-status');
  const statusText = () => statusNode()?.textContent.trim() ?? null;
  const rowButton = (name) => row()?.querySelector(`button[aria-label="${name} 合成实时下载速率"]`);
  const enabled = (node) => node?.isConnected && !node.matches(':disabled,[aria-disabled="true"]');
  const tab = (id) => page()?.querySelector(`#download-tab-${id}`);
  const positive = (text) => /^(?:[1-9][0-9]* (?:B|KB)\/s|[1-9][0-9]*\.[0-9] MB\/s)$/.test(text ?? '');
  function click(node) {
    assert(enabled(node), 'enabled visible download control');
    node.focus({ preventScroll: true });
    node.click();
  }
  async function snapshot(tab = 'active') {
    const view = await invoke('downloads', { epoch, query: {
      tab, category: tab === 'active' ? 'all' : 'song', keyword: '', sort: 'completed',
      descending: false, offset: 0, limit: 10,
    } });
    assert(view.epoch === epoch, 'downloads epoch');
    const task = view.tasks.find((candidate) => candidate.id === taskId);
    assert(!task || task.track.id === '7101', 'only captured synthetic track');
    last = {
      activeCount: view.activeCount, completedCount: view.completedCount,
      id: task?.id ?? null, status: task?.status ?? null, phase: task?.phase ?? null,
      rate: task?.speedBytesPerSecond, bytes: task?.bytes ?? null, total: task?.total ?? null,
      progress: task?.progress ?? null, label: statusText(),
    };
    return last;
  }
  async function matched(check, label, category = 'active') {
    return wait(async () => {
      const native = await snapshot(category);
      return row() && check(native) && native;
    }, label);
  }
  async function capture(name, evidence) {
    phase = name;
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const bounds = row()?.getBoundingClientRect();
    assert(bounds?.height > 0 && bounds.bottom <= innerHeight && bounds.top >= 0, 'visible download row');
    assert(document.documentElement.scrollWidth <= innerWidth, 'download view width');
    const label = statusText();
    const progress = statusNode();
    if (progress) {
      const rect = progress.getBoundingClientRect();
      assert(rect.width > 0 && rect.height > 0 && rect.left >= 0 && rect.right <= innerWidth
        && progress.scrollHeight <= progress.clientHeight && getComputedStyle(progress).visibility === 'visible',
      'speed or status label is visible without clipping');
    }
    assert(name === 'flowing' ? positive(label) : label === evidence.label, 'status still visible before capture');
    await emit(`ux_${String(++count).padStart(2, '0')}_${name}`, {
      ...evidence, label, visibleRow: true,
    });
  }
  try {
    const account = await wait(async () => {
      const value = await invoke('account_snapshot');
      return value.status === 'authenticated' && value;
    }, 'sealed authenticated startup');
    epoch = account.epoch;
    const button = await wait(
      () => [...document.querySelectorAll('.sidebar button')].find((node) => enabled(node) && node.textContent.trim() === '下载管理'),
      'download manager route',
    );
    click(button);
    await wait(() => enabled(tab('active')), 'download tabs');
    click(tab('active'));
    assert((await snapshot()).activeCount === 0 && last.completedCount === 0, 'fresh download inventory');

    const { root } = await fixture('inspect');
    const command = (command) => invoke('download_command', { epoch, command });
    const configured = await command({ kind: 'configure', root, path_type: 0, file_type: 0 });
    assert(configured.epoch === epoch && !configured.failed.length, 'configured isolated native directory');
    const start = await command({ kind: 'start', song_ids: ['7101'], options: {
      quality: 'lossless', tags: false, cover: false, lyrics: false, container: 'audio',
    } });
    assert(start.epoch === epoch && !start.failed.length && start.accepted.length === 1, 'one real native task');
    [taskId] = start.accepted;
    assert(/^[0-9a-f]{32}$/.test(taskId), 'native task id');

    const active = (state) => state.status === 'downloading' && state.phase === 'transfer'
      && state.activeCount === 1 && state.completedCount === 0;
    const zero = (state) => active(state) && state.rate === 0 && state.label === '0 B/s';
    await capture('open', await matched(zero, 'live open body before first bytes'));

    await fixture('flow');
    await capture('flowing', await matched(
      (state) => active(state) && Number.isSafeInteger(state.rate) && state.rate > 0 && positive(state.label),
      'measured native rate and visible speed',
    ));

    await fixture('stall');
    await capture('stalled', await matched(zero, 'same live body ages back to visible zero'));
    const pause = await wait(() => enabled(rowButton('暂停下载')) && rowButton('暂停下载'), 'pause control');
    click(pause);
    await wait(async () => (await fixture('inspect')).report.pauseRequested, 'native row pause intent');
    await capture('pausing', await matched(
      (state) => active(state) && state.rate === null && state.label === '正在下载',
      'pause intent suppresses speed while blocked read is still open',
    ));

    await fixture('wake_pause');
    const paused = await matched(
      (state) => state.status === 'paused' && state.rate === null && state.label === '已暂停'
        && state.bytes > 0 && state.bytes < state.total && enabled(rowButton('继续下载')),
      'native pause committed and paused status rendered',
    );
    await capture('paused', paused);

    click(rowButton('继续下载'));
    await capture('resumed', await matched(
      (state) => zero(state) && state.bytes === paused.bytes,
      'range resume starts with a fresh zero sample',
    ));
    await fixture('finish');
    const completed = await wait(async () => {
      const state = await snapshot('completed');
      return state.status === 'complete' && state.rate === null && state.progress === 1 && state;
    }, 'real native publication');
    click(tab('song'));
    await wait(
      () => tab('song')?.getAttribute('aria-selected') === 'true' && row()
        && !row().querySelector('.download-progress') && row().textContent.includes('合成实时下载速率'),
      'completed song row replaces the progress status',
    );
    await capture('completed', { ...completed, label: null });
    await emit('complete', { ok: true, stageCount: count, taskId, liveDownloadSpeedUi: true });
  } catch (error) {
    await emit('failed', { ok: false, phase, stageCount: count,
      error: error instanceof Error ? error.message : error?.kind ?? String(error), last });
  }
})();
