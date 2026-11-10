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
  const popularity = () => one('.catalog-page [aria-label="按热度排序"]');
  const popularityOrder = () => popularity()?.parentElement.getAttribute('aria-sort');
  const rows = () => all('[data-album-panel="songs"] .track-row').map((row) => row.dataset.trackId);
  const original = '11,12,13,14', asc = '12,14,11,13', desc = '11,13,14,12';
  const popAsc = '14,11,13,12', popDesc = '12,11,13,14';
  const albumName = '漫长的午后与未完的旋律 · A quiet afternoon / Extended edition';
  let epoch;
  const player = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  const act = (action) => invoke('player_action', { epoch, action });
  const queueIds = (view) => view.tracks.map((entry) => entry.track.id).join(',');
  const states = [];
  let transportAttempt, sourceFocus, appendBefore, appendAfter;
  const clock = (state) => ({
    state: state.state, trackId: state.track?.id, playId: state.playId, entryId: state.entryId,
    queueRevision: state.queueRevision, position: state.position, nativeSeeking: state.nativeSeeking,
    error: state.error,
  });
  async function capture(name, playing = false, extra = {}) {
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
      player: await player(), queue: await queue(), ...extra,
    });
    await wait(() => window.__CUMULUS_ALBUM_CAPTURE_ACK__ === stage, 'native capture acknowledgement');
  }
  async function captureOrigin(name, detail) {
    await wait(() => !one('.page-enter-active,.page-leave-active,.queue-panel-enter-active,.modal-enter-active'),
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
        state.origin?.kind === 'album' && state.origin.id === '8' &&
        state.originName === albumName && queueIds(view) === list;
    }, `native playback ${id} with ${list}`);
  }
  async function sortTo(expected, visible) {
    click(header());
    await wait(() => order() === expected && rows().join(',') === visible, `sort ${expected}`);
    assert(document.activeElement === header(), 'sort header lost keyboard focus');
  }
  async function sortPopularity(expected, visible) {
    click(popularity());
    await wait(() => popularityOrder() === expected && rows().join(',') === visible, `popularity ${expected}`);
    assert(document.activeElement === popularity() && order() === 'none',
      'popularity focus and mutually exclusive duration sort');
  }
  function popularityFacts() {
    const meter = all('[data-album-panel="songs"] .track-row').map((row) => {
      const node = row.querySelector('[role="meter"][aria-label="热度"]');
      const width = node.getBoundingClientRect().width;
      const fill = node.firstElementChild.getBoundingClientRect().width;
      const value = Number(node.getAttribute('aria-valuenow'));
      assert(width > 0 && Math.abs(fill - width * value / 100) < 1, 'painted popularity fill');
      return [row.dataset.trackId, value];
    });
    return { popularityOrder: popularityOrder(), meter, popularityFocus: document.activeElement === popularity() };
  }
  async function openAlbum() {
    const card = await wait(() => one('[data-catalog-key="album:8"]'), 'album 8 card');
    click(card);
    await wait(() => {
      const album = one('.catalog-page .album-header:not(.album-header-compact)');
      const songs = one('[data-album-panel="songs"]');
      return album?.querySelector('h1')?.textContent.trim() === albumName &&
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
    for (const [direction, visible, name] of [
      ['descending', popDesc, 'pop_descending'], ['ascending', popAsc, 'pop_ascending'],
      ['none', original, 'pop_reset'],
    ]) {
      await sortPopularity(direction, visible);
      await capture(name, false, popularityFacts());
    }
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
    const loaded = await wait(async () => {
      const state = await player();
      return state.state === 'playing' && state.track?.id === '900' && state.playId && state.entryId &&
        !state.nativeSeeking && !state.error && state.position > .2 && state;
    }, 'unrelated native entry is playing');
    await act({ kind: 'pause', paused: true });
    await act({ kind: 'seek', seconds: 5 });
    let stableAt, stablePosition;
    const before = await wait(async () => {
      const state = await player();
      if (state.state !== 'paused' || state.playId !== loaded.playId || state.entryId !== loaded.entryId ||
          state.queueRevision !== loaded.queueRevision || state.nativeSeeking || state.error ||
          !Number.isFinite(state.position) || Math.abs(state.position - 5) > .15) {
        stableAt = undefined;
        return false;
      }
      if (stableAt === undefined || stablePosition !== state.position) {
        stablePosition = state.position;
        stableAt = performance.now();
      }
      // The seek command can return before libmpv's next poll; require the
      // same paused clock across more than two 200 ms native polling ticks.
      return performance.now() - stableAt >= 450 && state;
    }, 'unrelated entry is paused with a settled native seek');
    appendBefore = clock(before);
    click(one('.album-header:not(.album-header-compact) [aria-label="更多专辑操作"]'));
    click(await wait(() => button('添加全部至播放列表'), 'whole album queue action'));
    await wait(async () => queueIds(await queue()) === '900,12,14,11,13,901', 'whole album queue insertion');
    const after = await player();
    appendAfter = clock(after);
    assert(after.state === 'paused' && after.track?.id === '900' && after.entryId === before.entryId &&
      after.playId === before.playId && after.position === before.position && !after.nativeSeeking && !after.error,
      'append changed paused source/clock');
    await capture('paused_append', false, { appendBefore, appendAfter });
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
    const sourceDestination = one('.catalog-page .album-header:not(.album-header-compact) h1');
    const painted = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    // Checking as soon as H1 exists can miss the deferred page-entry focus
    // effect. Wait for the destination animation and its final focus frame.
    await painted();
    await wait(() => !(sourceDestination.closest('.page-view')?.getAnimations() ?? []).some((animation) => {
      const end = animation.effect?.getComputedTiming().endTime;
      return Number.isFinite(end) && (animation.playState === 'running' || animation.pending);
    }), 'origin destination animation finished');
    await painted();
    sourceFocus = {
      queueOpen: !!one('.queue-panel'),
      sourceConnected: source.isConnected,
      sameSource: sourceButton() === source,
      activeOnSource: document.activeElement === sourceButton(),
      activeOnDestination: document.activeElement === sourceDestination,
      activeTag: document.activeElement?.tagName,
      activeClass: document.activeElement?.className,
    };
    assert(sourceFocus.queueOpen && sourceFocus.sameSource && sourceFocus.activeOnSource,
      'source navigation dismissed the queue or stole row focus');
    const unchanged = async () => {
      const current = await player(), displayed = await queue();
      return current.state === 'paused' && current.playId === after.playId &&
        current.entryId === after.entryId && current.position === after.position &&
        current.queueRevision === after.queueRevision && JSON.stringify(displayed) === JSON.stringify(queued);
    };
    assert(await unchanged(), 'source navigation changed paused decoder, clock or queue');
    await captureOrigin('queue_source_opened', { queueStayedOpen: true, source: fromAlbum.origin, focus: sourceFocus });
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
      menuItem('album').textContent.includes('测试专辑') && menuItem('source').textContent.trim() === `来自：${albumName}` &&
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
    const provides = one('#app').__vue_app__._context.provides;
    const client = provides[Reflect.ownKeys(provides).find((key) => key.description === 'core-client')];
    const nativeCollect = client.playerQueueCollectRows.bind(client);
    const collectionReads = [];
    client.playerQueueCollectRows = async (owner, revision, entryIds) => {
      const result = await nativeCollect(owner, revision, entryIds);
      collectionReads.push({ owner, revision, entryIds: [...entryIds], resultOwner: result.epoch,
        resultRevision: result.revision, ids: result.tracks.map((track) => track.id), total: result.total });
      return result;
    };
    const picker = () => one('.playlist-collect-dialog[open]:not(.modal-leave-active)');
    click(menuItem('collect'));
    const destination = await wait(() => {
      const dialog = picker(), control = dialog?.querySelector('.collect-row[data-playlist-id="77"]');
      return dialog?.contains(document.activeElement) && collectionReads.length === 1 &&
        !dialog.querySelector('.collect-load') && control && !control.disabled && control;
    },
      'selected queue rows opened the native collection picker');
    const expectedEntries = selectedRows.map((entry) => entry.entryId);
    const collection = collectionReads[0];
    assert(collection.owner === epoch && collection.resultOwner === epoch &&
      collection.revision === queued.revision && collection.resultRevision === queued.revision &&
      collection.entryIds.join(',') === expectedEntries.join(',') && collection.ids.join(',') === '12,13' &&
      collection.total === 2 && !picker().querySelector('.collect-selection-summary') && await unchanged(),
      'picker uses only the captured native selection and preserves the paused decoder');
    assert(destination.textContent.includes('合成队列收藏目标'), 'account playlist list rendered');
    await captureOrigin('queue_menu_collect', { collection, destination: destination.dataset.playlistId });
    click(picker().querySelector('[aria-label="关闭歌单操作"]'));
    await wait(() => !one('.playlist-collect-dialog') &&
      document.activeElement === row(selectedRows[0]).querySelector('.queue-row-more'),
    'cancelling collection returns focus to the opening row');
    assert(collectionReads.length === 1 && selectedRows.every((entry) => row(entry).classList.contains('selected')) &&
      await unchanged(), 'cancel preserves the original queue selection and paused decoder');
    client.playerQueueCollectRows = nativeCollect;
    await captureOrigin('queue_menu_collect_cancelled', { focusReturned: true });
    rightClick(selectedRows[0]);
    await wait(() => menuItem('remove'), 'same group reopened after cancelling collection');
    click(menuItem('remove'));
    await wait(async () => queueIds(await queue()) === '900,14,11,901', 'group removed from queue only');
    const remaining = await player();
    assert(remaining.state === 'paused' && remaining.playId === after.playId &&
      remaining.entryId === after.entryId && remaining.position === after.position &&
      remaining.queueRevision !== after.queueRevision, 'removal retains the loaded paused entry');
    await captureOrigin('queue_menu_removed', { removed: selectedRows.map(({ track }) => track.id) });
    // The removed group's menu still has a "Play" button during its leave
    // animation. Bind the control to the current row's newly active menu.
    const currentTransportItem = (text) => {
      const anchor = row(unbound)?.querySelector('.queue-row-more');
      const menuId = anchor?.getAttribute('aria-controls');
      const menu = menuId && all('[role="menu"]:not(.menu-enter-active):not(.menu-leave-active):not([inert])')
        .find((candidate) => candidate.id === menuId);
      const button = menu?.querySelector('[data-menu-key="play"]');
      const box = button?.getBoundingClientRect();
      const active = menu &&
        anchor.getAttribute('aria-expanded') === 'true' && !anchor.closest('[inert]') &&
        !button?.closest('[inert]') && getComputedStyle(menu).visibility === 'visible';
      transportAttempt.menu = { id: menuId, active: !!active, label: button?.textContent.trim() ?? null };
      return active && button && !button.disabled && button.textContent.trim() === text &&
        box.width > 0 && box.height > 0 &&
        button.contains(document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2)) && button;
    };
    transportAttempt = { command: 'play', entryId: unbound.entryId };
    await wait(() => {
      const anchor = row(unbound)?.querySelector('.queue-row-more');
      return selectedRows.every((entry) => !row(entry)) && anchor?.isConnected &&
        !anchor.disabled && !anchor.closest('[inert]');
    }, 'removed group is absent and the current queue row is actionable');
    rightClick(unbound);
    click(await wait(() => currentTransportItem('播放'), 'paused current row shows reachable Play'));
    const resumedFromMenu = await wait(async () => {
      const snapshot = await player();
      return snapshot.state === 'playing' && snapshot.position > remaining.position && snapshot;
    }, 'queue menu resumed the current decoder');
    const retained = (snapshot) => ['playId', 'entryId', 'queueRevision'].every((key) => snapshot[key] === remaining[key]);
    assert(retained(resumedFromMenu), 'menu Play must not reopen the current entry');
    await captureOrigin('queue_menu_resumed', { transport: 'play' });
    transportAttempt = { command: 'pause', entryId: unbound.entryId };
    rightClick(unbound);
    click(await wait(() => currentTransportItem('暂停'), 'playing current row shows reachable Pause'));
    const pausedFromMenu = await wait(async () => {
      const snapshot = await player();
      return snapshot.state === 'paused' && snapshot;
    }, 'menu paused the current decoder');
    assert(retained(pausedFromMenu), 'menu Pause must not reopen the current entry');
    await captureOrigin('queue_menu_paused', { transport: 'pause' });

    click(one('.queue-toggle'));
    await wait(() => !one('.queue-panel'), 'close queue before popularity playback');
    if (!albumPage()) {
      if (!one('[data-catalog-key="album:8"]')) click(one('[data-catalog-tab="albums"]'));
      await openAlbum();
    }
    const input = one('[aria-label="搜索专辑歌曲"]');
    input.value = '';
    input.dispatchEvent(new InputEvent('input', { bubbles: true }));
    await wait(() => rows().length === 4 && popularityOrder() === 'none', 'album is ready for full popularity order');
    await sortPopularity('descending', popDesc);
    click(one('.album-header:not(.album-header-compact) [data-album-play]'));
    await untilPlayed('12', popDesc);
    await capture('pop_play_all', true, popularityFacts());

    await invoke('play_selection', { epoch, selection: {
      kind: 'songs', ids: ['900', '901'], index: 0, quality: 'standard',
    } });
    await wait(async () => (await player()).state === 'playing' && queueIds(await queue()) === '900,901',
      'fresh independent queue before popularity add');
    await act({ kind: 'pause', paused: true });
    const paused = await wait(async () => {
      const view = await player();
      return view.state === 'paused' && view.track?.id === '900' && view.playId && view.entryId && view;
    }, 'independent track paused');
    await sortPopularity('ascending', popAsc);
    click(one('.album-header:not(.album-header-compact) [aria-label="更多专辑操作"]'));
    click(await wait(() => button('添加全部至播放列表'), 'popularity album queue action'));
    await wait(async () => queueIds(await queue()) === '900,14,11,13,12,901', 'popularity album appended in order');
    const added = await player();
    assert(added.state === 'paused' && added.playId === paused.playId && added.entryId === paused.entryId,
      'popularity add preserves the paused source');
    await capture('pop_add_all', false, popularityFacts());
    await emit('complete', { ok: true, states, realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    const snapshot = transportAttempt ? await player().catch(() => null) : null;
    await emit('failed', { ok: false, error: String(error), stack: error?.stack, states,
      sourceFocus, appendBefore, appendAfter, transportAttempt, transportSnapshot: snapshot && {
        state: snapshot.state, playId: snapshot.playId, entryId: snapshot.entryId,
        queueRevision: snapshot.queueRevision, position: snapshot.position, errorCode: snapshot.errorCode,
      },
    });
  }
})();
