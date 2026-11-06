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
  async function captureOrigin(name, detail) {
    await wait(() => !one('.page-enter-active,.page-leave-active,.queue-panel-enter-active'),
      'source navigation layout settled');
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const stage = `ux_${String(states.length + 1).padStart(2, '0')}_${name}`;
    states.push(name);
    window.__CUMULUS_ALBUM_CAPTURE_ACK__ = '';
    await emit(stage, { name, ...detail, player: await player(), queue: await queue() });
    await wait(() => window.__CUMULUS_ALBUM_CAPTURE_ACK__ === stage, 'source native capture acknowledgement');
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
    const queued = await queue();
    const unbound = queued.tracks.find((row) => row.track.id === '900');
    const fromAlbum = queued.tracks.find((row) => row.track.id === '14');
    assert(unbound.track.albumId === '7' && fromAlbum.track.albumId === '7' && !unbound.origin &&
      fromAlbum.origin?.kind === 'album' && fromAlbum.origin.id === '8',
      'row provenance must differ from both current entry and track metadata');
    const artistPage = () => one('.catalog-heading h1')?.textContent === '测试歌手' &&
      one('[data-catalog-key="album:8"]');
    const albumPage = () => one('.catalog-page .album-header:not(.album-header-compact) h1')
      ?.textContent.trim() === '漫长的午后与未完的旋律 · A quiet afternoon / Extended edition';
    click(one('.back-button'));
    await wait(artistPage, 'return to artist before opening row source');
    click(one('.queue-toggle'));
    const row = (entry) => one(`.queue-row[data-entry-id="${entry.entryId}"]`);
    const sourceButton = () => row(fromAlbum)?.querySelector('.queue-origin');
    const source = await wait(() => {
      const target = sourceButton();
      return target && !target.disabled && target;
    }, 'current queue row source action');
    assert(row(unbound) && !row(unbound).querySelector('.queue-origin'), 'plain song must not get its album metadata as a source');
    source.focus({ preventScroll: true });
    await wait(() => {
      const box = source.getBoundingClientRect();
      return document.activeElement === source && getComputedStyle(source).opacity === '1' &&
        source.contains(document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2));
    }, 'keyboard focused origin is visible and reachable');
    await captureOrigin('queue_source_ready', { sourceFocused: true, source: fromAlbum.origin });
    click(source);
    await wait(albumPage, 'queue source opened the originating album');
    assert(one('.queue-panel') && document.activeElement === sourceButton(),
      'source navigation dismissed the queue or stole row focus');
    const unchanged = async () => {
      const current = await player(), displayed = await queue();
      return current.state === 'paused' && current.playId === after.playId &&
        current.entryId === after.entryId && current.position === after.position &&
        current.queueRevision === after.queueRevision && JSON.stringify(displayed) === JSON.stringify(queued);
    };
    assert(await unchanged(), 'source navigation changed paused decoder, clock or queue');
    await captureOrigin('queue_source_opened', { queueStayedOpen: true, source: fromAlbum.origin });
    click(sourceButton());
    assert(albumPage() && await unchanged(), 'same destination changed playback');
    click(one('.back-button'));
    await wait(artistPage, 'source back returned to the artist without duplicate history');
    assert(one('.queue-panel') && await unchanged(), 'source back changed queue or paused playback');
    await captureOrigin('queue_source_back', { queueStayedOpen: true, source: fromAlbum.origin });

    const namesOnly = queued.tracks.find((entry) => entry.track.id === '901');
    const credits = () => row(fromAlbum)?.querySelector('.queue-credits');
    const secondCredit = () => credits()?.querySelectorAll('.queue-artist')[1];
    const shownCredits = (entry) => [...row(entry).querySelector('.queue-credits').children]
      .filter((node) => !node.matches('.queue-credit-separator'))
      .map((node) => ({ name: node.textContent.trim(), link: node.matches('.queue-artist') }));
    assert(fromAlbum.track.artistRefs.map(({ id }) => id).join(',') === '12,13,' &&
      namesOnly.track.artistRefs.length === 1 && !namesOnly.track.artistRefs[0].id,
      'the native other-row and names-only credits were not projected');
    const credit = await wait(() => secondCredit() && !secondCredit().disabled && secondCredit(),
      'second queue credit is independently actionable');
    const creditState = shownCredits(fromAlbum), namesOnlyState = shownCredits(namesOnly);
    assert(creditState.map(({ name, link }) => `${name}:${link}`).join('|') ===
      '测试歌手:true|相似歌手 13:true|仅有署名:false' &&
      namesOnlyState.map(({ name, link }) => `${name}:${link}`).join('|') === '无编号歌手:false',
      'ordered queue credit names or independent actions changed');
    assert(!credit.closest('.queue-song') && credit.getAttribute('aria-label') === '查看歌手：相似歌手 13',
      'the second credit is nested in the row transport or has the wrong label');
    credit.focus({ preventScroll: true });
    const canHit = () => {
      const box = credit.getBoundingClientRect();
      return box.width > 0 && box.height > 0 && credit.isConnected && document.activeElement === credit &&
        credit.contains(document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2));
    };
    await wait(canHit, 'second queue credit is focusable and pointer reachable');
    assert(await unchanged(), 'credit inspection changed native playback or queue');
    await captureOrigin('queue_artist_ready', { credits: creditState, namesOnly: namesOnlyState, creditHit: true });
    click(credit);
    const artistHeading = () => one('.catalog-page .catalog-heading h1');
    const artistTitle = () => artistHeading()?.textContent.trim();
    await wait(() => artistTitle() === '相似歌手 13', 'second queue credit opened artist 13');
    await wait(() => document.activeElement === artistHeading(), 'artist page entry focused its heading');
    assert(one('.queue-panel') && !row(fromAlbum).classList.contains('selected') && await unchanged(),
      'artist navigation disturbed queue selection, decoder or paused clock');
    await captureOrigin('queue_artist_opened', { artist: artistTitle(), queueStayedOpen: true, headingFocused: true });
    click(secondCredit());
    assert(artistTitle() === '相似歌手 13' && await unchanged(), 'same artist navigation changed the native state');
    click(one('.back-button'));
    await wait(artistPage, 'one Back restored the original artist, without duplicate history');
    assert(one('.queue-panel') && await unchanged(), 'artist Back changed the queue or native playback');
    await captureOrigin('queue_artist_back', { artist: artistTitle(), queueStayedOpen: true });
    const likeButton = (entry) => row(entry)?.querySelector('.queue-like');
    const shownLike = await wait(() => likeButton(fromAlbum), 'other-song like control');
    assert(fromAlbum.likeSongId === '14' && unbound.likeSongId === '900' &&
      shownLike.getAttribute('aria-pressed') === 'false' &&
      likeButton(unbound)?.getAttribute('aria-pressed') === 'false',
    'native queue song identity and initial liked state');
    let stale;
    try {
      await invoke('player_queue_like', { epoch, revision: (BigInt(queued.revision) + 1n).toString(),
        entryId: fromAlbum.entryId, liked: true });
    } catch (error) { stale = String(error); }
    assert(stale?.endsWith(': snapshot_changed') && await unchanged(), 'stale queue rejected before writing');
    shownLike.focus({ preventScroll: true });
    await wait(() => {
      const box = shownLike.getBoundingClientRect();
      return document.activeElement === shownLike && getComputedStyle(shownLike).opacity === '1' &&
        shownLike.contains(document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2));
    }, 'keyboard can reach the row like control');
    await captureOrigin('queue_like_ready', { staleRejected: true, likePressed: false });
    click(shownLike);
    shownLike.click(); // duplicate before the account readback returns
    const review = await wait(() => one('.queue-like-notice button'), 'lost reply recorded for readback');
    assert(likeButton(fromAlbum).getAttribute('aria-pressed') === 'false' && await unchanged(),
      'an uncertain write cannot optimistically flip the row');
    await captureOrigin('queue_like_uncertain', { likePressed: false, reviewAvailable: true });
    click(review);
    const operation = await wait(() => one('.like-recovery-dialog[open] [data-song-id="14"] .secondary'),
      'review dialog has the original song');
    click(operation);
    const closeReview = await wait(() => {
      const close = one('.like-recovery-dialog[open] [aria-label="关闭喜欢状态核对"]');
      return likeButton(fromAlbum)?.getAttribute('aria-pressed') === 'true' && close && !close.disabled && close;
    }, 'explicit readonly reconciliation');
    click(closeReview);
    await wait(() => !one('.like-recovery-dialog') && document.activeElement === likeButton(fromAlbum),
      'review returned focus to the original song action');
    assert(likeButton(unbound).getAttribute('aria-pressed') === 'false' &&
      await unchanged(), 'liking other row preserves loaded decoder and queue');
    await captureOrigin('queue_like_confirmed', { likePressed: true, otherTrackUnaffected: true });
    click(likeButton(fromAlbum));
    await wait(() => likeButton(fromAlbum)?.getAttribute('aria-pressed') === 'false' &&
      one('.queue-like-notice')?.textContent.trim() === '已取消喜欢', 'unlike readback');
    assert(await unchanged(), 'unlike changed playback or queue');
    await captureOrigin('queue_like_removed', { likePressed: false });
    const queueMenu = () => one('[role="menu"][aria-label="播放列表歌曲操作"]');
    const menuKeys = () => [...queueMenu().querySelectorAll('[data-menu-key]')].map((node) => node.dataset.menuKey);
    const menuItem = (key) => queueMenu()?.querySelector(`[data-menu-key="${key}"]`);
    const rightClick = (entry) => {
      const node = row(entry);
      const box = node.getBoundingClientRect();
      node.dispatchEvent(new MouseEvent('contextmenu', { button: 2, clientX: box.x + 10,
        clientY: box.y + 10, bubbles: true, cancelable: true }));
    };
    const dismissMenu = async () => {
      queueMenu().dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
      await wait(() => !queueMenu(), 'queue menu dismissed');
    };
    let oldMenu;
    try {
      await invoke('player_queue_catalog_menu', {
        epoch, revision: (BigInt(queued.revision) + 1n).toString(), entryId: fromAlbum.entryId,
      });
    } catch (error) { oldMenu = String(error); }
    assert(oldMenu?.endsWith(': snapshot_changed'), 'stale menu rejected without reading comments');
    click(row(fromAlbum).querySelector('.queue-row-more'));
    await wait(() => menuItem('comments')?.textContent.includes('36'), 'other row comments count');
    assert(menuKeys().join(',') === 'play,comments,collect,download,share,artist,album,source,copy,remove' &&
      menuItem('album').textContent.includes('测试专辑') && menuItem('source').textContent.includes('来自：专辑') &&
      !menuItem('source').disabled && await unchanged(), 'ordinary queue actions and independent origin');
    await captureOrigin('queue_menu_other', { keys: menuKeys(), staleMenuRejected: true });
    click(menuItem('source'));
    await wait(albumPage, 'menu follows actual queue origin, not track album');
    assert(await unchanged(), 'menu source replaced paused track');
    click(one('.back-button'));
    await wait(artistPage, 'menu source Back');
    rightClick(unbound);
    await wait(() => menuItem('comments')?.textContent.trim() === '查看评论', 'count failure uses bare comments item');
    assert(menuItem('source').disabled && await unchanged(), 'unknown source is not a link');
    await captureOrigin('queue_menu_fallback', { keys: menuKeys(), bareComment: true });
    await dismissMenu();
    const selectedRows = ['12', '13'].map((id) => queued.tracks.find((entry) => entry.track.id === id));
    for (const [index, entry] of selectedRows.entries()) {
      row(entry).querySelector('.queue-song').dispatchEvent(new MouseEvent('click', {
        bubbles: true, detail: 0, ctrlKey: index === 1,
      }));
    }
    rightClick(selectedRows[0]);
    await wait(() => menuItem('remove'), 'selected group menu');
    assert(menuKeys().join(',') === 'play,collect,download,artist,album,source,remove' &&
      selectedRows.every((entry) => row(entry).classList.contains('selected')) && await unchanged(),
    'group preserves selected rows and suppresses single-song actions');
    await captureOrigin('queue_menu_group', { keys: menuKeys() });
    click(menuItem('remove'));
    await wait(async () => queueIds(await queue()) === '900,14,11,901', 'group removed from queue only');
    const remaining = await player();
    assert(remaining.state === 'paused' && remaining.playId === after.playId &&
      remaining.entryId === after.entryId && remaining.position === after.position &&
      remaining.queueRevision !== after.queueRevision, 'removal retains the loaded paused entry');
    await captureOrigin('queue_menu_removed', { removed: selectedRows.map(({ track }) => track.id) });
    rightClick(unbound);
    await wait(() => menuItem('play')?.textContent.trim() === '播放', 'paused current row shows Play');
    click(menuItem('play'));
    const resumedFromMenu = await wait(async () => {
      const snapshot = await player();
      return snapshot.state === 'playing' && snapshot.position > remaining.position && snapshot;
    }, 'queue menu resumed the current decoder');
    const retained = (snapshot) => ['playId', 'entryId', 'queueRevision'].every((key) => snapshot[key] === remaining[key]);
    assert(retained(resumedFromMenu), 'menu Play must not reopen the current entry');
    await captureOrigin('queue_menu_resumed', { transport: 'play' });
    rightClick(unbound);
    await wait(() => menuItem('play')?.textContent.trim() === '暂停', 'playing current row shows Pause');
    click(menuItem('play'));
    const pausedFromMenu = await wait(async () => {
      const snapshot = await player();
      return snapshot.state === 'paused' && snapshot;
    }, 'menu paused the current decoder');
    assert(retained(pausedFromMenu), 'menu Pause must not reopen the current entry');
    await captureOrigin('queue_menu_paused', { transport: 'pause' });
    await emit('complete', { ok: true, states, realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', { ok: false, error: String(error), stack: error?.stack, states });
  }
})();
