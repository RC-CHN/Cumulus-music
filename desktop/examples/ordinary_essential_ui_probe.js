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
    await wait(() => !one('.page-enter-active,.page-leave-active'), 'settled page ' + stage);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
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
  const chapter = () => one('[data-listened-book="0"] [data-listened-chapter]');
  const radioTitle = () => one('[data-listened-book="0"] .listened-book-title');
  const radioRows = () => all('.podcast-page .episode-row[data-voice-id]').map((row) => row.dataset.voiceId);
  const orderButton = () => all('.podcast-page .podcast-toolbar button')
    .find((node) => ['正序', '倒序'].includes(node.textContent.trim()));
  async function waitOrder(ascending) {
    const label = ascending ? '正序' : '倒序';
    const ids = ascending ? '791,792' : '792,791';
    await wait(() => !one('.page-enter-active,.page-leave-active')
      && one('.podcast-summary h2')?.textContent.trim() === '合成播单 691'
      && orderButton()?.textContent.trim() === label && !orderButton().disabled
      && radioRows().join(',') === ids, 'radio 691 detail: ' + label);
  }
  async function openRadio(ascending) {
    await wait(() => !one('.page-enter-active,.page-leave-active') && radioTitle(), 'radio title on listened page');
    click(radioTitle());
    await waitOrder(ascending);
  }
  async function goFromRadioToBooks() {
    click(back());
    await wait(() => !one('.page-enter-active,.page-leave-active') && chapter(), 'back from radio to listened books');
  }
  async function savedOrder(epoch) {
    const reply = await invoke('podcast_radio_order', { epoch, query: { kind: 'read', id: '691' } });
    assert(reply.epoch === epoch, 'radio preference belongs to the current account');
    return reply.value;
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
      const { epoch } = await invoke('account_snapshot');
      click(button('books:history:open'));
      await openRadio(true);
      assert(await savedOrder(epoch) === null, 'another account has no saved order for the same radio');
      await capture('radio_order_other_account', { preference: null, visible: radioRows() });
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
    await wait(() => chapter() && !chapter().disabled, 'chapter back on first page');
    const { epoch } = await invoke('account_snapshot');
    const player = () => invoke('player_snapshot', { epoch });
    const action = (action) => invoke('player_action', { epoch, action });
    async function waitPlayer(predicate, label) {
      let state;
      for (let attempt = 0; attempt < 100; attempt++) {
        state = await player();
        if (predicate(state)) return state;
        await sleep(60);
      }
      throw Error(`${label}: ${state?.state} ${state?.track?.voiceId} @${state?.position}`);
    }
    async function playChapter(id, queue, earlier) {
      await wait(() => chapter() && !chapter().disabled, 'chapter ready: ' + id);
      click(chapter());
      return waitPlayer((state) => state.state === 'playing' && !state.nativeSeeking
        && state.playId !== earlier && state.track?.voiceId === id
        && state.track?.id === String(Number(id) + 1000) && state.queueLen === queue
        && state.position > .2 && state.timeline?.mode === 'full' && !state.error, 'playing chapter ' + id);
    }
    async function backFromBooks() {
      click(back());
      await wait(() => document.activeElement === button('books:history:open'), 'book return focus');
      assert(one('.ordinary-essential').scrollTop === scroll && bookSelect().value === '2', 'book return scroll/selection');
    }
    async function reopenBooks() {
      click(button('books:history:open'));
      await wait(() => all('[data-listened-book]').length === 20 && chapter() && !chapter().disabled, 'reopened books');
    }

    assert(chapter().textContent.trim() === '目录章节 792', 'initial row uses the remote chapter title');
    const original = await playChapter('792', 2);
    assert(original.position >= 2.45 && original.position < 4.5, 'remote checkpoint starts at 2.5');
    await capture('listened_chapter', { playing: true, initialPosition: original.position });
    await action({ kind: 'pause', paused: true });
    await action({ kind: 'seek', seconds: 11.75 });
    const sought = await waitPlayer((state) => state.state === 'paused' && !state.nativeSeeking
      && state.playId === original.playId && Math.abs(state.position - 11.75) < .15, 'paused chapter seek');
    async function unchangedPaused(previous, label) {
      const current = await player();
      assert(current.state === 'paused' && current.playId === previous.playId
        && current.entryId === previous.entryId && current.queueRevision === previous.queueRevision
        && Math.abs(current.position - previous.position) < .15, label);
    }
    async function toggleOrder(ascending, previous) {
      click(orderButton());
      await waitOrder(ascending);
      assert(await savedOrder(epoch) === ascending, 'only the requested order committed');
      await unchangedPaused(previous, 'sorting detail must not change paused native playback');
    }
    await openRadio(true);
    assert(await savedOrder(epoch) === null, 'first account starts with no radio preference');
    await toggleOrder(false, sought);
    await capture('radio_order_desc', { playing: true, preference: false, visible: radioRows() });
    await goFromRadioToBooks();
    await openRadio(false);
    assert(await savedOrder(epoch) === false, 'the descending preference survived closing detail');
    await unchangedPaused(sought, 'reopening a sorted detail must not change paused native playback');
    await capture('radio_order_reopened', { playing: true, preference: false, visible: radioRows() });
    await toggleOrder(true, sought);
    const stopped = await action({ kind: 'stop' });
    assert(stopped.state === 'idle' && !stopped.playId && stopped.queueRevision === sought.queueRevision,
      'stop leaves the exact played chapter before reopening');
    await goFromRadioToBooks();
    await backFromBooks();
    await capture('history_return', { scroll });

    await reopenBooks();
    const localTitle = chapter().textContent.trim();
    const progress = Math.floor(1100 / Math.floor(sought.timeline.total));
    assert(localTitle === '最近播放：合成声音 792'
      && one('[data-listened-book="0"]').textContent.includes(`已播 ${progress}%`), 'matching departed chapter shows local title and progress');
    await capture('listened_saved', { localTitle, progress, soughtPosition: sought.position, duration: sought.timeline.total });
    const resumed = await playChapter('792', 3, original.playId);
    assert(resumed.position >= 10.9 && resumed.position < 13, 'around resumes near the saved whole second');
    await capture('listened_around', { playing: true, initialPosition: resumed.position });
    const held = await action({ kind: 'pause', paused: true });
    assert(held.state === 'paused' && held.playId === resumed.playId, 'hold around playback for rejection');
    await fixture(2, 'listened-around-missing');
    await wait(() => chapter() && !chapter().disabled, 'chapter ready to validate missing anchor');
    click(chapter());
    await wait(() => all('.playback-message[role=status]')
      .some((node) => node.textContent.includes('列表已更新，请刷新后重试')), 'missing around anchor shows failure');
    await unchangedPaused(held, 'missing anchor preserves paused native playback and queue');
    await capture('listened_around_rejected', { playing: true, notice: '列表已更新，请刷新后重试' });

    await fixture(2, 'initial');
    await openRadio(true);
    await toggleOrder(false, held);
    await fixture(2, 'listened-different');
    await goFromRadioToBooks();
    assert(chapter().textContent.trim() === '目录章节 791'
      && one('[data-listened-book="0"]').textContent.includes('已播 10%'), 'same-book different chapter uses remote row');
    const other = await playChapter('791', 2, resumed.playId);
    assert(other.position >= 2.45 && other.position < 4.5, 'different chapter keeps its own breakpoint');
    await capture('listened_mismatch', { playing: true, initialPosition: other.position });
    await fixture(2, 'initial');
    await backFromBooks();

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
