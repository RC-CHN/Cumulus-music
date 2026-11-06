// Synthetic audio and private fixture; all selection/action gestures use the
// XTest driver, with renderer trust, hit and modifier receipts.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector) ?? null;
  const panel = () => one('.lyrics-panel:not(.lyrics-panel-leave-active)');
  const shelf = () => one('[data-voice-song-card]', panel());
  const row = (index) => one(`[data-voice-song-row][data-row-index="${index}"]`, shelf());
  const rowTitle = (index) => one('.voice-song-title', row(index));
  const cover = (index) => one('[data-voice-song-cover]', row(index));
  const menu = (key) => one(`[data-menu-key="${key}"]`);
  const label = (name, root = document) => one(`button[aria-label="${name}"]`, root);
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, values = {}) => invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...values } });
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const frame = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const equal = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const assert = (value, message) => { if (!value) throw Error(message); };
  const enabled = (node) => !!node?.getClientRects().length && !node.closest('[inert]') &&
    !node.disabled && node.getAttribute('aria-disabled') !== 'true';
  const mode = window.__voiceSongsMultiCase;
  const events = [], reads = [], selectedActions = [], singleActions = [];
  let epoch, last, count = 0, restore;
  const player = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  const stateView = (s) => ({ kind: s.resourceKind, state: s.state, trackId: s.track?.id,
    voiceId: s.track?.voiceId ?? null, playId: s.playId, entryId: s.entryId, position: s.position,
    queueRevision: s.queueRevision, seeking: s.nativeSeeking, error: s.error });
  const queueView = (q) => ({ current: q.current, revision: q.revision,
    entries: q.tracks.map((entry) => ({ entryId: entry.entryId, id: entry.track.id,
      voiceId: entry.track.voiceId ?? null })) });
  const ids = (q) => q.entries.map((e) => [e.id, e.voiceId]);
  const shown = () => [...(shelf()?.querySelectorAll('[aria-selected="true"][data-voice-song-row]') ?? [])]
    .map((item) => Number(item.dataset.rowIndex));
  const hit = (target) => {
    if (!(target instanceof Element)) return null;
    const key = target.closest('[data-menu-key]')?.dataset.menuKey;
    if (key) return `menu:${key}`;
    const index = target.closest('[data-voice-song-row]')?.dataset.rowIndex;
    return index === undefined ? null : `${target.closest('[data-voice-song-cover]') ? 'cover' : 'row'}:${index}`;
  };
  const observedTypes = ['mousedown', 'mouseup', 'dblclick', 'contextmenu', 'click'];
  const observe = (event) => {
    const target = hit(event.target);
    if (target && events.length < 64) events.push({ target, type: event.type, trusted: event.isTrusted,
      button: event.button, ctrl: event.ctrlKey, shift: event.shiftKey });
  };
  for (const type of observedTypes) document.addEventListener(type, observe, true);
  async function wait(check, reason) {
    last = reason;
    const until = performance.now() + 15000;
    while (performance.now() < until) {
      const result = await check();
      if (result) return result;
      await sleep(45);
    }
    throw Error(reason);
  }
  async function click(find, reason) {
    const node = await wait(() => enabled(find()) && find(), reason);
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    node.click();
  }
  async function capture(stage, extra = {}) {
    await wait(() => !one('.lyrics-panel-enter-active,.lyrics-panel-leave-active') &&
      (!panel() || Number(getComputedStyle(panel()).opacity) >= .999), 'settled vinyl layout');
    await frame();
    const id = `ux_${String(++count).padStart(2, '0')}_${stage}`;
    window.__podcastProgressStage = '';
    const p = stateView(await player()), q = queueView(await queue());
    await emit(id, { actual: document.visibilityState === 'visible', mode, player: p, queue: q,
      selected: shown(), focusRow: document.activeElement?.closest('[data-voice-song-row]')?.dataset.rowIndex ?? null,
      events: structuredClone(events), reads: structuredClone(reads),
      selectedActions: structuredClone(selectedActions), singleActions: structuredClone(singleActions), ...extra });
    await wait(() => window.__podcastProgressStage === id, 'native capture acknowledgement');
    return { p, q };
  }
  async function input(stage, find, { target, button = 1, ctrl = false, shift = false, count = 1 }, check) {
    const node = await wait(() => enabled(find()) && find(), `visible ${target}`);
    node.scrollIntoView({ block: 'nearest' });
    await frame();
    const rect = node.getBoundingClientRect();
    const x = Math.floor(rect.left + rect.width / 2), y = Math.floor(rect.top + rect.height / 2);
    assert(hit(document.elementFromPoint(x, y)) === target, `actual hit for ${target}`);
    const before = events.length;
    await capture(stage, { pointer: { x, y, width: innerWidth, height: innerHeight, ratio: devicePixelRatio },
      input: { target, button, ctrl, shift, count } });
    const type = button === 3 ? 'contextmenu' : count === 2 ? 'dblclick' : 'click';
    const inputEvent = { target, type, trusted: true, button: button === 3 ? 2 : 0, ctrl, shift };
    await wait(() => events.slice(before).some((e) => equal(e, inputEvent)), `trusted X11 ${type} ${target}`);
    if (check) await wait(check, `visible result after ${type} ${target}`);
  }
  async function waitPlaying(kind, id) {
    return wait(async () => {
      const p = await player();
      return p.state === 'playing' && p.resourceKind === kind &&
        (kind === 'voice' ? p.track?.voiceId : p.track?.id) === id &&
        p.position > .2 && !p.nativeSeeking && !p.error && p;
    }, `native ${kind} ${id} decodes and advances`);
  }
  try {
    assert(['menu', 'replace', 'add', 'cover'].includes(mode), 'explicit trusted synthetic case');
    await wait(async () => {
      const account = await invoke('account_snapshot');
      if (account.status !== 'authenticated' || !enabled(label('打开设置'))) return false;
      epoch = account.epoch;
      return enabled(one('input[aria-label="搜索音乐"]'));
    }, 'authenticated isolated app ready');
    const provides = one('#app').__vue_app__._context.provides;
    const core = provides[Reflect.ownKeys(provides).find((key) => typeof key === 'symbol' && key.description === 'core-client')];
    const methods = [['playerVoiceSongs', reads], ['playerVoiceSongsSelection', selectedActions],
      ['playerVoiceSongPlay', singleActions]];
    const originals = methods.map(([name]) => core[name]);
    assert(originals.every((method) => typeof method === 'function'), 'production typed native adapters');
    for (const [i, [name, trace]] of methods.entries()) {
      core[name] = async (...args) => {
        const call = { args: structuredClone(args) };
        trace.push(call);
        try {
          const value = await originals[i].apply(core, args);
          call.result = name === 'playerVoiceSongs' ? value : stateView(value);
          return value;
        } catch (error) {
          call.error = error?.code ?? String(error);
          throw error;
        }
      };
    }
    restore = () => { methods.forEach(([name], i) => { core[name] = originals[i]; }); };

    const preference = await invoke('queue_click_preferences', { epoch });
    assert(preference.value.replaceQueue === true, 'fresh native replace preference');
    if (mode === 'add') {
      const changed = await invoke('queue_click_preferences_command', { epoch, command: {
        kind: 'change', expected_revision: preference.value.revision, replace_queue: false,
      } });
      assert(changed.value.replaceQueue === false && changed.value.revision !== preference.value.revision,
        'native exact double-click add preference');
    }
    await click(() => one('input[aria-label="搜索音乐"]'), 'open discovery');
    await click(() => label('播客榜：更多', one('#search-discovery-panel:not(.menu-leave-active)')), 'open classic podcast page');
    await wait(() => one('.podcast-discovery .podcast-top[data-layout="classic"]') &&
      one('[data-podcast-scene-fm] [data-scene-fm-title]'), 'real classic Scene FM card ready');
    await click(() => label('播放随心听FM', one('[data-podcast-scene-fm]')), 'play the first synthetic episode');
    const episode = await waitPlaying('voice', '601');
    await click(() => label('暂停'), 'park actual native decoder');
    let previous;
    await wait(async () => {
      const p = await player();
      const same = p.position === previous;
      previous = p.position;
      return same && p.state === 'paused' && p.playId === episode.playId && p.position > .2 && !p.nativeSeeking;
    }, 'native episode is paused and position steady');
    await click(() => label('打开播放详情'), 'open exact episode vinyl');
    const page = await wait(() => {
      const p = reads[0]?.result;
      return p?.receipt && p.voiceId === '601' && p.playId === episode.playId &&
        one('[aria-multiselectable="true"]', shelf()) && row(3) && p;
    }, 'bound four-song native card');
    assert(equal(page.tracks.map((item) => item.id), ['1710', '1601', '1790', '1791']), 'exact visible fixture order');
    const base = await capture('baseline', { replace: mode !== 'add' });
    assert(equal(ids(base.q), [['1601','601'], ['1602','602']]) && base.p.state === 'paused', 'original paused episode queue');
    const preserved = async () => {
      const p = stateView(await player());
      return ['playId','entryId','state','position','trackId','voiceId','kind','seeking'].every((key) => p[key] === base.p[key]);
    };
    const expected = (kind, index, indices) => [epoch, episode.playId, page.receipt, { kind, index, indices }, 'standard'];
    const selectionOnly = () => !selectedActions.length && !singleActions.length;

    if (mode === 'menu' || mode === 'cover') {
      await input('first_row', () => rowTitle(1), { target: 'row:1' }, () => equal(shown(), [1]));
      await input('control_row', () => rowTitle(0), { target: 'row:0', ctrl: true }, () => equal(shown(), [0,1]));
      if (mode === 'menu') {
        await input('control_denied', () => rowTitle(2), { target: 'row:2', ctrl: true }, () => equal(shown(), [0,1,2]));
        assert(selectionOnly() && await preserved(), 'selection did not touch the decoder');
        await input('menu_next_open', () => rowTitle(0), { target: 'row:0', button: 3 }, () => enabled(menu('next')));
        assert(equal(shown(), [0,1,2]), 'right-click preserves the chosen group');
        await input('menu_next_choose', () => menu('next'), { target: 'menu:next' }, () => selectedActions[0]?.result);
        const queued = await capture('next_queued');
        assert(equal(selectedActions[0].args, expected('next_play', 0, [1,0,2])) && !singleActions.length,
          'next receives the frozen non-display-order group');
        assert(await preserved() && queued.q.current === base.q.current &&
          equal(ids(queued.q), [['1601','601'], ['1601',null], ['1710',null], ['1602','602']]) &&
          queued.q.entries[0].entryId === base.q.entries[0].entryId &&
          queued.q.entries[3].entryId === base.q.entries[1].entryId, 'queue next filters denied rows and preserves exact paused entry');
        await input('menu_play_open', () => rowTitle(0), { target: 'row:0', button: 3 }, () => enabled(menu('play')));
        await input('menu_play_choose', () => menu('play'), { target: 'menu:play' }, () => selectedActions[1]?.result);
        const playing = await waitPlaying('catalog', '1710');
        const final = await capture('playing');
        assert(equal(selectedActions[1].args, expected('menu_play', 0, [1,0,2])) &&
          selectedActions.length === 2 && !singleActions.length && equal(final.q.entries, queued.q.entries) &&
          final.q.current === playing.entryId, 'menu play starts the clicked anchor and retains the old queue');
      } else {
        assert(selectionOnly() && await preserved(), 'selection alone does not play');
        await input('cover_choose', () => cover(0), { target: 'cover:0' }, () => singleActions[0]?.result);
        const playing = await waitPlaying('catalog', '1710');
        const final = await capture('playing');
        assert(!selectedActions.length && singleActions.length === 1 &&
          equal(singleActions[0].args, [epoch, episode.playId, page.receipt, { kind: 'cover', index: 0 }, 'standard']) &&
          equal(ids(final.q), [['1601','601'], ['1710',null], ['1602','602']]) &&
          final.q.current === playing.entryId, 'artwork stays a one-song action inside a group');
      }
    } else {
      await input('first_row', () => rowTitle(0), { target: 'row:0' }, () => equal(shown(), [0]));
      assert(selectionOnly() && await preserved(), 'selection alone does not play');
      await input('shift_double', () => rowTitle(1), { target: 'row:1', shift: true, count: 2 },
        () => selectedActions[0]?.result);
      const playing = await waitPlaying('catalog', '1601');
      const final = await capture('playing');
      const expectedQueue = mode === 'replace' ? [['1710',null],['1601',null]] :
        [['1601','601'], ['1601',null], ['1602','602']];
      assert(!singleActions.length && selectedActions.length === 1 &&
        equal(selectedActions[0].args, expected('double_click', 1, [0,1])) &&
        equal(ids(final.q), expectedQueue) && final.q.current === playing.entryId,
        'modified double-click receives the same group but uses the native add/replace preference');
    }
    await emit('complete', { ok: true, mode, count, realMusicRequests: 0, realAccountWrites: 0,
      physicalAudioVerified: false });
  } catch (error) {
    await emit('failed', { ok: false, mode, last, error: String(error), events, reads,
      selectedActions, singleActions, selected: shown(), player: epoch ? await player().then(stateView, () => null) : null });
  } finally {
    for (const type of observedTypes) document.removeEventListener(type, observe, true);
    restore?.();
  }
})();
