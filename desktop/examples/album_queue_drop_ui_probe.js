// Production album route, typed queue drop, and libmpv null output with sealed fixtures.
(async () => {
  const one = selector => document.querySelector(selector);
  const all = selector => [...document.querySelectorAll(selector)];
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, data = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...data } });
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const check = (ok, label) => { if (!ok) throw Error(label); };
  async function wait(read, label) {
    for (let i = 0; i < 220; i++) {
      const value = await read();
      if (value) return value;
      await sleep(60);
    }
    throw Error(label);
  }
  const bar = () => one('.playerbar');
  const button = (label, root = document) => [...root.querySelectorAll('button')].find(
    node => node.getAttribute('aria-label') === label || node.textContent.trim() === label,
  );
  const click = node => {
    check(node && !node.disabled && !node.closest('[inert]'), 'usable control');
    node.focus({ preventScroll: true });
    node.click();
  };
  const scroll = () => one('.queue-scroll');
  const marker = () => one('.queue-drop-marker');
  const markerReady = () => marker()?.style.top === '64px' &&
    one('.queue-drag-hint')?.textContent.includes('正在添加 2 首歌曲');
  const songsPanel = '[data-album-panel="songs"]:not([inert])';
  const rowSelector = id => songsPanel + ' .track-row[data-track-id="' + id + '"]';
  const source = id => one(rowSelector(id) + ' .track-row-title-text');
  const selected = () => all(songsPanel + ' .track-row[aria-selected="true"]').map(row => row.dataset.trackId);
  const visibleSongs = () => all(songsPanel + ' .track-row').map(row => row.dataset.trackId).join(',');
  const queueIds = rows => rows.tracks.map(row => row.track.id).join(',');
  const keyword = () => one('[aria-label="搜索专辑歌曲"]');
  function filter(value) {
    const input = keyword();
    input.value = value;
    input.dispatchEvent(new InputEvent('input', { bubbles: true }));
  }
  const settled = () => !one('.page-enter-active,.page-leave-active,.catalog-tab-enter-active,.catalog-tab-leave-active,.queue-panel-enter-active');
  let epoch, phase = 0, last = 'startup';
  const snapshot = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  const action = action => invoke('player_action', { epoch, action });
  const trusted = window.__cumulusAlbumDropTrusted === true;
  const dropCalls = [];
  async function capture(label, extra = {}) {
    last = label;
    await wait(settled, 'page and panels settled before ' + label);
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const [player, rows] = await Promise.all([snapshot(), queue()]);
    check(player.queueRevision === rows.revision && document.visibilityState === 'visible' &&
      document.documentElement.scrollWidth <= innerWidth, 'settled native queue and visible contained WebView');
    const stage = `ux_${String(++phase).padStart(2, '0')}_${label}`;
    window.__CUMULUS_ALBUM_CAPTURE_ACK__ = '';
    await emit(stage, { checkedPlayer: player, checkedQueue: rows, selected: selected(),
      ratio: devicePixelRatio, screenX, screenY, ...extra });
    await wait(() => window.__CUMULUS_ALBUM_CAPTURE_ACK__ === stage, 'capture acknowledged ' + stage);
  }
  function pointOn(node) {
    const rect = node.getBoundingClientRect();
    const point = { x: rect.left + Math.min(8, rect.width / 2), y: rect.top + rect.height / 2 };
    check(node.contains(document.elementFromPoint(point.x, point.y)), 'visible album source is hit-tested');
    return point;
  }
  function points(id) {
    const target = scroll();
    target.scrollTop = 0;
    const viewport = target.getBoundingClientRect();
    const to = { x: viewport.left + viewport.width / 2, y: viewport.top + 70 };
    check(target.contains(document.elementFromPoint(to.x, to.y)), 'queue boundary is hit-tested');
    return { from: pointOn(source(id)), to };
  }
  function pointer(node, type, point, mods = {}) {
    node.dispatchEvent(new PointerEvent(type, {
      bubbles: true, cancelable: true, pointerId: 57, pointerType: 'mouse', button: 0,
      buttons: type === 'pointerup' ? 0 : 1, clientX: point.x, clientY: point.y, ...mods,
    }));
  }
  function pick(id, mods = {}) {
    const node = source(id), point = pointOn(node);
    pointer(node, 'pointerdown', point, mods);
    pointer(node, 'pointerup', point, mods);
    node.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, ...mods }));
  }
  function simulate(id, { from, to }) {
    const target = scroll();
    pointer(source(id), 'pointerdown', from);
    pointer(target, 'pointermove', to);
    return () => pointer(target.isConnected ? target : document.body, 'pointerup', to);
  }
  function retained(before, after) {
    check(after.state === 'paused' && after.playId === before.playId &&
      after.entryId === before.entryId && after.track?.id === before.track?.id &&
      after.position === before.position && !after.error, 'exact paused playback remains unchanged');
  }

  try {
    await wait(() => one('.track-credit-links button') && button('列表循环', bar() ?? document)?.disabled === false,
      'signed-in daily route and native player ready');
    await emit('resize', { width: 1120, height: 760 });
    await wait(() => innerWidth === 1120 && innerHeight === 760, 'wide viewport');
    ({ epoch } = await invoke('account_snapshot'));
    await invoke('play_selection', {
      epoch, selection: { kind: 'songs', ids: ['701', '702'], index: 0, quality: 'standard' },
    });
    await wait(async () => {
      const view = await snapshot();
      return view.state === 'playing' && view.position > .2;
    }, 'actual synthetic PCM decoded through libmpv');
    await action({ kind: 'pause', paused: true });
    await action({ kind: 'seek', seconds: 10 });
    await wait(async () => {
      const view = await snapshot();
      return view.state === 'paused' && Math.abs(view.position - 10) < .2 && !view.nativeSeeking;
    }, 'paused native seek settled');
    click(one('.track-credit-links button'));
    await wait(() => one('.catalog-heading h1')?.textContent === '测试歌手' && settled(), 'artist route');
    click(one('[data-catalog-tab="albums"]'));
    click(await wait(() => settled() && one('[data-artist-panel="albums"] .catalog-card'), 'first album card'));
    await wait(() => one('.album-header h1')?.textContent === '测试专辑' && source('2') && settled(), 'album 7');
    filter('合成歌曲 1');
    await wait(() => visibleSongs() === '1,10,11', 'filter exposes noncontiguous original indices');
    click(button('播放队列', bar()));
    await wait(() => scroll()?.querySelectorAll('.queue-row').length === 2 && settled(), 'ordinary queue rendered');
    const initial = await queue();
    const before = await snapshot();
    check(queueIds(initial) === '701,702' && before.playId && before.entryId, 'native paused baseline');

    const provides = one('#app').__vue_app__._context.provides;
    const core = provides[Reflect.ownKeys(provides)
      .find(key => typeof key === 'symbol' && key.description === 'core-client')];
    const realAction = core.playerAction;
    core.playerAction = (owner, input) => {
      if (input.kind === 'drop_album_songs_into_queue') dropCalls.push(input);
      return realAction(owner, input);
    };
    await capture('paused_baseline');
    pick('11');
    pick('10', { ctrlKey: true });
    await wait(() => selected().join(',') === '10,11', 'reverse selected rows visible');
    const grid = source('10').closest('[role="grid"]');
    check(grid.getAttribute('aria-multiselectable') === 'true' && grid.getAttribute('aria-label') === '专辑歌曲',
      'accessible album multi-selection grid');
    await capture('reverse_selection');

    const drag = points('10');
    let nativeDown = false, nativeUp = false;
    if (trusted) {
      document.addEventListener('pointerdown', event => {
        if (event.isTrusted && source('10')?.contains(event.target)) nativeDown = true;
      }, { capture: true, once: true });
      document.addEventListener('pointerup', event => {
        if (event.isTrusted && scroll()?.contains(event.target)) nativeUp = true;
      }, { capture: true, once: true });
    }
    const absolute = ({ x, y }) => ({
      x: Math.floor((screenX + x) * devicePixelRatio),
      y: Math.floor((screenY + y) * devicePixelRatio),
    });
    await capture('ordered_drag_target', { pointer: { from: absolute(drag.from), to: absolute(drag.to) } });
    const release = trusted ? null : simulate('10', drag);
    await wait(markerReady, 'two-song marker at the exact queue boundary');
    check(!trusted || nativeDown, 'WebView receives trusted album pointerdown');
    const line = marker(), box = line.getBoundingClientRect();
    await capture('insertion_boundary', {
      paintSamples: [{ x: box.left + 8, y: box.top + 1, color: getComputedStyle(line).backgroundColor }],
    });
    release?.();
    const after = await wait(async () => {
      const rows = await queue();
      return queueIds(rows) === '701,11,10,702' && rows;
    }, 'drop inserts in selection order at the exact boundary');
    check(!trusted || nativeUp, 'WebView receives trusted queue pointerup');
    const typed = dropCalls[0];
    check(dropCalls.length === 1 && typed.id === '7' && /^[a-f0-9]{64}$/.test(typed.revision) &&
      JSON.stringify(typed.rows) === JSON.stringify([{ track_id: '11', track_index: 10 }, { track_id: '10', track_index: 9 }]) &&
      typed.before_entry_id === initial.tracks[1].entryId && typed.expected_play_id === before.playId &&
      typed.expected_queue_revision === initial.revision, 'typed drop uses immutable source order and native fences');
    check(after.revision !== initial.revision, 'native commit advances the queue revision once');
    retained(before, await snapshot());
    await wait(() => scroll()?.querySelectorAll('.queue-row').length === 4 && !marker(), 'updated queue rendered');
    await capture('ordered_drop', { typed });

    async function noNewDrop(release) {
      await wait(() => !marker() && !one('.queue-drag-hint'), 'pending drag marker cleared');
      release();
      check((await queue()).revision === after.revision && dropCalls.length === 1, 'cancelled drag sends no queue mutation');
      retained(before, await snapshot());
    }
    let old = simulate('10', points('10'));
    await wait(markerReady, 'escape drag is active');
    document.activeElement.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    await noNewDrop(old);
    await capture('escape_cancelled');

    old = simulate('10', points('10'));
    await wait(markerReady, 'filter-change drag is active');
    filter('合成歌曲 10');
    await wait(() => visibleSongs() === '10', 'visible album rows change during drag');
    await noNewDrop(old);
    await capture('filter_changed');
    filter('合成歌曲 1');
    await wait(() => visibleSongs() === '1,10,11', 'restored album filter');
    pick('11');
    pick('10', { ctrlKey: true });
    old = simulate('10', points('10'));
    await wait(markerReady, 'tab-switch drag is active');
    click(one('.album-tabs [id$="-details"]'));
    await wait(() => one('[data-album-panel="details"]') && settled(), 'album tab left');
    await noNewDrop(old);
    await capture('tab_left');
    await emit('complete', { ok: true, stageCount: phase, actualNativePlayback: true,
      trustedPointer: trusted, dropCommands: dropCalls.length, realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', { ok: false, last, error: String(error?.stack ?? error) });
  }
})();
