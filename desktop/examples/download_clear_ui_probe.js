// Real Tauri window, native task state and consent. Only synthetic local work.
(async () => {
  const one = (s) => document.querySelector(s);
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const assert = (ok, label) => { if (!ok) throw Error(label); };
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const wait = async (predicate, label) => {
    for (let i = 0; i < 200; ++i) {
      if (await predicate()) return;
      await sleep(60);
    }
    throw Error(label);
  };
  const button = (label, within = document) => [...within.querySelectorAll('button')].find(
    (el) => el.getAttribute('aria-label') === label || el.textContent.trim() === label,
  );
  const click = (el) => {
    assert(el && !el.disabled, 'required action not available');
    el.focus({ preventScroll: true });
    el.click();
  };
  async function fixture(mode) {
    window.__downloadPageAck = undefined;
    await emit('download-page-fixture', { mode });
    await wait(() => window.__downloadPageAck, 'fixture ' + mode);
    assert(window.__downloadPageAck.ok, window.__downloadPageAck.message);
  }
  const dialog = () => one('.download-dialog[open]');
  const clear = () => button('全部清空');
  const confirm = () => dialog() && button('清空正在下载', dialog());
  let count = 0;
  async function capture(label, data = {}) {
    await wait(() => [...document.querySelectorAll('dialog,.download-page')].every(
      (el) => el.getAnimations().every((animation) => !animation.pending && animation.playState !== 'running'),
    ), 'animations settled');
    assert(document.documentElement.scrollWidth <= innerWidth + 1, 'window width');
    await emit(`ux_${String(++count).padStart(2, '0')}_${label}`, data);
  }
  try {
    await wait(() => one('.track-row'), 'authenticated initial view');
    await fixture('clear_seed');
    const { epoch } = await invoke('account_snapshot');
    const read = () => invoke('downloads', { epoch, query: {
      tab: 'active', category: 'all', keyword: '', sort: 'completed', descending: true, offset: 0, limit: 100,
    } });
    const verify = async (expected) => {
      const active = await read();
      const ids = active.tasks.map((task) => Number(task.track.id)).sort((a,b) => a-b);
      assert(JSON.stringify(ids) === JSON.stringify(expected), 'exact active native IDs: ' + ids);
      assert(active.tasks.every((task) => task.status === 'paused'), 'native rows are still paused');
    };
    click(button('下载管理'));
    await wait(() => one('#download-tab-active'), 'download management');
    click(one('#download-tab-active'));
    await wait(() => clear() && !clear().disabled, 'active list ready');
    await verify([701, 702]);
    const open = async () => {
      click(clear());
      await wait(() => confirm() && !confirm().disabled, 'native confirmation ready');
      assert(one('[data-download-clear-count]').textContent.includes('2 项'), 'captured exact count');
    };
    await open();
    assert(document.activeElement === button('返回', dialog()), 'safe initial focus');
    await capture('prepared', { count: 2 });
    click(button('返回', dialog()));
    await wait(() => !dialog() && document.activeElement === clear(), 'cancel restores focus');
    await verify([701, 702]);
    await capture('dismissed', { active: 2 });

    await open();
    await fixture('clear_late');
    await verify([701, 702, 703]);
    assert(one('[data-download-clear-count]').textContent.includes('2 项'), 'late row does not change confirmation');
    await capture('late_task', { count: 2, active: 3 });
    click(confirm());
    await wait(() => !dialog() && one('.download-page [role="status"]')?.textContent.includes('已受理 2 项'),
      'native cancellation result applied');
    await verify([703]);
    assert([...document.querySelectorAll('[data-download-task]')].some(
      (row) => row.textContent.includes('合成取消任务 703'),
    ), 'late task remains visible');
    await capture('confirmed', { active: 1 });
    await emit('complete', { ok: true, downloadClearVerified: true, stageCount: count,
      realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', { message: String(error), stageCount: count,
      view: document.body.innerText.slice(0, 1400) });
  }
})();
