// Actual desktop WebView, native queue and journal, synthetic account/media/server.
(async () => {
  const one = (selector, root = document) => root.querySelector(selector);
  const all = selector => [...document.querySelectorAll(selector)];
  const invoke = (...args) =>
    (window.__CUMULUS_PROOF_INVOKE__ ?? window.__TAURI_INTERNALS__.invoke)(...args);
  const emit = (stage, payload = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...payload },
  });
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const bar = () => one('.playerbar');
  const panel = () => one('.queue-panel');
  const picker = () => one('.playlist-collect-dialog[open]:not(.modal-leave-active)');
  const rows = () => all('.queue-row');
  const collect = index => one('[data-queue-collect-row]', rows()[index]);
  const song = index => one('.queue-song', rows()[index]);
  const selected = () => rows().flatMap((row, index) => row.classList.contains('selected') ? [index + 1] : []);
  const button = (label, root = document) => one(`button[aria-label="${label}"]`, root);
  const assert = (value, message) => { if (!value) throw Error(message); };
  const equals = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const wait = async (condition, message) => {
    for (let i = 0; i < 300; i++) {
      if (await condition()) return;
      await sleep(50);
    }
    throw Error(message);
  };
  const click = control => {
    assert(control && !control.disabled, 'enabled control');
    control.focus({ preventScroll: true });
    control.click();
  };
  const stablePaint = async () => {
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await sleep(130);
  };
  const target = control => {
    const rect = control.getBoundingClientRect();
    const x = (rect.left + rect.right) / 2, y = (rect.top + rect.bottom) / 2;
    const hit = document.elementFromPoint(x, y);
    assert(hit === control || control.contains(hit), 'collect target receives pointer input');
    return { x, y, width: innerWidth, height: innerHeight, ratio: devicePixelRatio,
      webKitScreenX: window.screenX, webKitScreenY: window.screenY };
  };
  const order = [];
  const gestures = [];
  const trusted = window.__queueRowCollectionTrusted === true;
  let count = 0, stage = 'startup', picked, bulkReads = 0, playbackId;
  async function capture(label, extras = () => ({})) {
    stage = label;
    await wait(() => !one('.queue-panel-enter-active,.modal-enter-active'), 'entry motion settled');
    await stablePaint();
    const receipt = `ux_${String(++count).padStart(2, '0')}_${label}`;
    window.__queueUiStageRecorded = '';
    await emit(receipt, {
      selected: selected(), gestures: [...gestures], picked: picked ?? null, bulkReads,
      focus: document.activeElement?.getAttribute('aria-label'),
      focusInPicker: !!picker()?.contains(document.activeElement), ...extras(),
    });
    await wait(() => window.__queueUiStageRecorded === receipt, 'host captured exact native queue stage');
  }
  async function openQueue(length) {
    click(button('播放队列'));
    await wait(() => rows().length === length && !one('.queue-collect').disabled, 'queue ready');
  }
  async function showPicker(ids) {
    await wait(() => picker() && one('.collect-row[data-playlist-id="2"]', picker())
      && !one('.collect-load', picker()), 'native collection picker ready');
    assert(equals(picked?.ids, ids), `native exact selection ${JSON.stringify(picked)}`);
    assert(one('h2', picker()).textContent.trim() === '收藏到歌单'
      && !one('.collect-selection-summary', picker()), 'inline picker shows no whole-queue summary');
    assert(picker().contains(document.activeElement), 'picker initially owns focus');
  }
  async function closed(trigger) {
    await wait(() => !one('.playlist-collect-dialog') && document.activeElement === trigger,
      'picker returns focus to actual row button');
  }
  async function group() {
    click(song(2));
    song(0).dispatchEvent(new MouseEvent('click', { bubbles: true, ctrlKey: true }));
    await wait(() => equals(selected(), [1, 3]), 'reverse ordered selection visible');
  }
  async function closeQueue() {
    click(button('关闭播放队列', panel()));
    await wait(() => !panel(), 'queue closed');
  }
  async function pauseNewSource(client, epoch, selection, resourceKind) {
    const old = bar().dataset.playId;
    await client.playSelection(epoch, selection);
    await wait(async () => {
      const state = await client.playerSnapshot(epoch);
      return state.resourceKind === resourceKind && state.playId !== old && state.state === 'playing'
        && state.position > .12;
    }, 'native playback advances on ' + resourceKind);
    click(button('暂停', bar()));
    let last;
    await wait(async () => {
      const current = await client.playerSnapshot(epoch);
      const paused = current.state === 'paused' && current.resourceKind === resourceKind;
      const settled = paused && current.position === last;
      last = paused ? current.position : undefined;
      return settled;
    }, 'new source paused at a settled native clock');
  }
  try {
    await wait(() => one('.track-row') && button('列表循环') && !button('列表循环').disabled,
      'authenticated initial page');
    const provides = one('#app').__vue_app__._context.provides;
    const client = provides[Reflect.ownKeys(provides).find(key => key.description === 'core-client')];
    const epoch = (await client.accountSnapshot()).epoch;
    const nativeRead = client.playerQueueCollectRows;
    const nativeBulk = client.playerQueueCollection;
    client.playerQueueCollectRows = async (...args) => {
      const result = await nativeRead(...args);
      picked = { ids: result.tracks.map(track => track.id), total: result.total, revision: result.revision,
        entryIds: [...args[2]], epoch: result.epoch };
      order.push(picked);
      return result;
    };
    client.playerQueueCollection = (...args) => { bulkReads++; return nativeBulk(...args); };
    await client.playSelection(epoch, { kind: 'songs', ids: ['1', '2', '3', '4'],
      index: 0, quality: 'standard' });
    await wait(() => bar().dataset.state === 'playing' && Number(bar().dataset.position) > .2,
      'actual native playback');
    click(button('暂停', bar()));
    await wait(() => bar().dataset.state === 'paused', 'native paused');
    const seek = one('[aria-label="播放进度"]');
    seek.value = '10';
    seek.dispatchEvent(new Event('change', { bubbles: true }));
    await wait(() => !seek.disabled && Math.abs(Number(bar().dataset.position) - 10) < .05,
      'paused native seek settled');
    playbackId = bar().dataset.playId;
    let previous;
    await wait(async () => {
      const sample = await client.playerSnapshot(epoch);
      const matching = sample.state === 'paused' && sample.playId === playbackId
        && Math.abs(sample.position - 10) < .05;
      const settled = matching && sample.position === previous;
      previous = matching ? sample.position : undefined;
      return settled;
    }, 'native paused clock repeats the same settled position');
    await openQueue(4);
    for (const [index, name] of [[1, 'pointer'], [3, 'keyboard']]) {
      collect(index).addEventListener('click', event => gestures.push({
        kind: name, trusted: event.isTrusted, detail: event.detail,
      }));
    }
    await group();
    const pointerButton = collect(1);
    pointerButton.focus({ preventScroll: true });
    await capture('pointer_ready', () => ({ pointer: target(pointerButton) }));
    if (!trusted) pointerButton.dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 1 }));
    await showPicker(['2']);
    assert(equals(selected(), [2]) && gestures[0]?.detail > 0, 'pointer collapses selection to clicked row');
    await capture('pointer_picker');
    click(button('关闭歌单操作', picker()));
    await closed(pointerButton);
    await capture('pointer_cancelled');

    await group();
    const keyButton = collect(3);
    keyButton.focus({ preventScroll: true });
    assert(document.activeElement === keyButton, 'keyboard target focused');
    await capture('keyboard_ready');
    if (!trusted) keyButton.click();
    await showPicker(['3', '1']);
    assert(equals(selected(), [1, 3]) && gestures[1]?.detail === 0, 'keyboard preserves live group');
    await capture('keyboard_picker');
    click(one('.collect-row[data-playlist-id="2"]', picker()));
    await closed(keyButton);
    const readback = (await client.playlistEditor(epoch, '2')).value.trackIds;
    assert(equals(readback, ['3', '1']), 'destination readback has precisely selected songs in order');
    assert(bar().dataset.playId === playbackId && bar().dataset.state === 'paused', 'same loaded song');
    await capture('saved', () => ({ readback }));

    await closeQueue();
    const scope = { kind: 'radio', id: '77', ascending: false };
    const page = await client.podcastPage(epoch, { scope, offset: 0, limit: 50, keyword: '', revision: null }, true);
    await client.playSelection(epoch, { kind: 'podcast', scope, start_id: null, revision: page.revision });
    await wait(() => bar().dataset.state === 'playing' && bar().dataset.playId !== playbackId, 'actual episode playback');
    click(button('暂停', bar()));
    await wait(() => bar().dataset.state === 'paused', 'episode paused');
    await openQueue(2);
    assert(!all('[data-queue-collect-row]').length && !one('.queue-collect').disabled,
      'voice has no inline collect; separate whole-queue action remains');
    await capture('voice_excluded');
    await closeQueue();

    stage = 'seed_download_and_local';
    await emit('download-page-fixture', { mode: 'seed_queue_collection' });
    await wait(() => window.__downloadPageAck, 'one native download and two real local files seeded');
    assert(window.__downloadPageAck.ok, window.__downloadPageAck.message);
    const downloads = { tab: 'completed', category: 'song', keyword: '', sort: 'completed',
      descending: true, offset: 0, limit: 10 };
    const published = (await client.downloads(epoch, downloads)).tasks;
    assert(published.length === 1 && published[0].status === 'complete' && published[0].track.id === '7',
      'completed task belongs to the fixture account and exact song');
    stage = 'start_download';
    await pauseNewSource(client, epoch, { kind: 'downloads', query: downloads, start_id: published[0].id }, 'downloaded');
    await openQueue(1);
    const offlineButton = collect(0);
    assert(offlineButton && !one('.queue-row-more', rows()[0]), 'download only uses the proven inline action');
    click(offlineButton);
    await showPicker(['7']);
    await capture('download_picker');
    click(button('关闭歌单操作', picker()));
    await closed(offlineButton);
    await capture('download_cancelled');
    await closeQueue();

    stage = 'start_local';
    const query = { keyword: '', sort: 'title', descending: false, group: 'list',
      groupKey: null, includeMissing: false, offset: 0, limit: 10, revision: null };
    const local = await client.localPage(query);
    const indexed = ['linked-queue-song.wav', 'unlinked-queue-song.wav'].map(name =>
      local.tracks.find(row => row.fileName === name)?.track.id);
    assert(local.tracks.length === 2 && indexed.every(Boolean), 'both native indexed files are present');
    await pauseNewSource(client, epoch, { kind: 'local', query: { ...query, revision: local.revision },
      start_id: indexed[0], revision: local.revision }, 'local');
    await openQueue(2);
    const displayed = await client.playerQueue(epoch);
    const localIndex = indexed.map(id => displayed.tracks.findIndex(row => row.track.id === id));
    assert(localIndex.every(i => i >= 0 && displayed.tracks[i].localMenu &&
      !displayed.tracks[i].catalogMenu && !displayed.tracks[i].voiceMenu && one('.queue-row-more', rows()[i])),
      'local rows have their own menu without gaining a catalog or voice menu');
    const mapped = collect(localIndex[0]), unmatched = collect(localIndex[1]);
    click(mapped);
    await showPicker(['8']);
    await capture('local_picker');
    click(button('关闭歌单操作', picker()));
    await closed(mapped);
    await capture('local_cancelled');
    click(unmatched);
    await wait(() => !picker() && one('.queue-notice[role="alert"]')?.textContent.includes('尚未关联有效的线上歌曲'),
      'unmatched file is explained instead of opened or uploaded');
    await capture('local_unmatched', () => ({ unmatchedRejected: true }));
    assert(bulkReads === 0 && order.length === 4 && gestures.length === 2
      && gestures.every(gesture => gesture.trusted === trusted), 'exact invocation and input origins');
    await emit('complete', { ok: true, stageCount: count, order, bulkReads, gestures,
      realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', { ok: false, phase: stage, error: String(error), order, bulkReads, gestures,
      selection: selected(), player: bar() && { ...bar().dataset }, body: document.body.innerText.slice(-1300),
      active: document.activeElement?.outerHTML.slice(0, 450) });
  }
})();
