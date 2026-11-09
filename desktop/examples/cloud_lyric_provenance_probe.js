// Sealed cloud service and null audio; use production IPC and lyric view.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (kind, stage, fields = {}) => invoke('plugin:event|emit', {
    event: 'mini-proof', payload: { kind, stage, ...fields },
  });
  const panel = () => one('#vinyl-lyrics-content');
  const source = () => panel()?.querySelector('[data-cloud-lyric-source]');
  const viewport = () => panel()?.querySelector('.lyric-scroll');
  const note = () => panel()?.querySelector('.lyric-cache-note');
  const button = (label) => one(`.playerbar button[aria-label="${label}"]`);
  const title = '来自我的音乐网盘';
  let epoch, count = 0, last = 'startup';
  const playback = () => invoke('player_snapshot', { epoch });
  const assert = (value, message) => { if (!value) throw Error(message); };
  async function wait(test, message) {
    last = message;
    for (let i = 0; i < 200; ++i) {
      const value = await test();
      if (value) return value;
      await sleep(65);
    }
    throw Error(message);
  }
  async function capture(name, evidence = {}) {
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await sleep(220);
    const stage = `ux_${String(++count).padStart(2, '0')}_${name}`;
    const value = await playback();
    window.__miniRecorded = null;
    await emit('snapshot', stage, {
      captureTitle: 'Cumulus', windowKind: 'main', source: source()?.textContent.trim() ?? null,
      lyricRows: panel()?.querySelectorAll('[data-line]').length ?? 0,
      playId: value.playId, trackId: value.track?.id, resourceKind: value.resourceKind,
      ...evidence,
    });
    await wait(() => window.__miniRecorded === stage, 'captured ' + name);
  }
  const query = { tab: 'uploaded', keyword: '', sort: 'natural', descending: true, offset: 0, limit: 200 };
  async function select(trackId, revision) {
    const prior = (await playback()).playId;
    const selection = revision
      ? { kind: 'cloud', query, start_id: trackId, revision, quality: 'standard' }
      : { kind: 'songs', ids: [trackId], index: 0, quality: 'standard' };
    await invoke('play_selection', { epoch, selection });
    const current = await wait(async () => {
      const value = await playback();
      return value.state === 'playing' && value.playId !== prior &&
        value.track?.id === trackId && value;
    }, 'play ' + trackId);
    assert(current.resourceKind === (revision ? 'cloud' : 'catalog'), 'native source scope');
    await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    await wait(async () => (await playback()).state === 'paused', 'pause ' + trackId);
    return current.playId;
  }
  async function centered(row) {
    const list = viewport();
    const box = list.getBoundingClientRect(), rowBox = row.getBoundingClientRect();
    list.scrollTop += (rowBox.top + rowBox.bottom - box.top - box.bottom) / 2;
    list.dispatchEvent(new WheelEvent('wheel', { deltaY: 1, bubbles: true }));
    list.dispatchEvent(new Event('scroll'));
  }
  try {
    const account = await wait(async () => {
      const value = await invoke('account_snapshot');
      return value.status === 'authenticated' && one('.playerbar') && value;
    }, 'sealed account');
    epoch = account.epoch;
    const cloud = await invoke('cloud_page', { epoch, query, refresh: true });
    const revision = cloud.revision;
    assert(typeof revision === 'string' && revision.length, 'sealed cloud list revision');
    const playId = await select('1001', revision);
    (await wait(() => button('打开播放详情'), 'open current vinyl')).click();
    const marker = await wait(() => source()?.closest('.lyric-scroll') && source(), 'cloud provenance');
    const row = await wait(() => one('#vinyl-lyrics-content [data-line="0"]'), 'first timed lyric');
    assert(marker.textContent.trim() === title && marker.nextElementSibling === row &&
      marker.hasAttribute('data-non-selectable') && !marker.hasAttribute('data-line'), 'untimed provenance row');
    const selection = window.getSelection(), range = document.createRange();
    range.setStartBefore(marker);
    range.setEndAfter(row.querySelector('[data-lyric-text]'));
    selection.removeAllRanges();
    selection.addRange(range);
    const transfer = new DataTransfer();
    const copy = new ClipboardEvent('copy', { bubbles: true, cancelable: true, clipboardData: transfer });
    document.dispatchEvent(copy);
    selection.removeAllRanges();
    assert(copy.defaultPrevented && transfer.getData('text/plain').trim() === '云盘合成歌词 1',
      'copy contains the lyric only');
    await capture('populated', { copyExcludedSource: true });

    const target = one('#vinyl-lyrics-content [data-line="3"]');
    await centered(target);
    const browse = await wait(() => target.classList.contains('browsed') &&
      one('.lyric-browse-time[aria-label="从歌词时间 00:04 播放"]'), 'correct browsed timestamp after source row');
    browse.click();
    await wait(async () => {
      const value = await playback();
      return value.playId === playId && value.state === 'playing' &&
        value.position >= 4 && value.position < 5;
    }, 'native seek and resume at 4 seconds');
    await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    await capture('seek', { nativeSeekSeconds: 4 });

    await select('1002', revision);
    await wait(() => source()?.parentElement.classList.contains('lyric-cloud-empty') &&
      source().nextElementSibling?.textContent.trim() === '暂无歌词', 'successful empty cloud lyric');
    assert(!note() && !viewport(), 'successful empty state offers no retry or timed lines');
    await capture('empty');

    await select('1003', revision);
    const failed = await wait(() => note()?.textContent.includes('歌词加载失败') && note(), 'failed cloud lyric');
    assert(!source() && !panel().textContent.includes('暂无歌词') &&
      !!failed.querySelector('button'), 'failure distinct from confirmed empty');
    await capture('failed_read');
    failed.querySelector('button').click();
    await wait(() => source()?.parentElement.classList.contains('lyric-cloud-empty') && !note(),
      'retry returns successful empty state');
    await capture('retry');

    await select('1001');
    await wait(() => one('#vinyl-lyrics-content [data-line="0"]')?.textContent.includes('合成歌词 0'),
      'same ID in ordinary catalog has its own lyrics');
    assert(!source() && !note(), 'cloud provenance does not leak to ordinary song');
    await capture('catalog_same_id');
    await emit('complete', 'complete', { ok: true, stageCount: count,
      cloudLyricProvenance: true, realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', 'failed', { ok: false, last, message: String(error),
      stageCount: count, playback: epoch ? await playback().catch(() => null) : null,
      source: source()?.textContent.trim(), note: note()?.textContent.trim() });
  }
})();
