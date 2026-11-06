// The production album screen and native actor, on a closed synthetic catalog.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
  const invoke = async (command, args = {}) => {
    try {
      return await window.__TAURI_INTERNALS__.invoke(command, args);
    } catch (error) {
      const code = typeof error?.code === 'string' && /^[a-z_]+$/.test(error.code) ? error.code : 'invoke_error';
      throw Error(`${command}: ${code}`);
    }
  };
  const emit = (stage, value = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...value } });
  const button = (text) => all('button').find((node) => node.textContent.trim() === text);
  const assert = (ok, message) => { if (!ok) throw Error(message); };
  async function wait(check, label) {
    for (let i = 0; i < 180; i++) {
      const value = await check();
      if (value) return value;
      await new Promise((resolve) => setTimeout(resolve, 85));
    }
    throw Error(label);
  }
  const click = (target) => {
    assert(target && !target.disabled, 'action unavailable');
    target.focus({ preventScroll: true });
    target.click();
  };
  const header = () => one('.catalog-page [aria-label="按时长排序"]');
  const order = () => header()?.parentElement.getAttribute('aria-sort');
  const rows = () => all('[data-album-panel="songs"] .track-row').map((row) => row.dataset.trackId);
  const original = '11,12,13,14', asc = '12,14,11,13', desc = '11,13,14,12';
  let epoch;
  const player = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  const act = (action) => invoke('player_action', { epoch, action });
  const queueIds = (view) => view.tracks.map((entry) => entry.track.id).join(',');
  const states = [];
  async function capture(name, playing = false) {
    await wait(() => !one('.page-enter-active,.page-leave-active,.catalog-tab-enter-active,.menu-leave-active'), 'layout settled');
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const target = header(), box = target.getBoundingClientRect();
    assert(box.width > 0 && box.height > 0 && !target.closest('[inert]') &&
      target.contains(document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2)),
      'duration header not painted or reachable');
    assert(one('.catalog-page').scrollWidth <= one('.catalog-page').clientWidth + 1, 'catalog overflow');
    const stage = `ux_${String(states.length + 1).padStart(2, '0')}_${name}`;
    states.push(name);
    window.__CUMULUS_ALBUM_CAPTURE_ACK__ = '';
    await emit(stage, {
      name, playing, order: order(), rows: rows(), focused: document.activeElement === target,
      player: await player(), queue: await queue(),
    });
    await wait(() => window.__CUMULUS_ALBUM_CAPTURE_ACK__ === stage, 'native capture acknowledgement');
  }
  async function untilPlayed(id, list) {
    await wait(async () => {
      const state = await player(), view = await queue();
      return state.state === 'playing' && state.track?.id === id && state.position > .2 &&
        state.origin?.kind === 'album' && state.origin.id === '8' && queueIds(view) === list;
    }, `native playback ${id} with ${list}`);
  }
  async function sortTo(expected, visible) {
    click(header());
    await wait(() => order() === expected && rows().join(',') === visible, `sort ${expected}`);
    assert(document.activeElement === header(), 'sort header lost keyboard focus');
  }
  async function openAlbum() {
    const card = await wait(() => one('[data-catalog-key="album:8"]'), 'album 8 card');
    click(card);
    await wait(() => {
      const album = one('.catalog-page .album-header:not(.album-header-compact)');
      const songs = one('[data-album-panel="songs"]');
      return album?.querySelector('h1')?.textContent.trim() ===
          '漫长的午后与未完的旋律 · A quiet afternoon / Extended edition' &&
        !album.querySelector('[data-album-play]').disabled && songs && !songs.inert &&
        header() && rows().length === 4 && rows().every((id) => original.split(',').includes(id));
    }, 'album 8 loaded the four scoped song rows');
  }
  try {
    click(await wait(() => one('.track-credit-links button'), 'daily artist credit'));
    await wait(() => one('.catalog-heading h1')?.textContent === '测试歌手', 'artist');
    click(one('[data-catalog-tab="albums"]'));
    await openAlbum();
    ({ epoch } = await invoke('account_snapshot'));
    await act({ kind: 'mode', mode: 'single_loop' });
    assert(order() === 'none' && rows().join(',') === original, 'initial service order');
    await capture('original');
    await sortTo('ascending', asc);
    await capture('ascending');
    await sortTo('descending', desc);
    await capture('descending');

    // A same-resource new visit, including actual application Back.
    click(one('.back-button'));
    await openAlbum();
    assert(order() === 'descending' && rows().join(',') === desc, 'return lost album sort');
    await capture('restored');
    await sortTo('none', original);
    await capture('reset');
    await sortTo('ascending', asc);

    const field = one('[aria-label="搜索专辑歌曲"]');
    field.value = '合成候选';
    field.dispatchEvent(new InputEvent('input', { bubbles: true }));
    await wait(() => rows().join(',') === '12,11,13', 'filtered album order');
    click(one('[data-track-id="13"] .track-play'));
    await untilPlayed('13', '12,11,13');
    await capture('filtered_row', true);

    const sameRow = one('[data-album-panel="songs"] [data-track-id="13"]');
    const doubleClick = () => sameRow.dispatchEvent(new MouseEvent('dblclick', { bubbles: true, button: 0, detail: 2 }));
    const initial = await player();
    const sameDecoder = (state) => state.playId === initial.playId && state.entryId === initial.entryId &&
      state.queueRevision === initial.queueRevision;
    doubleClick();
    const rowPaused = await wait(async () => {
      const state = await player();
      return state.state === 'paused' && state;
    }, 'double-click paused the active album row');
    assert(sameDecoder(rowPaused) && rowPaused.position >= initial.position &&
      queueIds(await queue()) === '12,11,13', 'row pause changed decoder, position or queue');
    await capture('same_row_pause');
    doubleClick();
    await untilPlayed('13', '12,11,13');
    assert(sameDecoder(await player()), 'row resume replaced the decoder or queue');
    await capture('same_row_resume', true);

    click(one('.album-header:not(.album-header-compact) [data-album-play]'));
    await untilPlayed('12', asc);
    assert(field.value === '合成候选' && rows().join(',') === '12,11,13', 'header altered active filter');
    await capture('full_album', true);

    // Set up an unrelated paused native entry; the action under test is the
    // visible album menu, not the fixture setup IPC.
    await invoke('play_selection', { epoch, selection: {
      kind: 'songs', ids: ['900', '901'], index: 0, quality: 'standard',
    } });
    await act({ kind: 'seek', seconds: 5 });
    const before = await act({ kind: 'pause', paused: true });
    click(one('.album-header:not(.album-header-compact) [aria-label="更多专辑操作"]'));
    click(await wait(() => button('添加全部至播放列表'), 'whole album queue action'));
    await wait(async () => queueIds(await queue()) === '900,12,14,11,13,901', 'whole album queue insertion');
    const after = await player();
    assert(after.state === 'paused' && after.track.id === '900' && after.entryId === before.entryId &&
      after.playId === before.playId && after.position === before.position, 'append changed paused source/clock');
    await capture('paused_append');
    await emit('complete', { ok: true, states, realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', { ok: false, error: String(error), stack: error?.stack, states });
  }
})();
