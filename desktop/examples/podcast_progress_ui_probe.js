// Production Vue gestures, typed IPC, native playback clock and sealed progress transport.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const buttons = (root = document) => [...root.querySelectorAll('button')];
  const byText = (text, root = document) => buttons(root).find((node) => node.textContent.trim() === text);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (condition, label) => { if (!condition) throw Error(label); };
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const recent = () => one('[data-podcast-recent]');
  const row = (id) => one(`[data-recent-voice="${id}"]`);
  const episodeButton = (id) => one('button.recent-play', row(id));
  const songButton = () => one('.track-row[data-track-id="1"] button.track-play');
  const visible = (node) => node && !node.disabled && node.getClientRects().length > 0;
  const click = (node, label) => {
    assert(visible(node), label);
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    node.click();
  };
  async function wait(predicate, label) {
    const end = performance.now() + 12000;
    while (performance.now() < end) {
      const value = await predicate();
      if (value) return value;
      await sleep(50);
    }
    throw Error(label);
  }
  let stageCount = 0;
  let poll = 0;
  let epoch;
  const player = () => invoke('player_snapshot', { epoch });
  async function count() {
    const nonce = ++poll;
    await emit('progress_poll', { nonce });
    const reply = await wait(() => window.__podcastProgressCount?.[0] === nonce && window.__podcastProgressCount,
      'native upload counter reply');
    return reply[1];
  }
  async function uploaded(expected) {
    await wait(async () => {
      const current = await count();
      assert(current <= expected, 'unexpected extra upload');
      return current === expected;
    }, `wait for ${expected} uploads`);
  }
  async function stableCount(expected) {
    // Give the native publisher and its asynchronous permission check time
    // to run while paused; the final stage also checks for a late extra send.
    await sleep(550);
    assert(await count() === expected, `expected ${expected} uploads while playback stayed put`);
  }
  async function capture(name, current, extra = {}) {
    const stage = `ux_${String(++stageCount).padStart(2, '0')}_${name}`;
    window.__podcastProgressStage = '';
    await emit(stage, { ...extra, actual: document.visibilityState === 'visible',
      player: current && { state: current.state, track: current.track?.id, voice: current.track?.voiceId,
        playId: current.playId, queueRevision: current.queueRevision, position: current.position,
        timelineMode: current.timeline?.mode, duration: current.timeline?.total },
    });
    await wait(() => window.__podcastProgressStage === stage, `saved ${stage}`);
  }
  async function openRecent() {
    click(await wait(() => visible(byText('我的播客')) && byText('我的播客'), 'podcast navigation'), 'podcast navigation');
    const tab = await wait(() => {
      const node = one('[data-podcast-recent-tab]');
      return visible(node) && node;
    }, 'podcast recent tab');
    if (!recent()) click(tab, 'podcast recent tab');
    await wait(() => visible(episodeButton('501')), 'first synthetic episode');
  }
  async function playEpisode(id, previous) {
    const button = await wait(() => visible(episodeButton(id)) && episodeButton(id), `episode ${id} visible`);
    click(button, `play episode ${id}`);
    return wait(async () => {
      const p = await player();
      return p.state === 'playing' && p.track?.voiceId === id && p.track?.id === String(1000 + Number(id)) &&
        p.playId !== previous && p.position > .15 && p.timeline?.mode === 'full' && p;
    }, `authorized full episode ${id} clock`);
  }
  async function pauseAndSeek(playing, seconds) {
    click(await wait(() => visible(one('.playerbar [aria-label="暂停"]')) && one('.playerbar [aria-label="暂停"]'),
      'native pause available'), 'pause actual playback');
    await wait(async () => (await player()).state === 'paused', 'native pause settles');
    const input = await wait(() => {
      const field = one('.playerbar input[aria-label="播放进度"]');
      return visible(field) && field;
    }, 'native seek slider available');
    input.value = String(seconds);
    input.dispatchEvent(new Event('input', { bubbles: true }));
    input.dispatchEvent(new Event('change', { bubbles: true }));
    return wait(async () => {
      const p = await player();
      return p.state === 'paused' && !p.nativeSeeking && p.playId === playing.playId &&
        p.queueRevision === playing.queueRevision && Math.abs(p.position - seconds) < .12 && p;
    }, 'native paused seek settles without changing identity');
  }
  try {
    await openRecent();
    ({ epoch } = await invoke('account_snapshot'));
    const first = await playEpisode('501');
    assert(await count() === 0, 'starting an episode is not a progress upload');
    await capture('started', first);
    const firstFrozen = await pauseAndSeek(first, 7.4);
    await stableCount(0);
    await capture('first_paused_seek', firstFrozen);

    const second = await playEpisode('503', first.playId);
    await uploaded(1);
    await capture('episode_departure', second);
    const secondFrozen = await pauseAndSeek(second, 12.4);
    await stableCount(1);
    await capture('second_paused_seek', secondFrozen);
    click(await wait(() => visible(byText('每日推荐')) && byText('每日推荐'), 'daily navigation'), 'daily navigation');
    await wait(() => visible(songButton()), 'ordinary first song available');
    assert(await count() === 1, 'leaving the podcast page does not report a still-selected episode');
    click(songButton(), 'start ordinary first song');
    const song = await wait(async () => {
      const p = await player();
      return p.state === 'playing' && p.track?.id === '1' && !p.track?.voiceId && p.position > .15 && p;
    }, 'ordinary playback has native clock');
    await uploaded(2);
    await capture('song_departure', song);

    // Recent single-row selection inserts after the currently selected song.
    // Empty the synthetic queue so EOF occurs without an unrelated next row.
    click(await wait(() => visible(one('.playerbar [aria-label="播放队列"]')) &&
      one('.playerbar [aria-label="播放队列"]'), 'queue toggle available'), 'open ordinary queue');
    click(await wait(() => visible(byText('清空')) && byText('清空'), 'queue clear available'), 'open queue clear');
    click(await wait(() => visible(byText('确认清空')) && byText('确认清空'), 'local queue clear confirmation'),
      'confirm local queue clear');
    await wait(async () => {
      const p = await player();
      return p.queueLen === 0 && p.state === 'idle' && !one('dialog[open]');
    }, 'synthetic queue cleared before EOF');
    click(await wait(() => visible(one('[aria-label="关闭播放队列"]')) &&
      one('[aria-label="关闭播放队列"]'), 'queue close available'), 'close ordinary queue');
    assert(await count() === 2, 'ordinary song and queue clear do not upload podcast progress');

    if ((await player()).mode !== 'sequential') {
      await invoke('player_action', { epoch, action: { kind: 'mode', mode: 'sequential' } });
      await wait(async () => (await player()).mode === 'sequential', 'sequential queue mode settles');
    }
    await openRecent();
    const last = await playEpisode('504', song.playId);
    assert(last.queueLen === 1, 'natural EOF has no unrelated following row');
    const nearEnd = await pauseAndSeek(last, 28.4);
    assert(await count() === 2, 'seek near end does not report completion');
    await capture('before_eof', nearEnd);
    click(await wait(() => visible(one('.playerbar [aria-label="播放"]')) && one('.playerbar [aria-label="播放"]'),
      'resume control available'), 'resume to natural EOF');
    const end = await wait(async () => {
      const p = await player();
      return p.state === 'ended' && p.track?.voiceId === '504' && p;
    }, 'native decoder natural EOF');
    await uploaded(3);
    await capture('natural_eof', end);

    click(await wait(() => visible(one('[aria-label="打开设置"]')) && one('[aria-label="打开设置"]'),
      'open settings available'), 'open settings');
    const checkbox = () => one('input[name="recent-history-sync"]');
    await wait(() => visible(checkbox()) && checkbox().checked, 'recent sync initially enabled');
    click(checkbox(), 'turn off this account’s local sync');
    const confirm = await wait(() => {
      const dialog = one('dialog[open]');
      return dialog && byText('确定关闭', dialog);
    }, 'first disable confirmation');
    click(confirm, 'confirm local-only history');
    await wait(() => visible(checkbox()) && !checkbox().checked && !one('dialog[open]'), 'local-only setting saved');
    await capture('opt_out', null, { syncEnabled: checkbox().checked });

    await openRecent();
    const privatePlay = await playEpisode('506', last.playId);
    const privateFrozen = await pauseAndSeek(privatePlay, 5.4);
    await stableCount(3);
    await capture('private_paused_seek', privateFrozen);
    click(await wait(() => visible(one('.playerbar [aria-label="停止播放"]')) && one('.playerbar [aria-label="停止播放"]'),
      'stop control available'), 'stop with local-only history');
    const stopped = await wait(async () => {
      const p = await player();
      return p.state === 'idle' && p;
    }, 'native stop settles');
    await stableCount(3);
    const history = (await invoke('history_page', { epoch, kind: 'voice', refresh: false })).value;
    const saved = history.entries.find((entry) => entry.id === '506' && entry.origin === 'local');
    assert(!history.syncEnabled && saved && Math.abs(saved.playProgress - privateFrozen.position) < .12,
      'local-only history still records this episode and its clock');
    await capture('private_stopped', stopped, {
      localVoice: saved.id, localPosition: saved.playProgress, syncEnabled: history.syncEnabled,
    });
    await emit('complete', { ok: true, stageCount, realMusicRequests: 0, remoteWrites: 0 });
  } catch (error) {
    await emit('failed', { ok: false, error: String(error), stack: error?.stack, stageCount,
      player: epoch ? await player().catch(() => null) : null });
  }
})();
