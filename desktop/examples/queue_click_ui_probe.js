// Production UI + native queue/preferences, isolated synthetic API and null audio.
(async () => {
  const one = (selector, root = document) => root.querySelector(selector);
  const button = (name, root = document) =>
    [...root.querySelectorAll('button')].find(
      (node) => node.getAttribute('aria-label') === name || node.textContent.trim() === name,
    );
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const emit = (stage, value = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...value } });
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (ok, error) => { if (!ok) throw Error(error); };
  async function wait(check, error) {
    for (let i = 0; i < 240; i++) {
      if (await check()) return;
      await sleep(40);
    }
    throw Error(error);
  }
  const click = (node) => {
    assert(node && !node.disabled, 'available button');
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    node.click();
  };
  const row = (id, root = document) => one('.track-row[data-track-id="' + id + '"]', root);
  const doubleClick = (id, root = document) => {
    const title = row(id, root)?.querySelector('.track-title');
    assert(title, 'rendered song row ' + id);
    title.dispatchEvent(new MouseEvent('dblclick', { bubbles: true, cancelable: true, detail: 2 }));
  };
  let step = 'startup', epoch, count = 0;
  const queue = () => invoke('player_queue', { epoch });
  const settings = () => invoke('queue_click_preferences', { epoch });
  const bar = () => one('.playerbar');
  const ids = (value) => value.tracks.map((entry) => entry.track.id);
  const selected = (value) => value.tracks.find((entry) => entry.entryId === value.current)?.track.id;
  const choice = (value) => one('[data-queue-click-choice="' + value + '"]');
  const settled = () => !one('.page-enter-active,.page-leave-active');
  async function waitPlayback(id, length) {
    let settled;
    await wait(async () => {
      if (bar()?.dataset.state !== 'playing') return false;
      const value = await queue();
      const player = await invoke('player_snapshot', { epoch });
      if (value.tracks.length !== length || selected(value) !== id ||
          player.state !== 'playing' || player.track?.id !== id ||
          player.queueRevision !== value.revision || player.queueLen !== length ||
          player.entryId !== value.current ||
          player.playId !== bar()?.dataset.playId || !(player.position > .2)) return false;
      settled = { queue: value, playId: player.playId, position: player.position };
      return true;
    }, step);
    return settled;
  }
  async function stage(label, settled) {
    const name = 'ux_' + String(++count).padStart(2, '0') + '_' + label;
    window.__queueClickRecorded = null;
    await emit(name, {
      preference: (await settings()).value.replaceQueue,
      ...(settled && { observedPlayback: {
        playId: settled.playId, queueRevision: settled.queue.revision,
        entryId: settled.queue.current, position: settled.position,
      } }),
      // Deliberately deschedule the native observer while the next action is
      // ready to run. The probe must still save this stage before advancing.
      ...(label === 'catalog_added' && { deferQueueClickObservation: true }),
    });
    await wait(() => window.__queueClickRecorded === name, name + ' native evidence saved');
  }
  async function openSettings() {
    click(button('打开设置'));
    await wait(() => settled() && choice('replace') && !choice('replace').matches(':disabled'), 'settings');
    one('.queue-click-preferences').scrollIntoView({ block: 'center' });
    await wait(() => ['replace', 'add'].every((value) => {
      const control = choice(value);
      const r = control.getBoundingClientRect();
      return document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2) === control;
    }), 'both setting controls in view');
  }
  async function openPlaylist() {
    click(button('我的歌单'));
    await wait(() => settled() && one('[data-library-open="2"]'), 'playlist library');
    click(one('[data-library-open="2"]'));
    await wait(() => settled() && one('.playlist-hero') && row('3'), 'playlist detail');
  }
  async function seed() {
    const value = await invoke('play_selection', {
      epoch, selection: { kind: 'songs', ids: ['51', '52'], index: 0, quality: 'standard' },
    });
    assert(value.queueLen === 2, 'two distinct baseline queue entries');
    return waitPlayback('51', 2);
  }
  function retained(before, after) {
    const oldEntries = before.tracks.map((entry) => entry.entryId);
    assert(
      after.tracks.map((entry) => entry.entryId).filter((id) => oldEntries.includes(id)).join('|') ===
        oldEntries.join('|'),
      'previous queue entries and relative order kept',
    );
  }
  try {
    await wait(() => row('3') && !row('3').querySelector('.track-play')?.disabled, 'initial playback');
    epoch = (await invoke('account_snapshot')).epoch;
    await openSettings();
    if (window.__queueClickReopened) {
      assert(choice('add').checked && !choice('replace').checked, 'persisted UI preference after restart');
      assert((await settings()).value.replaceQueue === false, 'persisted native preference after restart');
      await stage('reopened_add');
      await emit('complete', { ok: true, count, reopened: true, realMusicRequests: 0 });
      return;
    }
    assert(choice('replace').checked && !choice('add').checked, 'official replace default');
    await stage('default_setting');

    await seed();
    await openPlaylist();
    step = 'playlist replaces by default';
    doubleClick('3');
    const replaced = await waitPlayback('3', 40);
    assert(!ids(replaced.queue).includes('51'), 'default replaces old source list');
    await stage('default_replaced', replaced);

    await openSettings();
    click(choice('add'));
    await wait(() => choice('add').checked && !choice('add').matches(':disabled'), 'native setting accepted');
    assert((await settings()).value.replaceQueue === false, 'native add preference saved');
    await stage('add_setting');
    await openPlaylist();
    const before = await seed();
    step = 'playlist adds selected song and keeps queue';
    doubleClick('3');
    const appended = await waitPlayback('3', 3);
    retained(before.queue, appended.queue);
    await stage('playlist_added', appended);

    const original = appended.queue.current;
    const first = appended.queue.tracks.find((entry) => entry.track.id === '51').entryId;
    await invoke('player_action', { epoch, action: { kind: 'select', entry_id: first } });
    await waitPlayback('51', 3);
    step = 'duplicate song is selected without insertion';
    doubleClick('3');
    const duplicate = await waitPlayback('3', 3);
    assert(duplicate.queue.current === original && ids(duplicate.queue).filter((id) => id === '3').length === 1,
      'reuse original queue entry');
    await stage('duplicate_selected', duplicate);

    step = 'open the exact album search result';
    const input = one('[aria-label="搜索音乐"]');
    input.focus();
    input.value = '合成专辑';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    input.form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await wait(() => one('[data-category="albums"]'), 'search category');
    click(one('[data-category="albums"]'));
    const albumAction = () => one(
      '#search-result-panel[aria-labelledby="search-tab-albums"][aria-busy="false"]:not([inert]) ' +
      '[data-result-key="album:201"] .search-card-open:not(:disabled)',
    );
    await wait(() => one('[data-category="albums"][aria-selected="true"]') && albumAction(),
      'loaded Albums tab contains synthetic album 201');
    click(albumAction());
    step = 'open the selected album tracks';
    const albumPage = () => one('.catalog-page[aria-label="专辑详情"]');
    const albumSongs = () => albumPage()?.querySelector('[data-album-panel="songs"]:not([inert])');
    await wait(() => settled() && albumPage()?.querySelector('h1')?.textContent.trim() === '合成专辑 201' &&
      albumSongs() && row('4', albumSongs()), 'selected album 201 shows song 4');
    step = 'catalog double click adds just selected song';
    doubleClick('4', albumSongs());
    const catalog = await waitPlayback('4', 4);
    retained(duplicate.queue, catalog.queue);
    await stage('catalog_added', catalog);

    step = 'explicit album Play All replaces independent of add preference';
    click(button('播放全部', albumPage()));
    const all = await waitPlayback('1', 11);
    assert(!ids(all.queue).includes('51') && (await settings()).value.replaceQueue === false,
      'explicit Play All replaces; preference remains enabled');
    await stage('explicit_replace', all);
    await emit('complete', { ok: true, count, reopened: false, realMusicRequests: 0 });
  } catch (error) {
    await emit('failed', { ok: false, step, message: String(error) });
  }
})();
