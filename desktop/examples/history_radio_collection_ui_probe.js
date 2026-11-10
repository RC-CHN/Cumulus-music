// Real Recently Played collector controls over a sealed account and null decoder.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])];
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, payload = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...payload } });
  const assert = (condition, message) => { if (!condition) throw Error(message); };
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  let step = 'startup', stageCount = 0, epoch, paused;
  async function wait(observe, label) {
    step = label;
    const deadline = performance.now() + 15_000;
    do {
      const value = await observe();
      if (value) return value;
      await sleep(60);
    } while (performance.now() < deadline);
    throw Error(label);
  }
  const ready = node => node?.isConnected && node.getClientRects().length &&
    !node.matches(':disabled,[aria-disabled="true"]') && !node.closest('[inert]');
  function click(node) {
    assert(ready(node), 'enabled control for ' + step);
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    node.click();
  }
  const tabs = '[aria-label="播放记录类型"]';
  const settled = () => one('.history-page')?.getAttribute('aria-busy') === 'false' &&
    !one('.history-content:is(.catalog-tab-enter-active,.catalog-tab-leave-active)');
  const button = id => one(`[data-history-radio-collect="${id}"]`);
  const confirm = () => one('dialog[data-voice-podcast-uncollect][open]:not([inert])');
  const nativePlayer = () => invoke('player_snapshot', { epoch });
  const action = action => invoke('player_action', { epoch, action });
  const history = kind => invoke('history_page', { epoch, kind, refresh: false });
  const tuple = state => ({
    state: state.state, playId: state.playId, entryId: state.entryId,
    queueRevision: state.queueRevision, position: state.position, voiceId: state.track?.voiceId,
    mediaId: state.track?.id,
  });
  async function collection(id, title) {
    return wait(() => {
      const control = button(id);
      return settled() && ready(control) && control.title === title && control;
    }, `collector ${id} becomes ${title}`);
  }
  const saved = new Map();
  async function enter(kind, tab, id, title) {
    click(all(`${tabs} button`).find(node => node.textContent.trim() === tab));
    const control = await collection(id, '收藏');
    assert(one(`${tabs} [aria-current]`)?.textContent.trim() === tab, 'right history category');
    assert(one('.history-name', control.closest('[role="row"]'))?.textContent.trim() === title,
      'collector belongs to the displayed title');
    const data = (await history(kind)).value;
    const row = data.entries.find(row => row.id === id);
    assert(data.kind === kind && data.remoteIncluded && row?.canCollectRadio && row.chapter?.playable,
      'eligible native row retains its chapter');
    saved.set(kind, data);
    return control;
  }
  async function capture(name, id, kind) {
    step = name;
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const target = button(id);
    assert(target, 'same row keeps collection control');
    const row = target.closest('[role="row"]');
    const chapter = one('.history-chapter button', row);
    assert(chapter && ready(chapter), 'chapter stays playable');
    const snapshot = (await history(kind)).value;
    const origin = saved.get(kind);
    assert(snapshot.revision === origin.revision &&
      JSON.stringify(snapshot.entries) === JSON.stringify(origin.entries), 'collection altered history rows');
    const playback = tuple(await nativePlayer());
    assert(JSON.stringify(playback) === JSON.stringify(paused), 'collection changed paused native playback');
    const bounds = target.getBoundingClientRect();
    assert(bounds.width > 0 && bounds.height > 0 && bounds.right <= innerWidth &&
      document.documentElement.scrollWidth <= innerWidth, 'visible control in bounded history table');
    const dialog = confirm();
    await emit(`ux_${String(++stageCount).padStart(2, '0')}_${name}`, {
      kind, id, chapter: chapter.textContent.trim(), title: target.title,
      pressed: target.getAttribute('aria-pressed'), focus: document.activeElement === target,
      dialog: dialog ? {
        title: one('h2', dialog).textContent.trim(),
        focus: document.activeElement?.textContent.trim(),
        actions: all('button', dialog).map(node => node.textContent.trim()),
      } : null,
      notice: all('.history-page [role="status"]').map(node => node.textContent.trim()).join(' '),
      playback, checkPausedPlayback: true,
    });
  }

  try {
    epoch = await wait(async () => {
      const account = await invoke('account_snapshot');
      return account.status === 'authenticated' && account.epoch;
    }, 'synthetic account authenticated');
    click(await wait(() => all('.sidebar button').find(node =>
      node.textContent.trim() === '最近播放' && ready(node)), 'history entry'));
    await wait(() => one(tabs), 'history categories mounted');
    await action({ kind: 'mode', mode: 'single_loop' });
    await enter('audiobook', '有声书', '82', '合成历史有声书');
    const playbackRow = all('.history-row').find(row =>
      one('.history-name', row)?.textContent.trim() === '独立播放有声书');
    click(one('.history-chapter button', playbackRow));
    await wait(async () => {
      const state = await nativePlayer();
      return state.state === 'playing' && state.track?.voiceId === '842' &&
        state.track?.id === '1842' && state.position > .25;
    }, 'exact history chapter drives native PCM');
    await action({ kind: 'pause', paused: true });
    paused = await wait(async () => {
      const state = await nativePlayer();
      return state.state === 'paused' && state.position > .25 && tuple(state);
    }, 'chapter paused at a real native position');
    await enter('podcast', '播客', '81', '合成历史播客');
    const own = saved.get('podcast').entries.find(row => row.id === '83');
    assert(own && !own.canCollectRadio && !button('83'), 'own podcast has no collection button');
    await capture('podcast_initial', '81', 'podcast');
    click(await collection('81', '收藏'));
    await collection('81', '取消收藏');
    await capture('podcast_collected', '81', 'podcast');

    click(button('81'));
    await wait(() => confirm() && document.activeElement?.textContent.trim() === '取消',
      'cancel initially focused in unsubscribe confirmation');
    await capture('confirmation', '81', 'podcast');
    click(one('.secondary', confirm()));
    await wait(() => !one('dialog[data-voice-podcast-uncollect]') && document.activeElement === button('81'),
      'cancel restores the exact collection button');
    await capture('cancelled', '81', 'podcast');

    click(button('81'));
    await wait(confirm, 'reopened unsubscribe confirmation');
    click(one('.primary', confirm()));
    await collection('81', '核对收藏');
    await wait(() => !one('dialog[data-voice-podcast-uncollect]'), 'confirmation closed after explicit action');
    await capture('lost_ack', '81', 'podcast');
    click(button('81'));
    await collection('81', '收藏');
    await capture('reconciled', '81', 'podcast');

    await enter('audiobook', '有声书', '82', '合成历史有声书');
    await capture('book_initial', '82', 'audiobook');
    click(button('82'));
    await collection('82', '取消收藏');
    await capture('book_collected', '82', 'audiobook');
    await emit('complete', { ok: true, stageCount, checkPausedPlayback: true });
  } catch (error) {
    await emit('failed', { ok: false, step, error: String(error?.message ?? error) });
  }
})();
