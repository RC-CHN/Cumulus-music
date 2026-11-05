// Real mounted vinyl/Settings/catalog UI and native null-output playback.
(async () => {
  const one = (s, scope = document) => scope.querySelector(s);
  const all = (s, scope = document) => [...scope.querySelectorAll(s)];
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const assert = (test, label) => { if (!test) throw new Error(label); };
  const equal = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const click = (node) => {
    assert(node && !node.disabled, 'available control');
    node.focus({ preventScroll: true });
    node.click();
  };
  const doubleClick = (node) => {
    assert(node, 'available row');
    node.dispatchEvent(new MouseEvent('dblclick', { bubbles: true, cancelable: true, detail: 2 }));
  };
  async function wait(test, label) {
    for (let round = 0; round < 180; ++round) {
      const result = await test();
      if (result) return result;
      await sleep(65);
    }
    throw new Error(label);
  }
  const footer = (label) => one('.playerbar [aria-label="' + label + '"]');
  const top = (label) => one('button[aria-label="' + label + '"]');
  const lyrics = () => one('.lyrics-panel:not(.lyrics-panel-leave-active)');
  const tab = () => one('[data-vinyl-similar-tab]', lyrics() || document);
  const pane = () => one('[data-vinyl-similar]', lyrics() || document);
  const row = (id) => one('[data-vinyl-similar-track="' + id + '"]', pane() || document);
  const songIds = () => all('[data-vinyl-similar-track]', pane() || document)
    .map((node) => node.getAttribute('data-vinyl-similar-track'));
  const playlistIds = () => all('[data-vinyl-similar-playlist]', pane() || document)
    .map((node) => node.getAttribute('data-vinyl-similar-playlist'));
  const songState = () => one('[data-vinyl-similar-songs]', pane() || document)
    ?.getAttribute('data-vinyl-similar-state');
  const choice = (value) => one('[data-queue-click-choice="' + value + '"]');
  const queueIds = (value) => value.tracks.map((entry) => entry.track.id);
  let epoch, stages = 0, step = 'restore', actionId = 0;
  const player = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  const preference = async () => (await invoke('queue_click_preferences', { epoch })).value.replaceQueue;

  async function fixture(action) {
    const id = ++actionId;
    window.__vinylSimilarFixtureAck = null;
    await emit('vinyl-similar-fixture', { id, action });
    const reply = await wait(() => window.__vinylSimilarFixtureAck?.id === id && window.__vinylSimilarFixtureAck,
      'native vinyl fixture acknowledgement');
    assert(!reply.error, 'native vinyl fixture rejected ' + action);
    return reply.reply;
  }
  async function pauseAfterPlayback(id) {
    const played = await wait(async () => {
      const value = await player();
      return value.state === 'playing' && value.track?.id === id && value.position > .18 && value;
    }, 'native output of ' + id);
    click(await wait(() => footer('暂停') && !footer('暂停').disabled && footer('暂停'), 'pause control'));
    const paused = await wait(async () => {
      const value = await player();
      return value.state === 'paused' && value.playId === played.playId && value;
    }, 'pause of native play ' + id);
    const list = await queue();
    assert(paused.queueRevision === list.revision && paused.entryId === list.current &&
      list.tracks.find((entry) => entry.entryId === list.current)?.track.id === id,
      'native queue and play agree');
    return { player: paused, queue: list, playedPosition: played.position };
  }
  function retained(before, after) {
    const original = before.queue.tracks.map((entry) => entry.entryId);
    assert(equal(after.queue.tracks.map((entry) => entry.entryId).filter((id) => original.includes(id)),
      original), 'unrelated native queue entries stay in order and keep their identity');
  }
  async function capture(label, native, more = {}) {
    await sleep(420);
    const current = await player();
    const list = await queue();
    assert(current.state === 'paused' && current.playId === native.player.playId &&
      list.revision === native.queue.revision && Math.abs(current.position - native.player.position) < .12,
      'read and navigation preserve paused native transport');
    const stage = 'ux_' + String(++stages).padStart(2, '0') + '_' + label;
    window.__vinylSimilarRecorded = null;
    await emit(stage, {
      playId: current.playId,
      trackId: current.track?.id,
      queueRevision: list.revision,
      playedPosition: native.playedPosition,
      preference: await preference(),
      tab: lyrics()?.dataset.vinylTab ?? null,
      songState: songState() ?? null,
      songIds: songIds(),
      playlistIds: playlistIds(),
      captureHandshake: true,
      ...more,
    });
    await wait(() => window.__vinylSimilarRecorded === stage, 'native stage recorded: ' + stage);
  }
  async function showLyrics() {
    if (!lyrics()) click(footer('歌词'));
    await wait(lyrics, 'standard vinyl display');
  }
  async function showSimilar() {
    await showLyrics();
    if (lyrics().dataset.vinylTab !== 'similar') {
      click(await wait(tab, 'treatment recommendation tab'));
    }
    await wait(() => lyrics()?.dataset.vinylTab === 'similar' && pane(), 'mounted recommendation pane');
  }
  async function songs(ids) {
    await showSimilar();
    await wait(() => songState() === 'ready' && equal(songIds(), ids), 'rows for current seed: ' + ids.join(','));
  }
  async function addPreference() {
    if (lyrics()) {
      click(await wait(() => top('收起播放详情'), 'close vinyl before settings'));
      await wait(() => !lyrics(), 'vinyl closed');
    }
    click(top('打开设置'));
    await wait(() => choice('add') && !choice('add').disabled && !one('.page-leave-active'), 'queue preference mounted');
    one('.queue-click-preferences').scrollIntoView({ block: 'center' });
    assert(choice('replace').checked && (await preference()) === true, 'default replacement preference');
    click(choice('add'));
    await wait(async () => choice('add').checked && !choice('add').disabled && (await preference()) === false,
      'native device add preference saved');
  }
  async function expandPlaylists() {
    await wait(() => playlistIds().length === 6, 'six related playlists before expansion');
    click(one('[data-vinyl-similar-expand]', pane()));
    await wait(() => playlistIds().length === 8, 'all eight related playlists after expansion');
  }

  try {
    ({ epoch } = await wait(async () => {
      const account = await invoke('account_snapshot');
      return account.status === 'authenticated' && account;
    }, 'sealed account ready'));
    await emit('resize', { width: 1000, height: 680 });
    await wait(() => innerWidth === 1000 && innerHeight === 680, 'bounded viewport');
    await wait(() => {
      const item = one('.track-row[data-track-id="1"]');
      const mode = footer('列表循环');
      if (!item || !one('.daily-track-actions') || !mode || mode.disabled) return false;
      doubleClick(item);
      return true;
    }, 'recommendations for real native play');
    const first = await pauseAfterPlayback('1');
    assert(queueIds(first.queue).length > 3 && queueIds(first.queue)[1] === '2', 'independent initial queue');
    await showLyrics();
    await wait(() => lyrics().dataset.vinylTab === 'lyrics' && one('.lyric-line.current'), 'standard lyrics rendered');
    if (window.__vinylSimilarControl) {
      await sleep(600);
      assert(!tab() && !pane(), 'control does not mount the treatment tab or pane');
      await capture('control_lyrics', first);
      await emit('complete', { ok: true, stageCount: stages, realMusicRequests: 0 });
      return;
    }
    await wait(tab, 'treatment tab is visible');
    assert(!pane(), 'inactive recommendation pane has not mounted');
    await capture('before_read', first);

    step = 'read failure and retry';
    await showSimilar();
    await wait(() => songState() === 'error' && one('[data-vinyl-similar-retry="songs"]', pane()), 'song read failure visible');
    if (window.__vinylSimilarRelated) {
      await wait(() => playlistIds().length === 6, 'playlist lane is independent of a song read failure');
    }
    await capture('read_failed', first);
    click(one('[data-vinyl-similar-retry="songs"]', pane()));
    await songs(['51', '52', '53']);
    await capture('read_retried', first);
    if (window.__vinylSimilarRelated) {
      await expandPlaylists();
      await capture('related_first_expanded', first);
    }

    step = 'title queues all displayed rows';
    click(one('[data-vinyl-similar-play-all]', pane()));
    const title = await pauseAfterPlayback('51');
    retained(first, title);
    assert(equal(queueIds(title.queue), ['1', '51', '52', '53', ...queueIds(first.queue).slice(1)]),
      'title inserts the full displayed list after current');
    await songs(['54', '55', '56']);
    await capture('title_queued', title);

    step = 'cover queues only its row';
    click(one('[data-vinyl-similar-track-art]', row('55')));
    const cover = await pauseAfterPlayback('55');
    retained(title, cover);
    assert(equal(queueIds(cover.queue), ['1', '51', '55', ...queueIds(title.queue).slice(2)]),
      'cover inserts only its selected row even with replacement enabled');
    await songs(['57', '58', '59']);
    await capture('cover_queued', cover);

    step = 'double click replacement';
    doubleClick(one('[data-vinyl-similar-track-title]', row('58')));
    const replace = await pauseAfterPlayback('58');
    assert(equal(queueIds(replace.queue), ['57', '58', '59']), 'default double click replaces with displayed order');
    await capture('row_replaced', replace);

    step = 'change the device double-click preference in Settings';
    await addPreference();
    await capture('preference_add', replace);
    await songs(['60', '61', '62']);

    step = 'double click addition and hold the old seed';
    doubleClick(one('[data-vinyl-similar-track-title]', row('61')));
    const add = await pauseAfterPlayback('61');
    retained(replace, add);
    assert(equal(queueIds(add.queue), ['57', '58', '61', '59']), 'add preference keeps unrelated tracks');
    await showSimilar();
    await wait(async () => (await fixture('status')).holding, 'native old seed is held');
    await capture('row_added', add);

    step = 'new play supersedes an unresolved recommendation read';
    click(footer('下一首'));
    const latest = await pauseAfterPlayback('59');
    await songs(['84', '85', '86']);
    await fixture('release');
    await wait(async () => (await fixture('status')).heldReturned, 'old backend response returned');
    await sleep(350);
    assert(songState() === 'ready' && equal(songIds(), ['84', '85', '86']) &&
      !one('[data-vinyl-similar-retry="songs"]', pane()), 'late old response did not replace rows or add an error');
    await capture('late_discarded', latest);

    if (window.__vinylSimilarRelated) {
      step = 'related playlists expand locally and navigate';
      await expandPlaylists(); // The first seed's expansion must not carry over.
      await capture('related_expanded', latest);
      click(one('[data-vinyl-similar-playlist="108"]', pane()));
      await wait(() => !lyrics() && one('.playlist-hero')?.textContent.includes('合成歌单 108'),
        'related card opened the stock playlist route');
      await capture('playlist_route', latest, { openedPlaylist: '108' });
    }
    await emit('complete', { ok: true, stageCount: stages, realMusicRequests: 0 });
  } catch (error) {
    await emit('failed', {
      step, error: String(error), stack: error?.stack, songState: songState(),
      songIds: songIds(), playlistIds: playlistIds(), tab: lyrics()?.dataset.vinylTab,
    });
  }
})();
