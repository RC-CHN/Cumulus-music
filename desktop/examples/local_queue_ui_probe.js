// Real library, guest player and native queue with synthetic files only.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const button = (label) => [...document.querySelectorAll('button')].find(
    (item) => item.getAttribute('aria-label') === label || item.textContent.trim() === label,
  );
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, value = {}) => invoke('plugin:event|emit', { event: 'local-proof', payload: { stage, ...value } });
  const assert = (value, label) => { if (!value) throw Error(label); };
  async function wait(test, label) {
    for (let i = 0; i < 240; i++) {
      const value = await test();
      if (value) return value;
      await new Promise((done) => setTimeout(done, 100));
    }
    throw Error(label);
  }
  function click(item) {
    assert(item && !item.disabled, 'missing or disabled control');
    item.focus({ preventScroll: true });
    item.click();
  }
  async function capture(stage, value = {}) {
    await emit(stage, value);
    if (window.__localProof.capture) await new Promise(done => setTimeout(done, value.paintSamples ? 3400 : 700));
  }
  async function filter(word, count) {
    const input = one('[aria-label="搜索本地音乐"]');
    input.value = word;
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await wait(() => one('.local-page')?.dataset.localLoading === 'false' &&
      document.querySelectorAll('.local-tracks tbody tr').length === count, 'complete filter');
  }
  async function add() {
    click(button('更多本地音乐操作'));
    await wait(() => button('添加全部至播放列表') && !button('添加全部至播放列表').disabled, 'enabled queue action');
    click(button('添加全部至播放列表'));
    await wait(() => one('.local-page .notice')?.textContent.includes('已添加至播放列表') &&
      !one('.action-menu'), 'local queue accepted');
    assert(document.activeElement === button('更多本地音乐操作'), 'queue menu restores focus');
  }
  try {
    await wait(() => one('.welcome'), 'guest startup');
    const { epoch } = await wait(async () => {
      const state = await invoke('account_snapshot');
      return ['signed_out', 'expired'].includes(state.status) && state;
    }, 'settled guest snapshot');
    click(button('本地音乐'));
    await wait(() => one('.local-page'), 'local page');
    await invoke('local_command', { command: { kind: 'watch', path: window.__localProof.root + '/media' } });
    const trackRows = () => [...document.querySelectorAll('.local-tracks tbody tr')];
    await wait(() => trackRows().length === 5, 'native file scan');
    const sorted = () => [...document.querySelectorAll('.local-tracks th[aria-sort]')];
    const direction = () => sorted()[0]?.getAttribute('aria-sort');
    const durations = () => trackRows().map(row => row.cells[5].textContent.trim());
    assert(sorted().length === 1 && sorted()[0].textContent.includes('音乐标题'), 'default active sort');
    const search = one('[aria-label="搜索本地音乐"]');
    search.value = '夜行';
    search.dispatchEvent(new Event('input', { bubbles: true }));
    click(button('时长'));
    await wait(() => direction() === 'ascending' && durations().join(',') === '00:30,01:30' &&
      search.value === '夜行', 'sort preserves an input still inside the search debounce');
    await filter('', 5);
    click(button('音乐标题'));
    await wait(() => sorted().length === 1 && sorted()[0].textContent.includes('音乐标题') &&
      trackRows().length === 5, 'restore initial header before sort regression');
    click(button('时长'));
    await wait(() => direction() === 'ascending' && durations().join(',') === '00:30,01:00,01:30,02:00,02:00',
      'native duration ascending and active header');
    const durationHeader = sorted()[0];
    assert(durationHeader.querySelector('[aria-hidden=true]')?.textContent.includes('↑'), 'visible ascending arrow');
    click(durationHeader.querySelector('button'));
    await wait(() => direction() === 'descending' && durations().join(',') === '02:00,02:00,01:30,01:00,00:30',
      'native duration descending and active header');
    assert(sorted().length === 1 && sorted()[0].querySelector('[aria-hidden=true]')?.textContent.includes('↓') &&
      document.activeElement === sorted()[0].querySelector('button'), 'direction and keyboard focus');
    click(button('音乐标题'));
    await wait(() => sorted().length === 1 && direction() === 'ascending' &&
      sorted()[0].textContent.includes('音乐标题') && trackRows().length === 5, 'return to title order');
    const groupTab = label => [...document.querySelectorAll('.local-tabs button')]
      .find(node => node.textContent.trim() === label);
    const namedGroups = () => [...document.querySelectorAll('.local-groups [data-local-group-key]')];
    for (const label of ['歌手', '专辑']) {
      click(await wait(() => groupTab(label), 'group tab: ' + label));
      await wait(() => namedGroups().map(node => node.title).join(',') === '北京,上海,其他',
        'pinyin order and trailing unnamed group: ' + label);
      const shanghai = await wait(() => namedGroups().find(node => node.title === '上海'), 'Shanghai group: ' + label);
      const groupedSearch = one('[aria-label="搜索本地音乐"]');
      groupedSearch.value = '归途';
      groupedSearch.dispatchEvent(new Event('input', { bubbles: true }));
      click(shanghai);
      await wait(() => trackRows().length === 1 && trackRows()[0].textContent.includes('归途') &&
        groupedSearch.value === '归途' && document.activeElement === namedGroups().find(node => node.title === '上海'),
        'group selection preserves native identity, pending search and focus: ' + label);
      await filter('', 1);
      const allGroups = () => one('[data-local-all-groups]');
      click(await wait(allGroups, 'all groups control: ' + label));
      await wait(() => trackRows().length === 5 && document.activeElement === allGroups(),
        'all groups restores focus after native read: ' + label);
    }
    click(button('文件夹'));
    const toggle = () => one('.local-folder-toggle');
    const nested = () => [...document.querySelectorAll('[data-local-folder-key]')]
      .find(node => node.title.endsWith('Bonus'));
    await wait(() => toggle() && trackRows().length === 5, 'folder groups');
    assert(toggle().getAttribute('aria-expanded') === 'false' && !nested(), 'nested folder starts collapsed');
    const parentFolder = () => toggle()?.closest('.local-folder-row')?.querySelector('[data-local-folder-key]');
    const folderKey = key => document.activeElement.dispatchEvent(new KeyboardEvent('keydown', {
      key, bubbles: true, cancelable: true,
    }));
    parentFolder().focus();
    folderKey('ArrowRight');
    await wait(() => nested() && document.activeElement === parentFolder(), 'right expands branch without selecting');
    folderKey('ArrowRight');
    await wait(() => document.activeElement === nested(), 'right focuses first child');
    folderKey('ArrowLeft');
    await wait(() => document.activeElement === parentFolder(), 'left returns to parent');
    folderKey('ArrowLeft');
    await wait(() => !nested() && toggle().getAttribute('aria-expanded') === 'false' &&
      trackRows().length === 5, 'left collapses without changing the native selection');
    click(toggle());
    assert(document.activeElement === toggle(), 'disclosure retains focus');
    await wait(() => nested() && toggle().getAttribute('aria-expanded') === 'true', 'nested folder expanded');
    assert(trackRows().length === 5, 'disclosure does not filter songs');
    click(nested());
    await wait(() => nested()?.getAttribute('aria-pressed') === 'true' &&
      document.activeElement === nested() && trackRows().length === 1 &&
      trackRows()[0].textContent.includes('Bonus Track'), 'exact nested folder and restored focus');
    click(toggle());
    await wait(() => !nested() && toggle()?.getAttribute('aria-expanded') === 'false', 'nested folder collapsed');
    assert(trackRows().length === 1, 'collapse retains the selected folder query');
    await filter('Bonus', 1);
    await wait(() => nested() && !toggle(), 'search presents a flat folder result');
    click(button('歌曲'));
    await wait(() => trackRows().length === 5, 'songs tab resets folder filter');
    await filter('夜行 · 下篇', 1);
    await add();
    await wait(() => one('.playerbar [aria-label="暂停"]') && Number(one('[aria-label="播放进度"]')?.value) > 0.1, 'empty queue starts native CUE');
    const before = await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    assert(before.queueLen === 1 && before.timeline.mode === 'cue', 'one local item playing');
    await emit('local_queue_started', { queue: before.queueLen, cue: true });
    await filter('夜行', 2);
    await add();
    await filter('雨声', 1);
    await add();
    const after = await invoke('player_snapshot', { epoch });
    const queue = await invoke('player_queue', { epoch });
    const names = queue.tracks.map((entry) => entry.track.name);
    assert(names.join(',') === '夜行 · 下篇,雨声,夜行 · 上篇', 'insert after the current song, preserve prior suffix');
    assert(queue.tracks.every((entry) => entry.track.local), 'all rows retain offline identity');
    assert(after.playId === before.playId && after.entryId === before.entryId &&
      after.state === before.state && Math.abs(after.position - before.position) < 0.05, 'retain paused playback');
    click(button('文件夹'));
    await wait(() => toggle() && trackRows().length === 5, 'folder groups for context action');
    if (!nested()) click(toggle());
    const origin = await wait(nested, 'nested context origin');
    const groupMenu = () => one('.action-menu[aria-label="本地分组操作"]');
    const choice = label => [...(groupMenu()?.querySelectorAll('button') ?? [])]
      .find(item => item.textContent.trim() === label);
    origin.focus();
    origin.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2 }));
    await wait(() => choice('下一首播放') && !choice('下一首播放').disabled, 'group next menu');
    click(choice('下一首播放'));
    await wait(async () => (await invoke('player_queue', { epoch })).tracks
      .map(entry => entry.track.name).join(',') === '夜行 · 下篇,Bonus Track,雨声,夜行 · 上篇', 'next queues only the clicked folder');
    const grouped = await invoke('player_snapshot', { epoch });
    assert(grouped.playId === before.playId && grouped.state === before.state &&
      Math.abs(grouped.position - before.position) < 0.05, 'group next retains playback');
    assert(origin.getAttribute('aria-pressed') === 'false' && trackRows().length === 5, 'context action does not select the group');
    assert(document.activeElement === origin, 'next restores menu origin focus');

    const rowTrigger = (name) => button(`更多操作 ${name}`);
    const rowMenu = () => one('.action-menu[aria-label="本地歌曲操作"]');
    const rowAction = (label) => [...(rowMenu()?.querySelectorAll('button') ?? [])]
      .find(item => item.textContent.trim() === label);
    const upper = rowTrigger('夜行 · 上篇');
    const rowContext = (trigger) => {
      trigger.focus();
      trigger.dispatchEvent(new KeyboardEvent('keydown', {key:'F10', shiftKey:true, bubbles:true, cancelable:true}));
    };
    rowContext(upper);
    await wait(() => document.activeElement === rowAction('播放') && !rowAction('下一首播放').disabled, 'track menu focuses the first action');
    await capture('ux_01_local_track_menu');
    click(rowAction('下一首播放'));
    await wait(async () => (await invoke('player_queue', { epoch })).tracks
      .map(entry => entry.track.name).join(',') === '夜行 · 下篇,夜行 · 上篇,Bonus Track,雨声', 'next moves exactly the chosen CUE segment');
    const queuedTrack = await invoke('player_snapshot', { epoch });
    assert(queuedTrack.playId === before.playId && queuedTrack.entryId === before.entryId &&
      queuedTrack.state === before.state && Math.abs(queuedTrack.position - before.position) < .05,
      'track next leaves the paused segment untouched');
    assert(document.activeElement === upper && trackRows().length === 5, 'track next retains row focus and filter');

    const contextKey = () => origin.dispatchEvent(new KeyboardEvent('keydown', {
      key: 'F10', shiftKey: true, bubbles: true, cancelable: true,
    }));
    contextKey();
    await wait(() => document.activeElement === choice('播放'), 'keyboard context menu focus');
    document.activeElement.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    await wait(() => origin.getAttribute('aria-expanded') === 'false' &&
      document.activeElement === origin, 'keyboard menu returns focus');
    contextKey();
    await wait(() => choice('播放') && !choice('播放').disabled, 'group play menu');
    click(choice('播放'));
    await wait(async () => {
      const result = await invoke('player_snapshot', { epoch });
      return result.state === 'playing' && result.queueLen === 1 && result.track?.name === 'Bonus Track';
    }, 'play replaces queue with clicked folder only');
    click(button('歌曲'));
    await filter('夜行', 2);
    const filtered = rowTrigger('夜行 · 上篇');
    click(filtered);
    await wait(() => rowAction('播放') && !rowAction('播放').disabled, 'track play menu');
    click(rowAction('播放'));
    await wait(async () => {
      const result = await invoke('player_snapshot', { epoch });
      return result.state === 'playing' && result.queueLen === 2 && result.track?.name === '夜行 · 上篇' && result.timeline?.mode === 'cue';
    }, 'track play starts chosen segment in the full filter');
    await capture('ux_02_local_track_playing');
    assert(document.documentElement.scrollWidth <= innerWidth &&
      one('.local-page').scrollWidth <= one('.local-page').clientWidth + 1, 'no horizontal clipping');
    let dropQueue = null;
    if (innerWidth >= 980) {
      const held = await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
      await filter('雨声', 1);
      click(button('播放队列'));
      const viewport = await wait(() => one('.queue-scroll') &&
        document.querySelectorAll('.queue-row').length === 2 && one('.queue-scroll'), 'queue drop destination');
      await new Promise(done => setTimeout(done, 320));
      const pointer = (target, kind, x, y) => target.dispatchEvent(new PointerEvent(kind, {
        bubbles: true, cancelable: true, pointerId: 57, pointerType: 'mouse', button: 0,
        buttons: kind === 'pointerup' ? 0 : 1, clientX: x, clientY: y,
      }));
      function startDrop(slot) {
        const title = one('.local-tracks tbody .local-title');
        const source = title.getBoundingClientRect();
        const x = source.left + 4, y = source.top + source.height / 2;
        assert(document.elementFromPoint(x, y)?.closest('.local-title') === title, 'visible source is reachable by pointer');
        const bounds = viewport.getBoundingClientRect();
        const targetX = bounds.left + bounds.width / 2, targetY = bounds.top + 8 + slot * 64;
        pointer(title, 'pointerdown', x, y);
        pointer(viewport, 'pointermove', targetX, targetY);
        return (backToSource = false) => {
          if (backToSource) {
            pointer(title, 'pointerup', x, y);
            title.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
          } else pointer(viewport, 'pointerup', targetX, targetY);
        };
      }
      const old = await invoke('player_queue', { epoch });
      const aborted = startDrop(1);
      const marker = await wait(() => {
        const node = one('.queue-drop-marker');
        return node?.style.top === '64px' && one('.queue-drag-hint')?.textContent.includes('添加本地歌曲') && node;
      }, 'drop insertion line between the two displayed rows');
      const markerRect = marker.getBoundingClientRect();
      await capture('ux_03_local_queue_drop', {
        insertBefore: 1, noTransfer: true, screenX, screenY, ratio: devicePixelRatio,
        paintSamples: [{ x: markerRect.left + 8, y: markerRect.top + 1, color: getComputedStyle(marker).backgroundColor }],
      });
      viewport.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
      aborted(true);
      await wait(() => !one('.queue-drop-marker') && one('.queue-scroll'), 'Escape clears only drag, leaves queue open');
      assert((await invoke('player_queue', { epoch })).revision === old.revision, 'cancel does not mutate queue');
      assert((await invoke('player_snapshot', { epoch })).playId === held.playId, 'canceled release does not play the source');
      const external = new Event('drop', { bubbles: true, cancelable: true });
      Object.defineProperty(external, 'dataTransfer', {
        get() { throw Error('queue must not inspect an external drop payload'); },
      });
      viewport.dispatchEvent(external);
      assert((await invoke('player_queue', { epoch })).revision === old.revision, 'external drop does not mutate queue');
      const drop = startDrop(0);
      await wait(() => one('.queue-drop-marker')?.style.top === '0px', 'explicit insertion boundary before first row');
      drop();
      await wait(async () => (await invoke('player_queue', { epoch })).tracks.map(row => row.track.name).join(',') ===
        '雨声,夜行 · 上篇,夜行 · 下篇', 'drop inserts at the pointer boundary rather than next after current');
      await wait(() => document.querySelectorAll('.queue-row').length === 3 && !one('.queue-drop-marker'), 'updated visible queue');
      await filter('夜行 · 上篇', 1);
      await wait(() => one('.local-tracks tbody .local-title')?.textContent.includes('夜行 · 上篇'), 'active duplicate source is indexed');
      startDrop(3)();
      dropQueue = await wait(async () => {
        const state = await invoke('player_queue', { epoch });
        const found = state.tracks.map(row => row.track.name);
        return found.join(',') === '雨声,夜行 · 下篇,夜行 · 上篇' && found;
      }, 'drop moves the active duplicate to the tail without a duplicate');
      const final = await invoke('player_snapshot', { epoch });
      assert(final.playId === held.playId && final.entryId === held.entryId && final.state === 'paused' &&
        Math.abs(final.position - held.position) < .05, 'drop retains the same native paused CUE clock');
    }
    await emit('complete', { ok: true, queue: names, dropQueue, retainedPlayback: true, folderTree: true, groupMenu: true, trackMenu: true, localSort: true, groupPinyinOrder: true, realMusicRequests: 0, audioOutput: 'null', width: innerWidth });
  } catch (error) {
    await emit('failed', { message: String(error), stack: error?.stack });
  }
})();
