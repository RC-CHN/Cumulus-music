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
    assert(!one('[data-podcast-program-more]', row(501)) && one('[data-podcast-program-more]', row(504)), 'More belongs to normal rows only');
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

    const popup = () => one('[role="menu"][aria-label^="单集菜单"]');
    const more = (id) => one('[data-podcast-program-more]', row(id));
    const item = (label) => all('[role="menuitem"]', popup()).find((node) => node.textContent.trim() === label);
    const openMenu = async (id, keyboard = false) => {
      if (keyboard) {
        more(id).focus();
        more(id).dispatchEvent(new KeyboardEvent('keydown', { key:'F10', shiftKey:true, bubbles:true }));
      } else more(id).click();
      await wait(() => popup() && Number(getComputedStyle(popup()).opacity) > 0.98 && item('播放'), 'visible programme menu');
    };
    step = 'menu plays the single selected normal row';
    await invoke('player_action', { epoch, action: { kind: 'mode', mode: 'sequential' } });
    await openMenu(504, true);
    const labels = all('[role="menuitem"]', popup()).map((node) => node.textContent.trim());
    assert(labels.join('|') === '播放|下一首播放|查看评论(54)|下载|分享...|复制链接', 'official action order and exact count');
    await stage('ux_03_menu', { labels });
    item('播放').click();
    await wait(() => row(504).classList.contains('current') && player()?.dataset.state === 'playing' && Number(player().dataset.position) > 0.08, 'native menu playback');
    const first = await playerSnapshot();
    const firstQueue = await queue();
    assert(firstQueue.tracks.map((entry) => entry.track.voiceId).join() === '504', 'menu Play does not replace with the liked collection');
    await stage('ux_04_single_play', { voice:first.track.voiceId, queue:['504'] });

    step = 'next, share and download preserve paused playback';
    const paused = await invoke('player_action', { epoch, action: { kind: 'pause', paused:true } });
    await wait(() => player()?.dataset.state === 'paused', 'paused before next');
    await openMenu(503);
    item('下一首播放').click();
    await wait(() => one('[data-podcast-program-status]', pane())?.textContent.includes('已添加'), 'next acknowledged');
    const nextQueue = await queue(), afterNext = await playerSnapshot();
    assert(nextQueue.tracks.map((entry) => entry.track.voiceId).join() === '504,503' && afterNext.playId === paused.playId && afterNext.position === paused.position && afterNext.state === 'paused', 'next inserts one voice without reopening');
    await stage('ux_05_next', { queue:['504','503'], playIdRetained:true });
    await openMenu(503);
    item('分享...').click();
    const share = () => one('dialog[open] input[aria-label$="分享链接"]');
    await wait(share, 'public share dialog');
    assert(share().value === 'https://music.163.com/dj?id=503', 'public voice link only');
    await stage('ux_06_share', { link:share().value });
    one('[aria-label="关闭分享"]').click();
    await wait(() => !share() && document.activeElement === more(503), 'share closes back to More');
    await openMenu(503);
    item('下载').click();
    const dialog = () => one('dialog.download-dialog[open]');
    await wait(() => dialog()?.textContent.includes('普通') && !dialog().textContent.includes('正在读取'), 'scoped programme quality');
    await stage('ux_07_download', { target:503, beforeCancel:true });
    text('取消', dialog()).click();
    await wait(() => !dialog(), 'cancel scoped download');
    assert((await playerSnapshot()).playId === paused.playId, 'cancel leaves playback untouched');
    await openMenu(504);
    item('播放').click();
    await wait(() => player()?.dataset.state === 'playing', 'menu resumes current voice');
    assert((await playerSnapshot()).playId === paused.playId, 'resume keeps current native instance');

    step = 'double-click keeps the existing liked-list behaviour';
    const beforeDirect = await playerSnapshot();
    one('.episode-info strong', row(504)).dispatchEvent(new MouseEvent('dblclick', { bubbles: true, detail: 2 }));
    let playing;
    for (let i = 0; i < 240; i++) {
      const candidate = await playerSnapshot();
      if (candidate.queueRevision !== beforeDirect.queueRevision && candidate.track?.voiceId === '504' && candidate.state === 'playing' && candidate.position > 0.08) {
        playing = candidate;
        break;
      }
      await sleep(40);
    }
    assert(playing, 'new collection queue and native audio progression');
    await wait(() => Number(player().dataset.position) > playing.position + 0.08, 'native clock advances');
    const active = await queue();
    assert(active.tracks.map((entry) => entry.track.voiceId).join() === '503,504' && active.current === active.tracks[1].entryId, 'filtered original order and exact start');
    await stage('ux_08_valid_native', { voice: playing.track.voiceId, position: playing.position, queue: active.tracks.map((entry) => entry.track.voiceId) });

    step = 'explicit unavailable target retains playback';
    one('button[aria-label^="播放"]', row(502)).click();
    await wait(() => document.body.textContent.includes('该声音暂不可收听'), 'confirmed unavailable prompt');
    const after = await playerSnapshot();
    assert(after.playId === playing.playId && after.queueRevision === playing.queueRevision, `missing target leaves existing native decoder and queue: ${playing.queueRevision} → ${after.queueRevision}`);
    await stage('ux_09_missing_retained', { voice: after.track.voiceId, playIdRetained: true });
    await stage('complete', { ok: true });
  } catch (error) {
    await stage('failed', { step, message: String(error), body: document.body.innerText.slice(-1800) });
  }
})();
