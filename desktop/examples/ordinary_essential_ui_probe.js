(async () => {
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const all = (selector) => [...document.querySelectorAll(selector)];
  const one = (selector) => document.querySelector(selector);
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, extra = {}) => invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...extra } });
  const assert = (condition, message) => { if (!condition) throw Error(message); };
  async function wait(test, label) {
    for (let n = 0; n < 140; n++) { if (test()) return; await sleep(100); }
    throw Error(label);
  }
  const click = (node) => {
    assert(node && !node.disabled, 'missing or disabled control');
    node.focus({ preventScroll: true });
    node.click();
  };
  const choice = () => all('.sidebar button').find((node) => node.textContent.trim() === '精选');
  const back = () => one('[aria-label="返回上一页"]');
  const button = (key) => one(`[data-essential-focus="${key}"]`);
  const states = [];
  async function capture(stage, extra = {}) {
    const name = 'ux_' + stage;
    assert(document.documentElement.scrollWidth <= innerWidth, 'application must not scroll horizontally');
    await emit(name, { state: stage, ...extra });
    await wait(() => window.__CUMULUS_ALBUM_CAPTURE_ACK__ === name, 'capture ' + stage);
    states.push(stage);
  }
  let sequence = 0;
  async function fixture(owner, mode) {
    const id = ++sequence;
    await emit('essential-fixture', { id, owner, mode });
    await wait(() => window.__essentialAck === id, 'fixture ' + mode);
  }
  const bookSelect = () => one('[aria-label="有声书分类"]');
  function select(value) {
    const element = bookSelect();
    element.value = String(value);
    element.dispatchEvent(new Event('change', { bubbles: true }));
  }
  async function reloadAt(name) {
    sessionStorage.setItem('essential-phase', name);
    sessionStorage.setItem('essential-states', JSON.stringify(states));
    location.reload();
  }
  try {
    states.push(...JSON.parse(sessionStorage.getItem('essential-states') ?? '[]'));
    const phase = sessionStorage.getItem('essential-phase');
    await wait(() => choice() && !choice().disabled, 'initial account ready');
    click(choice());
    if (phase === 'guest') {
      await wait(() => one('.ordinary-essential')?.textContent.includes('登录后查看精选内容'), 'guest gate');
      await capture('guest');
      await emit('complete', { ok: true, states, realMusicRequests: 0, realAccountWrites: 0 });
      return;
    }
    if (phase === 'other-account') {
      await wait(() => one('.ordinary-essential [role=status]')?.textContent.includes('暂时无法加载'), 'failure screen');
      assert(!one('[data-essential-rail]'), 'other account must not display previous cards');
      await capture('other_account_failure');
      await fixture(3, 'initial');
      click(all('.ordinary-essential button').find((node) => node.textContent.trim() === '重试'));
      await wait(() => button('songs:1:play')?.getAttribute('aria-label')?.includes('精选新歌 3'), 'retry loads only current account');
      await capture('retry');
      await fixture(0, 'initial');
      await reloadAt('guest');
      return;
    }
    await wait(() => all('[data-essential-rail]').length === 4, 'four independent choice blocks');
    await emit('resize', { width: 1280, height: 840 });
    await wait(() => innerWidth === 1280 && innerHeight === 840, 'wide fixture viewport');
    assert(all('.choice-tabs button').map((n) => n.textContent.trim()).join(',') === '精选,歌单广场,排行榜,歌手,VIP', 'official tab order');
    assert(all('[data-essential-rail="songs"] .essential-card').length === 9, 'incomplete song column');
    assert(all('[data-essential-rail="voices"] .essential-card').length === 9, 'incomplete voice column');
    assert(!one('.ordinary-essential').textContent.includes('SYNTHETIC_PRIVATE'), 'raw metadata');
    await capture('initial');

    const rail = () => one('[data-essential-rail="playlists"]');
    const next = () => one('.essential-playlists [aria-label="官方歌单下一组"]');
    await wait(() => next() && !next().disabled, 'playlist carousel can advance');
    click(next());
    await wait(() => rail().scrollLeft > 0 && next()?.disabled, 'carousel movement completes');
    const left = rail().scrollLeft;
    click(button('playlists:heading'));
    await wait(() => one('.choice-tabs [aria-current="page"]')?.textContent.trim() === '歌单广场', 'official playlist category link');
    click(back());
    await wait(() => document.activeElement === button('playlists:heading'), 'playlist title return focus');
    assert(Math.abs(rail().scrollLeft - left) <= 1, `playlist carousel offset ${rail().scrollLeft} versus ${left}`);
    click(button('songs:heading'));
    await wait(() => one('.latest-releases-page'), 'latest music destination');
    click(back());
    await wait(() => document.activeElement === button('songs:heading'), 'latest title return focus');

    await fixture(2, 'books-down');
    select(2);
    await wait(() => one('.essential-books [role=status]'), 'category failure');
    assert(bookSelect().value === '0' && one('.essential-books').textContent.includes('type0'), 'failed category retains selected block');
    await capture('category_failure');
    await fixture(2, 'initial');
    select(2);
    await wait(() => bookSelect().value === '2' && one('.essential-books').textContent.includes('type2'), 'category changed');
    const scroller = one('.ordinary-essential');
    scroller.scrollTop = scroller.scrollHeight;
    await sleep(300);
    const scroll = scroller.scrollTop;
    assert(scroll > 0, 'viewport can scroll');
    click(button('books:history:open'));
    const listened = () => one('.listened-books');
    const pagination = (label) => all('.listened-books-pagination button').find((node) => node.textContent.trim() === label);
    await wait(() => all('[data-listened-book]').length === 20, 'dedicated listened books first page');
    assert(listened().querySelector('h1').textContent.trim() === '听过的书', 'dedicated listened books heading');
    assert(!one('.history-page') && listened().textContent.includes('已播 10%'), 'recent chapter checkpoint and no timeline controls');
    click(pagination('下一页'));
    await wait(() => all('[data-listened-book]').length === 1 && one('[data-listened-book="20"]'), 'twenty rows per page');
    click(pagination('上一页'));
    const chapter = () => one('[data-listened-book="0"] [data-listened-chapter]');
    await wait(() => chapter() && !chapter().disabled, 'chapter back on first page');
    click(chapter());
    const { epoch } = await invoke('account_snapshot');
    let listenedPlayback;
    for (let attempt = 0; attempt < 100; attempt++) {
      listenedPlayback = await invoke('player_snapshot', { epoch });
      if (listenedPlayback.track?.voiceId === '702' && listenedPlayback.state === 'playing'
          && listenedPlayback.position > .2) break;
      await sleep(60);
    }
    assert(listenedPlayback.track?.voiceId === '702' && listenedPlayback.track?.id === '1702'
        && listenedPlayback.state === 'playing' && listenedPlayback.queueLen === 2
        && listenedPlayback.position > .2, 'clicked assigned episode, not first chapter or radio as media');
    await capture('listened_chapter', { playing: true });
    click(back());
    await wait(() => document.activeElement === button('books:history:open'), 'book return focus');
    assert(one('.ordinary-essential').scrollTop === scroll && bookSelect().value === '2', 'book return scroll/selection');
    await capture('history_return', { scroll });

    const play = button('songs:2:play');
    await wait(() => !play.disabled, 'native player is ready');
    play.scrollIntoView({ block: 'center' });
    click(play);
    await wait(() => play.getAttribute('aria-label').startsWith('暂停'), 'song control switches to pause');
    await sleep(850);
    await capture('native_play', { playing: true });
    click(play);
    await wait(() => play.getAttribute('aria-label').startsWith('播放'), 'song control resumes after pause');
    await capture('paused');

    const voice = button('voices:704:play');
    voice.scrollIntoView({ block: 'center' });
    click(voice);
    await wait(() => voice.getAttribute('aria-label').startsWith('暂停'), 'voice playback authorized');
    await sleep(850);
    await capture('voice_play', { playing: true });

    const book = button('books:601:play');
    book.scrollIntoView({ block: 'center' });
    click(book);
    await wait(() => book.getAttribute('aria-label').startsWith('暂停'), 'book queue started');
    await sleep(850);
    await capture('book_play', { playing: true });

    const session = await invoke('account_snapshot');
    await fixture(2, 'songs-down');
    const recovered = await invoke('ordinary_essential', { epoch: session.epoch, category: 2 });
    assert(recovered.value.failedBlocks === 1, 'one branch failed');
    assert(recovered.value.content.songs[0].title === '精选新歌 2 / 1', 'isolated cached songs restored');
    assert(recovered.value.content.books.cards[1].title.startsWith('type2'), 'new books projected');
    assert(!JSON.stringify(recovered).includes('blocked.invalid'), 'raw links not in IPC');
    await capture('partial_fallback');
    await fixture(3, 'all-down');
    await reloadAt('other-account');
  } catch (error) {
    await emit('failed', { ok: false, error: String(error), states });
  }
})();
