// Closed synthetic account and local audio; no playlist mutation or download submit.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, extra = {}) => invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...extra } });
  const assert = (condition, reason) => { if (!condition) throw Error(reason); };
  async function wait(test, label) {
    for (let n = 0; n < 160; n++) { if (test()) return; await sleep(80); }
    throw Error(label);
  }
  function click(node) {
    assert(node && !node.disabled, 'control unavailable');
    node.focus({ preventScroll: true });
    node.click();
  }
  const key = (node, code, shiftKey = false) => node.dispatchEvent(new KeyboardEvent('keydown', { key: code, shiftKey, bubbles: true, cancelable: true }));
  const button = (text, root = document) => [...root.querySelectorAll('button')].find((node) => node.getAttribute('aria-label') === text || node.textContent.trim() === text);
  const anchor = (id) => one(`[data-essential-focus="playlists:${id}:open"]`);
  const rail = () => one('[data-essential-rail="playlists"]');
  const menu = () => one('.action-menu[role="menu"]:not([inert])');
  const item = (name) => menu()?.querySelector(`[data-menu-key="${name}"]`);
  const dialog = (kind) => one(`.${kind}[open]:not([inert])`);
  const state = [];
  let downloads = 0, collectionWrites = 0;
  const clipboard = [], reads = [];
  const fetchIpc = window.fetch;
  window.fetch = function(url, options) {
    const path = String(url);
    if (path.endsWith('/copy_text')) clipboard.push(JSON.parse(options.body).text);
    if (path.endsWith('/download_command')) downloads++;
    if (path.endsWith('/mutate_playlist')) collectionWrites++;
    if (path.endsWith('/playlist_page')) {
      const { id, page } = JSON.parse(options.body);
      reads.push([id, page.offset, page.limit, page.revision]);
    }
    return fetchIpc.call(window, url, options);
  };
  async function capture(name, extra = {}) {
    const stage = 'ux_' + name;
    await sleep(80);
    await wait(() => all('.page-view,.action-menu,dialog,.playlist-action-feedback').every((node) =>
      node.getAnimations().every((animation) => !animation.pending && animation.playState !== 'running')),
    'settled card/menu/dialog animation');
    await sleep(170);
    if (name === 'download_confirmation')
      assert(!one('.playlist-download-read') && dialog('download-dialog')?.querySelector('h2')?.textContent.trim() === '下载 105 首歌曲',
        'only the finished confirmation is visible');
    await emit(stage, { ...extra });
    await wait(() => window.__CUMULUS_ALBUM_CAPTURE_ACK__ === stage, 'capture ' + stage);
    state.push(name);
  }
  async function openMenu(id, gesture = 'keyboard') {
    const opener = anchor(id);
    assert(opener, 'playlist card ' + id);
    opener.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'instant' });
    await sleep(160);
    if (gesture === 'pointer') {
      const r = opener.getBoundingClientRect();
      opener.dispatchEvent(new MouseEvent('contextmenu', { button: 2, clientX: r.left + 2, clientY: r.top + 2, bubbles: true, cancelable: true }));
    } else {
      opener.focus({ preventScroll: true });
      key(opener, gesture === 'menu-key' ? 'ContextMenu' : 'F10', gesture === 'keyboard');
    }
    await wait(() => item('view') && menu().contains(document.activeElement), 'focused playlist context menu');
    const bounds = menu().getBoundingClientRect();
    assert(bounds.left >= 0 && bounds.top >= 0 && bounds.right <= innerWidth && bounds.bottom <= innerHeight,
      'menu contained in window');
    return opener;
  }
  async function closeMenu(opener) {
    key(document.activeElement, 'Escape');
    await wait(() => !menu() && document.activeElement === opener, 'menu closes onto exact card');
  }
  async function closeDialog(kind, label, opener) {
    click(button(label, dialog(kind)));
    await wait(() => !one('.' + kind) && document.activeElement === opener, kind + ' restores exact focus');
  }
  let sequence = 0;
  async function fixture(mode) {
    const id = ++sequence;
    await emit('essential-fixture', { id, owner: 2, mode });
    await wait(() => window.__essentialAck === id, 'fixture acknowledgement');
  }
  try {
    await wait(() => all('.sidebar button').some((node) => !node.disabled && node.textContent.trim() === '精选'), 'choice ready');
    click(all('.sidebar button').find((node) => node.textContent.trim() === '精选'));
    await wait(() => anchor(501), 'official playlist cards');
    const opener = await openMenu(501, 'pointer');
    assert(document.activeElement === item('view'), 'official first item receives keyboard focus');
    assert(all('.action-menu [data-menu-key]').map((node) => node.dataset.menuKey).join(',') ===
      'view,play,next,subscribe,share,copy,download', 'ordinary card menu order and omissions');
    await capture('context_menu');
    await closeMenu(opener);

    const scrolledOpener = await openMenu(505, 'menu-key');
    assert(!item('subscribe'), 'existing collection hides favorite');
    const left = rail().scrollLeft;
    assert(left > 0, 'detail starts from a scrolled rail');
    click(item('view'));
    await wait(() => one('.playlist-hero h1')?.textContent.includes('合成官方歌单 505'), 'native playlist detail');
    click(button('返回上一页'));
    await wait(() => anchor(505) && document.activeElement === anchor(505), 'detail Back restores exact card');
    assert(Math.abs(rail().scrollLeft - left) <= 1 && !scrolledOpener.isConnected, 'detail Back restores original horizontal scroll');
    await capture('detail_back');

    await openMenu(501);
    click(item('share'));
    const link = 'https://music.163.com/#/playlist?id=501';
    await wait(() => dialog('track-share-dialog')?.querySelector('input')?.value === link, 'canonical playlist share confirmation');
    await closeDialog('track-share-dialog', '关闭分享', anchor(501));
    await openMenu(501);
    click(item('copy'));
    await wait(() => one('.playlist-action-feedback')?.textContent.includes('链接已复制'), 'native clipboard acknowledgment');
    assert(clipboard.join(',') === link && document.activeElement === anchor(501), 'canonical copy preserves focus');
    await openMenu(501);
    click(item('subscribe'));
    await wait(() => dialog('playlist-mutation-dialog')?.textContent.includes('收藏「合成官方歌单 501」'), 'explicit collection confirmation');
    await capture('link_and_collection');
    await closeDialog('playlist-mutation-dialog', '关闭歌单确认', anchor(501));
    assert(collectionWrites === 0, 'collection cancellation performs no writes');

    for (const id of [502, 503, 504, 506]) {
      const current = await openMenu(id);
      assert(item('share').disabled === (id !== 504) && item('copy').disabled === (id !== 504), 'link availability follows public and empty metadata');
      assert(item('play').disabled === (id === 502) && item('next').disabled === (id === 502), 'empty differs from unknown');
      assert(!item('download').disabled, 'ordinary card still shows download when empty');
      if (id === 502) {
        assert(one('[data-essential-focus="playlists:502:play"]').disabled, 'empty card cover cannot play');
        await capture('empty_and_private');
        click(item('download'));
        await wait(() => dialog('playlist-download-read')?.textContent.includes('歌单中没有可下载的在线歌曲'), 'empty download reports no online tracks');
        await closeDialog('playlist-download-read', '关闭歌单下载', current);
      } else {
        await closeMenu(current);
      }
    }

    const { epoch } = await invoke('account_snapshot');
    const song = () => one('[data-essential-focus="songs:2:play"]');
    song().scrollIntoView({ block: 'center' });
    click(song());
    await wait(() => song().getAttribute('aria-label').startsWith('暂停'), 'existing song starts');
    await sleep(600);
    click(song());
    await wait(() => song().getAttribute('aria-label').startsWith('播放'), 'existing song pauses');
    const paused = await invoke('player_snapshot', { epoch });
    const before = await invoke('player_queue', { epoch });
    assert(paused.state === 'paused' && before.tracks.length === 9, 'native queue before insert');
    const nextOrigin = await openMenu(501);
    click(item('next'));
    await wait(() => one('.playlist-action-feedback')?.textContent.includes('已添加至播放列表'), 'queue insertion feedback');
    const after = await invoke('player_snapshot', { epoch });
    const queue = await invoke('player_queue', { epoch });
    assert(after.playId === paused.playId && after.state === 'paused' && Math.abs(after.position - paused.position) < .05,
      'next preserves exact paused decoder');
    assert(queue.tracks.length === 114 && queue.tracks.slice(2,107).every((row,i) => row.track.id === String(1000 + i)),
      'native tracks insert after the current song in authoritative order');
    assert(queue.tracks.slice(0,2).concat(queue.tracks.slice(107)).every((row,i) => row.entryId === before.tracks[i].entryId),
      'original queue entries survive without reconstruction');
    assert(document.activeElement === nextOrigin, 'queue action preserves focus');
    await capture('native_next', { retainedSongs: before.tracks.length });

    await fixture('detail-down');
    const downloadOrigin = await openMenu(501);
    click(item('download'));
    await wait(() => dialog('playlist-download-read')?.querySelector('[role="alert"]') && button('重新读取', dialog('playlist-download-read')), 'download failure is recoverable');
    await capture('download_retry');
    await fixture('initial');
    click(button('重新读取', dialog('playlist-download-read')));
    await wait(() => dialog('download-dialog')?.querySelector('h2')?.textContent.trim() === '下载 105 首歌曲', 'confirmation includes the second authoritative page');
    const batch = reads.filter(([id]) => id === '501').slice(-3);
    assert(batch.map((row) => row[1]).join(',') === '0,0,100' && batch[2][3], 'retry uses revision-bound paging');
    assert(downloads === 0 && collectionWrites === 0, 'confirmation has not submitted any command');
    await capture('download_confirmation', { trackCount: 105 });
    await closeDialog('download-dialog', '关闭下载对话框', downloadOrigin);
    const afterDownload = await invoke('player_snapshot', { epoch });
    assert(afterDownload.playId === after.playId && afterDownload.queueRevision === after.queueRevision,
      'download cancellation never changes playback');
    await capture('download_cancel', { downloadedCount: 105 });

    await openMenu(501);
    click(item('play'));
    let playing;
    for (let n = 0; n < 100; n++) {
      playing = await invoke('player_snapshot', { epoch });
      if (playing.origin?.id === '501' && playing.state === 'playing' && playing.position > .2) break;
      await sleep(80);
    }
    assert(playing.origin?.id === '501' && playing.state === 'playing' && playing.queueLen === 105 && playing.position > .2, 'menu Play replaces the queue with native playlist playback');
    await capture('native_play', { playing: true });
    await emit('complete', { ok: true, states: state, downloads, collectionWrites, realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', { ok: false, error: String(error), states: state });
  }
})();
