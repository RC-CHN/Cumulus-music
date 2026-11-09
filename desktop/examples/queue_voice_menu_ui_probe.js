// Real queue row More and right click use the bound episode, not its backing song.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, extra = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...extra } });
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const frame = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const assert = (value, label) => { if (!value) throw Error(label); };
  async function wait(read, label) {
    for (let n = 0; n < 210; n++) {
      const value = await read();
      if (value) return value;
      await sleep(45);
    }
    throw Error(label);
  }
  const panel = () => one('.action-menu[aria-label="播放列表声音操作"]:not([inert])');
  const action = (key) => panel()?.querySelector(`[data-menu-key="${key}"]`);
  const text = (key) => action(key)?.textContent.trim();
  const row = (entryId) => one(`.queue-row[data-entry-id="${CSS.escape(entryId)}"]`);
  const trigger = (entryId) => row(entryId)?.querySelector('.queue-row-more');
  function pointer(node) {
    const r = node.getBoundingClientRect();
    const x = r.left + r.width / 2, y = r.top + r.height / 2;
    if (!node.contains(document.elementFromPoint(x, y))) return;
    return { x: Math.round((screenX + x) * devicePixelRatio),
      y: Math.round((screenY + y) * devicePixelRatio) };
  }
  async function open(entryId) {
    const button = await wait(() => trigger(entryId), 'current row More');
    await frame();
    button.focus({ preventScroll: true });
    button.click();
    await wait(() => action('play'), 'current queue menu opened');
    return button;
  }
  let epoch, count = 0, step = 'startup';
  const playback = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  const playing = (state) => async () => {
    const observed = await playback();
    return observed.state === state && observed.track?.voiceId === '401' &&
      one('.playerbar')?.dataset.state === state && observed;
  };
  async function capture(label, options = {}) {
    const paintSamples = [];
    let menu;
    if (options.menu) {
      menu = await wait(() => {
        const node = panel();
        return node && !node.classList.contains('menu-enter-active') &&
          getComputedStyle(node).opacity === '1' && node;
      }, 'menu animation completed');
      const r = menu.getBoundingClientRect();
      paintSamples.push({ x: r.left + 3, y: r.top + r.height / 2,
        color: getComputedStyle(menu).backgroundColor });
    }
    await frame();
    const hit = options.target && await wait(() => pointer(options.target), 'trusted input hits expected control');
    await emit(`ux_${String(++count).padStart(2, '0')}_${label}`, {
      playback: await playback(), queue: await queue(), pointer: hit,
      items: menu && Array.from(menu.querySelectorAll('[data-menu-key]'),
        (item) => [item.dataset.menuKey, item.textContent.trim()]),
      focused: options.focused, paintSamples, screenX, screenY, ratio: devicePixelRatio,
    });
  }
  const trusted = { pause: 0, play: 0, right: 0, escape: 0, remove: 0, directory: 0 };
  document.addEventListener('click', (event) => {
    if (!event.isTrusted || !(event.target instanceof Element)) return;
    const item = event.target.closest('.action-menu[aria-label="播放列表声音操作"] [data-menu-key]');
    if (item?.dataset.menuKey === 'play') trusted[item.textContent.trim() === '暂停' ? 'pause' : 'play']++;
    if (item?.dataset.menuKey === 'remove') trusted.remove++;
    if (item?.dataset.menuKey === 'directory') trusted.directory++;
  }, true);
  document.addEventListener('contextmenu', (event) => {
    if (event.isTrusted && event.target instanceof Element && event.target.closest('.queue-row')) trusted.right++;
  }, true);
  document.addEventListener('keydown', (event) => {
    if (event.isTrusted && event.key === 'Escape' && panel()?.contains(event.target)) trusted.escape++;
  }, true);
  try {
    await wait(() => one('.track-row') && !one('.page-enter-active,.page-leave-active'), 'synthetic Daily page');
    ({ epoch } = await invoke('account_snapshot'));
    window.__searchFixtureAck = false;
    await emit('search-fixture', { mode: 'voice_next' });
    await wait(() => window.__searchFixtureAck, 'sealed voice transport fixture');
    await invoke('player_action', { epoch, action: { kind: 'mode', mode: 'single_loop' } });
    await invoke('play_selection', { epoch, selection: { kind: 'voice', id: '401' } });
    const loaded = await wait(playing('playing'), 'original native voice starts');
    await invoke('player_action', { epoch, action: { kind: 'queue_voice', id: '402',
      expected_queue_revision: loaded.queueRevision } });
    const initial = await queue();
    const [a, b] = initial.tracks.map(({ entryId }) => entryId);
    assert(initial.tracks.map(({ track }) => track.voiceId).join() === '401,402' &&
      initial.tracks.every(({ voiceMenu, catalogMenu }) => voiceMenu && !catalogMenu), 'native queue voice entries');
    one('[aria-label="播放队列"]').click();
    await wait(() => trigger(a) && trigger(b) && !one('.queue-panel-enter-active'), 'queue rows are mounted');
    const same = (view) => view.playId === loaded.playId && view.entryId === a;

    step = 'pause-from-more';
    const first = await open(a);
    assert(text('play') === '暂停' && text('comments') === '查看评论(54)' &&
      text('source') === '来自：合成播客 301' && action('directory') && !action('download') &&
      !action('collect') && !action('next'), 'current episode uses voice queue rules');
    await capture('playing_menu', { menu: true, target: action('play') });
    const paused = await wait(playing('paused'), 'queue More pauses the native episode');
    assert(same(paused) && paused.queueRevision === initial.revision, 'pause keeps native queue and decoder');
    await wait(() => !panel() && document.activeElement === first, 'Pause restores this row opener');
    await capture('paused', { target: row(b).querySelector('.queue-song'), focused: a });

    step = 'context-menu-and-escape';
    await wait(() => text('comments') === '查看评论', 'trusted context opens second episode, count fallback');
    assert(text('play') === '播放' && action('download') && !action('directory'), 'non-current missing download branch');
    await capture('fallback_menu', { menu: true });
    const second = trigger(b);
    await wait(() => !panel() && document.activeElement === second, 'Escape returns focus to second opener');
    await capture('escaped', { focused: b });

    step = 'remove-second-row';
    await open(b);
    assert(text('remove') === '从列表中删除', 'local queue-only removal');
    await capture('remove_menu', { menu: true, target: action('remove') });
    await wait(async () => {
      const current = await queue();
      return current.tracks.length === 1 && current.tracks[0].entryId === a && current.revision !== initial.revision;
    }, 'the selected second row has been removed');
    await wait(() => !panel() && !row(b) && document.activeElement === row(a)?.querySelector('.queue-song'),
      'remove focuses the remaining row');
    assert(same(await playback()), 'remove leaves current native decoder intact');
    await capture('removed', { focused: a });

    step = 'resume-from-more';
    await open(a);
    assert(text('play') === '播放', 'paused queue episode exposes Play');
    await capture('resume_menu', { menu: true, target: action('play') });
    await wait(async () => {
      const view = await playing('playing')();
      return view && same(view) && view.position > paused.position + .12;
    }, 'resume reuses the native decoder and position');
    await capture('resumed');

    step = 'native-directory';
    await open(a);
    assert(action('directory') && !action('download'), 'completed episode directory is available');
    await capture('directory_menu', { menu: true, target: action('directory') });
    await wait(() => !panel() && document.activeElement === trigger(a) && trusted.directory === 1,
      'native directory action restores the same opener');
    await capture('directory_closed', { focused: a });
    assert(Object.values(trusted).every((count) => count === 1), 'all actual X11 actions reached the correct controls');
    await emit('complete', { ok: true, stageCount: count, trusted });
  } catch (error) {
    await emit('failed', { step, error: String(error), stageCount: count });
  }
})();
