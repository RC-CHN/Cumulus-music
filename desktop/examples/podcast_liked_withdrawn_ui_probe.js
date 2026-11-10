(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])];
  const text = (label, root = document) => all('button', root).find((node) => node.textContent.trim() === label);
  const pane = () => one('.podcast-page');
  const row = (id) => one(`[data-voice-id="${id}"]`, pane());
  const player = () => one('.playerbar');
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (condition, message) => { if (!condition) throw Error(message); };
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, data) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  async function wait(check, label) {
    for (let i = 0; i < 240; i++) {
      if (check()) return;
      await sleep(40);
    }
    throw Error(label);
  }
  async function stage(name, data = {}) {
    await emit(name, { actual: document.visibilityState === 'visible', ...data });
    if (name.startsWith('ux_')) await wait(() => window.__podcastProgressStage === name, name + ' captured');
  }
  let step = 'open library';
  try {
    await wait(() => text('我的播客') && !text('我的播客').disabled, step);
    text('我的播客').click();
    await wait(() => text('点赞的单集', pane()), 'liked tab');
    text('点赞的单集', pane()).click();
    await wait(() => row(501) && row(504), 'all liked rows');
    const epoch = (await invoke('account_snapshot')).epoch;
    const queue = () => invoke('player_queue', { epoch });
    const playerSnapshot = () => invoke('player_snapshot', { epoch });
    const ids = all('[data-voice-id]', pane()).map((node) => node.dataset.voiceId);
    assert(ids.join() === '501,502,503,504', 'original row order');
    assert(row(501).classList.contains('episode-withdrawn') && row(501).textContent.includes('已下线'), 'masked row stays visible');
    assert(one('button[aria-label^="播放"]', row(501)).disabled && one('button[aria-label="下载单集"]', row(501)).disabled, 'withdrawn playback controls');
    assert(!row(504).classList.contains('episode-withdrawn'), 'normal sibling stays available');
    await stage('ux_01_liked_rows', { ids });

    step = 'withdrawn gestures';
    const initial = await queue();
    one('.episode-info strong', row(501)).click();
    await wait(() => one('.podcast-withdrawn-notice', pane()), 'withdrawn click notice');
    const notice = one('.podcast-withdrawn-notice', pane());
    const box = notice.getBoundingClientRect(), content = pane().getBoundingClientRect();
    assert(notice.textContent.trim() === '该声音已下线不可收听' && box.top >= content.top && box.bottom <= content.bottom + 1, 'visible original wording');
    const context = new MouseEvent('contextmenu', { button: 2, bubbles: true, cancelable: true });
    assert(row(501).dispatchEvent(context) === false && !one('[role="menu"]', pane()), 'no context menu');
    row(501).focus();
    row(501).dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    const stillEmpty = await queue();
    assert(stillEmpty.revision === initial.revision && stillEmpty.tracks.length === 0, 'withdrawn gestures are read-only');
    await stage('ux_02_withdrawn', { notice: notice.textContent.trim(), queue: stillEmpty.tracks.length });

    step = 'play valid target past unavailable siblings';
    await invoke('player_action', { epoch, action: { kind: 'mode', mode: 'sequential' } });
    one('.episode-info strong', row(504)).dispatchEvent(new MouseEvent('dblclick', { bubbles: true, detail: 2 }));
    await wait(() => row(504).classList.contains('current') && player()?.dataset.state === 'playing' && Number(player().dataset.position) > 0.08, 'native audio progression');
    const playing = await playerSnapshot();
    await wait(() => Number(player().dataset.position) > playing.position + 0.08, 'native clock advances');
    const active = await queue();
    assert(active.tracks.map((entry) => entry.track.voiceId).join() === '503,504' && active.current === active.tracks[1].entryId, 'filtered original order and exact start');
    await stage('ux_03_valid_native', { voice: playing.track.voiceId, position: playing.position, queue: active.tracks.map((entry) => entry.track.voiceId) });

    step = 'explicit unavailable target retains playback';
    one('button[aria-label^="播放"]', row(502)).click();
    await wait(() => document.body.textContent.includes('该声音暂不可收听'), 'confirmed unavailable prompt');
    const after = await playerSnapshot();
    assert(after.playId === playing.playId && after.queueRevision === playing.queueRevision, 'missing target leaves existing native decoder and queue');
    await stage('ux_04_missing_retained', { voice: after.track.voiceId, playIdRetained: true });
    await stage('complete', { ok: true });
  } catch (error) {
    await stage('failed', { step, message: String(error), body: document.body.innerText.slice(-1800) });
  }
})();
