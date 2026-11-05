// Synthetic results; production WebView, queue IPC and native null-output playback.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (value, message) => { if (!value) throw Error(message); };
  const wait = async (read, message) => {
    for (let attempt = 0; attempt < 180; attempt++) {
      const value = await read();
      if (value) return value;
      await sleep(60);
    }
    throw Error(message);
  };
  const invoke = (...args) =>
    (window.__CUMULUS_PROOF_INVOKE__ ?? window.__TAURI_INTERNALS__.invoke)(...args);
  const emit = (stage, info = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...info } });
  const button = (label, root = document) => [...root.querySelectorAll('button')]
    .find((node) => node.getAttribute('aria-label') === label || node.textContent.trim() === label);
  const click = (node) => {
    assert(node && !node.disabled, 'control is enabled');
    node.focus({ preventScroll: true });
    node.click();
  };
  const bar = () => one('.playerbar');
  const scroll = () => one('.queue-scroll');
  const marker = () => one('.queue-drop-marker');
  const source = (id) => one(`.search-song-panel .track-row[data-track-id="${id}"] .track-row-title-text`);
  const selected = () => all('.search-song-panel .track-row[aria-selected="true"]')
    .map((row) => row.dataset.trackId);
  const songs = (queue) => queue.tracks.map(({ track }) => track.id).join(',');
  const state = (epoch) => invoke('player_snapshot', { epoch });
  const queue = (epoch) => invoke('player_queue', { epoch });
  const markerReady = () => marker()?.style.top === '64px' &&
    one('.queue-drag-hint')?.textContent.includes('正在添加 2 首歌曲');
  const trusted = window.__cumulusSearchDropTrusted === true;
  let step = 'startup', count = 0, epoch;
  const dropCalls = [];

  async function capture(label, extra = {}) {
    step = label;
    assert(document.documentElement.scrollWidth <= innerWidth, 'page is contained');
    const [checkedPlayer, checkedQueue] = await Promise.all([state(epoch), queue(epoch)]);
    assert(checkedPlayer.queueRevision === checkedQueue.revision, 'consistent native revision');
    await emit(`ux_${String(++count).padStart(2, '0')}_${label}`, {
      checkedPlayer, checkedQueue, selected: selected(), ratio: devicePixelRatio,
      screenX, screenY, ...extra,
    });
  }
  function pointer(node, type, point, mods = {}) {
    node.dispatchEvent(new PointerEvent(type, {
      bubbles: true, cancelable: true, pointerId: 57, pointerType: 'mouse', button: 0,
      buttons: type === 'pointerup' ? 0 : 1, clientX: point.x, clientY: point.y, ...mods,
    }));
  }
  function pointOn(node) {
    const rect = node.getBoundingClientRect();
    const point = { x: rect.left + 5, y: rect.top + rect.height / 2 };
    assert(node.contains(document.elementFromPoint(point.x, point.y)), 'source is pointer reachable');
    return point;
  }
  function select(id, mods = {}) {
    const node = source(id), point = pointOn(node);
    pointer(node, 'pointerdown', point, mods);
    pointer(node, 'pointerup', point, mods);
    node.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, ...mods }));
  }
  function points(id) {
    const target = scroll();
    target.scrollTop = 0;
    const viewport = target.getBoundingClientRect();
    const to = { x: viewport.left + viewport.width / 2, y: viewport.top + 70 };
    assert(target.contains(document.elementFromPoint(to.x, to.y)), 'queue boundary is pointer reachable');
    return { from: pointOn(source(id)), to };
  }
  function simulate(id, { from, to }) {
    const target = scroll();
    pointer(source(id), 'pointerdown', from);
    pointer(target, 'pointermove', to);
    return () => pointer(target.isConnected ? target : document.body, 'pointerup', to);
  }
  function retained(before, after) {
    assert(after.state === 'paused' && after.playId === before.playId &&
      after.entryId === before.entryId && after.track?.id === before.track?.id &&
      Math.abs(after.position - before.position) < .05 && !after.error && !one('.playback-message'),
      'queue editing keeps the same paused native track, entry, play ID and position');
  }

  try {
    await wait(() => one('.track-row') && !button('播放', bar())?.disabled, 'signed-in daily page');
    await emit('resize', { width: 1120, height: 760 });
    await wait(() => innerWidth === 1120 && innerHeight === 760, 'wide viewport');
    ({ epoch } = await invoke('account_snapshot'));
    // Setup only: no drop is invoked directly by this probe.
    await invoke('play_selection', {
      epoch, selection: { kind: 'songs', ids: ['701', '702'], index: 0, quality: 'standard' },
    });
    await wait(() => bar().dataset.state === 'playing' && Number(bar().dataset.position) > .2,
      'baseline reached native output');
    click(button('暂停', bar()));
    await wait(() => bar().dataset.state === 'paused', 'baseline paused');
    const seek = one('[aria-label="播放进度"]');
    seek.value = '10';
    seek.dispatchEvent(new Event('change', { bubbles: true }));
    const before = await wait(async () => {
      const observed = await state(epoch);
      return observed.state === 'paused' && Math.abs(observed.position - 10) < .2 && observed;
    }, 'real native paused seek');

    window.__searchFixtureAck = false;
    await emit('search-fixture', { mode: 'song_waterfall' });
    await wait(() => window.__searchFixtureAck, 'signed-in synthetic search fixture');
    const search = one('[aria-label="搜索音乐"]');
    search.value = '合成连续单曲';
    search.dispatchEvent(new Event('input', { bubbles: true }));
    search.form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await wait(() => one('.search-results-page h1')?.textContent === '合成连续单曲' &&
      one('#search-result-panel')?.getAttribute('aria-busy') === 'false' &&
      !one('.page-enter-active,.page-leave-active'), 'initial search route');
    click(one('[data-category="songs"]'));
    await wait(() => one('[data-category="songs"]')?.getAttribute('aria-selected') === 'true' &&
      one('#search-result-panel')?.getAttribute('aria-busy') === 'false' &&
      source('101') && source('102'), 'ordinary song tab returned two distinct tracks');
    click(button('播放队列', bar()));
    await wait(() => scroll()?.querySelectorAll('.queue-row').length === 2 &&
      !one('.queue-panel-enter-active,.search-results-enter-active'), 'settled queue next to search');
    const initial = await queue(epoch);
    assert(songs(initial) === '701,702' && before.playId && before.entryId, 'exact native baseline');

    const provides = one('#app').__vue_app__._context.provides;
    const core = provides[Reflect.ownKeys(provides)
      .find((key) => typeof key === 'symbol' && key.description === 'core-client')];
    const action = core.playerAction;
    core.playerAction = (owner, input) => {
      if (input.kind === 'drop_search_songs_into_queue') dropCalls.push(input);
      return action(owner, input);
    };
    await capture('paused_baseline');
    select('102');
    select('101', { ctrlKey: true });
    await wait(() => selected().join(',') === '101,102', 'both reverse-selected rows are visible');
    assert(source('101').closest('[role="grid"]').getAttribute('aria-multiselectable') === 'true',
      'search grid exposes multiple selection');
    await capture('reverse_selection');

    const drag = points('101');
    let nativeDown = false, nativeUp = false;
    if (trusted) {
      document.addEventListener('pointerdown', (event) => {
        if (event.isTrusted && source('101')?.contains(event.target)) nativeDown = true;
      }, { capture: true, once: true });
      document.addEventListener('pointerup', (event) => {
        if (event.isTrusted && scroll()?.contains(event.target)) nativeUp = true;
      }, { capture: true, once: true });
    }
    const absolute = ({ x, y }) => ({
      x: Math.floor((screenX + x) * devicePixelRatio),
      y: Math.floor((screenY + y) * devicePixelRatio),
    });
    await capture('ordered_drag_target', { pointer: { from: absolute(drag.from), to: absolute(drag.to) } });
    const release = trusted ? null : simulate('101', drag);
    await wait(markerReady, 'two-song marker at the first native boundary');
    assert(!trusted || nativeDown, 'source received trusted pointer down');
    const line = marker(), rect = line.getBoundingClientRect();
    await capture('insertion_boundary', {
      paintSamples: [{ x: rect.left + 8, y: rect.top + 1, color: getComputedStyle(line).backgroundColor }],
    });
    release?.();
    const updated = await wait(async () => {
      const actual = await queue(epoch);
      return songs(actual) === '701,102,101,702' && actual;
    }, 'one drop uses selection order rather than displayed order');
    assert(!trusted || nativeUp, 'queue received trusted pointer up');
    assert(updated.revision !== initial.revision, 'successful drop advances the queue revision');
    assert(dropCalls.length === 1 &&
      dropCalls[0].rows.map((row) => row.track_id).join(',') === '102,101' &&
      dropCalls[0].rows.every((row) => /^[a-f0-9]{32}$/.test(row.receipt)) &&
      dropCalls[0].before_entry_id === initial.tracks[1].entryId &&
      dropCalls[0].expected_play_id === before.playId, 'one fenced typed drop with observed receipts');
    retained(before, await state(epoch));
    await wait(() => scroll()?.querySelectorAll('.queue-row').length === 4, 'visible updated queue');
    await capture('ordered_drop');

    // Cancellation paths hold the real insertion hint first, then release a
    // synthetic pointer over the formerly valid target without calling IPC.
    step = 'escape';
    const afterEscape = simulate('101', points('101'));
    await wait(markerReady, 'second drag active before Escape');
    document.activeElement.dispatchEvent(new KeyboardEvent('keydown', {
      key: 'Escape', bubbles: true, cancelable: true,
    }));
    await wait(() => !marker() && !one('.queue-drag-hint'), 'Escape removes the insertion marker');
    afterEscape();
    assert((await queue(epoch)).revision === updated.revision && dropCalls.length === 1,
      'releasing after Escape sends no drop');
    await capture('escape_cancelled');

    step = 'leave origin';
    const afterLeaving = simulate('101', points('101'));
    await wait(markerReady, 'third drag active before leaving search');
    click(button('每日推荐'));
    await wait(() => !one('.search-song-panel') && !marker() && !one('.queue-drag-hint') &&
      !one('.page-leave-active'), 'navigation detaches the drag origin and insertion marker');
    afterLeaving();
    assert((await queue(epoch)).revision === updated.revision && dropCalls.length === 1,
      'leaving the origin and releasing sends no drop');
    retained(before, await state(epoch));
    await capture('origin_left');

    await emit('complete', {
      ok: true, stageCount: count, actualNativePlayback: true, trustedPointer: trusted,
      dropCommands: dropCalls.length, realMusicRequests: 0, realAccountWrites: 0,
    });
  } catch (error) {
    await emit('failed', {
      ok: false, error: String(error), stack: error.stack, step, stageCount: count,
      dropCount: dropCalls.length, selected: selected(), hint: one('.queue-drag-hint')?.textContent,
      body: document.body.innerText.slice(0, 3200),
    });
  }
})();
