// Read-only podcast navigation from an actual native download inventory.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])];
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, payload = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...payload },
  });
  const assert = (condition, message) => { if (!condition) throw Error(message); };
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const wait = async (read, message, timeout = 18000) => {
    const end = performance.now() + timeout;
    while (performance.now() < end) {
      const result = await read();
      if (result) return result;
      await sleep(50);
    }
    throw Error(message);
  };
  const button = (label, root = document) =>
    all('button', root).find(node => node.getAttribute('aria-label') === label || node.textContent.trim() === label);
  function click(node) {
    assert(node?.isConnected && !node.disabled && !node.closest('[inert]'), 'connected enabled button');
    node.focus({ preventScroll: true });
    node.click();
  }
  const voiceTab = () => one('#download-tab-voice');
  const link = () => one('button[data-download-action="podcast"]');
  const heading = () => one('.podcast-cover h2')?.textContent.trim();
  const voiceName = '合成下载节目所属播客';
  const pending = () => one('.page-enter-active,.page-leave-active,.download-section-enter-active,.download-section-leave-active');
  const ready = () => one('.downloads-page') && !pending() &&
    one('#download-content')?.getAttribute('aria-busy') === 'false';
  const taskRow = task => one(`[data-download-task="${task.id}"]`);
  const query = (category, keyword = '') =>
    ({ tab: 'completed', category, keyword, sort: 'completed', descending: true, offset: 0, limit: 100 });
  let epoch, baseline, count = 0, step = 'startup';
  const player = () => invoke('player_snapshot', { epoch });
  const samePlayer = sample => sample.state === 'paused' && !sample.error &&
    ['playId', 'entryId', 'queueRevision', 'position', 'downloadTaskId'].every(key => sample[key] === baseline[key]);
  async function capture(label, data = {}) {
    step = label;
    await wait(() => !pending(), 'page transition finished');
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await sleep(120);
    assert(samePlayer(await player()), 'read-only navigation keeps the paused native file');
    const stage = `ux_${String(++count).padStart(2, '0')}_${label}`;
    window.__queueUiStageRecorded = '';
    await emit(stage, data);
    await wait(() => window.__queueUiStageRecorded === stage, 'native stage recorded');
  }
  try {
    await wait(async () => {
      const account = await invoke('account_snapshot');
      if (account.status !== 'authenticated' || button('打开设置')?.disabled !== false || !one('.track-row')) return false;
      epoch = account.epoch;
      return true;
    }, 'isolated navigation-ready app');
    step = 'inventory';
    await emit('download-page-fixture', { mode: 'seed' });
    await wait(() => window.__downloadPageAck, 'native download records seeded', 70000);
    assert(window.__downloadPageAck.ok, window.__downloadPageAck.message);
    const songQuery = query('song', '合成单曲 001');
    const [song] = (await invoke('downloads', { epoch, query: songQuery })).tasks;
    assert(song?.track.id === '1' && !song.voicePodcast, 'ordinary downloaded song has no voice affiliation');
    step = 'paused-file';
    await invoke('play_selection', { epoch, selection: { kind: 'downloads', query: songQuery, start_id: song.id } });
    await wait(async () => {
      const state = await player();
      return state.state === 'playing' && state.downloadTaskId === song.id && state.position > .2;
    }, 'real native file playback advances');
    await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    await invoke('player_action', { epoch, action: { kind: 'seek', seconds: 4 } });
    let previous;
    baseline = await wait(async () => {
      const sample = await player();
      const matching = sample.state === 'paused' && sample.downloadTaskId === song.id &&
        Math.abs(sample.position - 4) < .12;
      const settled = matching && sample.position === previous;
      previous = matching ? sample.position : undefined;
      return settled && sample;
    }, 'same native pause and settled clock');
    step = 'voice-section';
    click(button('下载管理'));
    await wait(ready, 'download management');
    click(voiceTab());
    await wait(() => ready() && voiceTab()?.getAttribute('aria-selected') === 'true' && link(), 'completed voice rows');
    const view = await invoke('downloads', { epoch, query: query('voice') });
    const [linked, idless, mismatched] = ['121', '122', '123'].map(id => view.tasks.find(task => task.track.id === id));
    assert(linked?.track.voiceId === '701' && linked.voicePodcast?.id === '9007199254740993' &&
      linked.voicePodcast.name === voiceName, 'native episode, media and source identities are distinct');
    assert(idless?.voicePodcast?.name === '没有编号的播客' && !idless.voicePodcast.id &&
      mismatched && !mismatched.voicePodcast, 'native omission for invalid affiliations');
    const plain = one('.download-track-subtitle > span.ellipsis', taskRow(idless));
    assert(plain?.textContent.trim() === '没有编号的播客' && !one('button[data-download-action="podcast"]', taskRow(idless)) &&
      taskRow(mismatched) && !one('button[data-download-action="podcast"]', taskRow(mismatched)) &&
      !taskRow(mismatched).textContent.includes('错误的归属不能展示'), 'plain and rejected sources are never navigable');
    assert(link() === one('button[data-download-action="podcast"]', taskRow(linked)) &&
      link().textContent.trim() === voiceName &&
      !taskRow(linked).textContent.includes('合成主播（不是播客名）'), 'row title shows containing podcast, not the presenter');
    plain.click();
    assert(ready() && !heading(), 'plain source is not a navigation action');
    await emit('focus-sample');
    await wait(() => document.hasFocus(), 'native main window focused');
    link().focus({ preventScroll: true });
    await capture('voice_rows', { linked: linked.voicePodcast, plain: idless.voicePodcast,
      mismatchedHasPodcast: !!mismatched.voicePodcast });
    step = 'podcast-detail';
    click(link());
    await wait(() => heading() === voiceName && !pending(), 'existing podcast detail opens');
    await capture('podcast_opened', { heading: heading() });
    step = 'back-to-download';
    click(button('返回上一页'));
    await wait(() => ready() && voiceTab()?.getAttribute('aria-selected') === 'true' &&
      link() && document.activeElement === link(), 'Back restores the same row subtitle focus');
    await capture('download_returned', { focus: document.activeElement?.dataset.downloadAction,
      task: document.activeElement?.closest('[data-download-task]')?.dataset.downloadTask,
      originalTask: linked.id });
    await emit('complete', { ok: true, stageCount: count, realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', { ok: false, step, error: String(error), heading: heading(),
      active: document.activeElement?.outerHTML.slice(0, 450), page: one('.main-content')?.innerText.slice(0, 1200) });
  }
})();
