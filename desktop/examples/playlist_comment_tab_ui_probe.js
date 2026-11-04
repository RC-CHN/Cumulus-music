// Production playlist tabs and comments IPC backed by a synthetic API.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const button = (label) =>
    [...document.querySelectorAll('button')].find(
      (item) => item.getAttribute('aria-label') === label || item.textContent.trim() === label,
    );
  const assert = (value, label) => {
    if (!value) throw Error(label);
  };
  const click = (item) => {
    assert(item && !item.disabled, 'missing or disabled button');
    item.focus({ preventScroll: true });
    item.click();
  };
  async function wait(predicate, label) {
    for (let i = 0; i < 180; i++) {
      if (predicate()) return;
      await new Promise((done) => setTimeout(done, 100));
    }
    throw Error(label);
  }
  const emit = (stage, values = {}) =>
    window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
      event: 'ux-proof',
      payload: { stage, ...values },
    });
  const pane = () => one('.comments-page.comments-embedded');
  const songs = () => one('.playlist-section');
  const collectors = () => [...document.querySelectorAll('.playlist-section')].at(-1);
  const commentTab = () => button('歌单评论');
  async function openPlaylist(id, name) {
    click(button('我的歌单'));
    await wait(() => button('打开歌单链接'), 'library');
    click(button('打开歌单链接'));
    await wait(() => one('[aria-label="歌单分享链接"]'), 'playlist link dialog');
    const input = one('[aria-label="歌单分享链接"]');
    input.value = id;
    input.dispatchEvent(new Event('input', { bubbles: true }));
    click(one('.playlist-dialog button.primary'));
    await wait(() => one('.playlist-hero h1')?.textContent.includes(name) && one('.track-row') &&
      !one('.playlist-dialog[open]') && !one('.playlist-hero').closest('[inert]'),
      'playlist tracks and completed modal close');
  }
  async function verifySearch() {
    const frame = one('.playlist-track-search');
    const field = frame?.querySelector('input');
    assert(field, 'playlist search field');
    const width = (expected) => Math.abs(frame.getBoundingClientRect().width - expected) < 1;
    const clear = () => button('清空歌单搜索');
    const type = (value, composing = false) => {
      field.value = value;
      field.dispatchEvent(new InputEvent('input', { bubbles: true, isComposing: composing }));
    };
    await wait(() => width(74), 'empty unfocused search is compact');
    field.focus();
    assert(document.activeElement === field, 'search field accepts focus');
    await wait(() => width(178), 'focus expands the search field');
    field.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }));
    type('合成歌曲 2 ·', true);
    assert(!one('.playlist-track-match'), 'composition is not applied prematurely');
    field.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true }));
    await wait(() => one('.playlist-track-match')?.textContent.includes('1 首匹配歌曲') &&
      one('.track-row')?.textContent.includes('合成歌曲 2 ·'), 'committed keyword filters exact song');
    songs().focus();
    await wait(() => width(178), 'nonempty search remains expanded when blurred');
    click(clear());
    await wait(() => document.activeElement === field && !field.value &&
      !one('.playlist-track-match') && !clear(), 'clear restores input focus and all songs');
    type('合成歌曲 3 ·');
    await wait(() => one('.playlist-track-match'), 'keyboard clear setup');
    field.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    await wait(() => !field.value && !one('.playlist-track-match') && document.activeElement === field,
      'Escape clears without losing focus');
    songs().focus();
    await wait(() => width(74), 'empty search collapses on blur');
  }

  async function verifyComment(needle, total, hero) {
    click(commentTab());
    await wait(() => pane()?.textContent.includes(needle), 'embedded playlist comment');
    assert(one('.playlist-hero') === hero, 'same playlist hero');
    assert(document.activeElement === commentTab(), 'comment tab retains focus');
    assert(commentTab().getAttribute('aria-pressed') === 'true', 'comments selected');
    assert(commentTab().textContent.includes(total), 'confirmed comment count');
    assert(!one('.track-row') && !one('.playlist-subscribers'), 'exclusive content pane');
    assert(!one('.playlist-track-search') && !one('.playlist-locate'), 'song controls hidden');
    assert(document.documentElement.scrollWidth <= innerWidth, 'page width');
  }

  try {
    await wait(() => one('.track-row'), 'daily tracks');
    await openPlaylist('2', '午后散步');
    const first = one('.playlist-hero');
    assert(!pane() && !commentTab().querySelector('small'), 'no speculative initial count');
    await verifySearch();
    await emit('ux_01_tracks', { compactSearch: true });
    await verifyComment('第一张合成歌单的评论', '12', first);
    await emit('ux_02_comments');
    click(collectors());
    await wait(() => one('.playlist-subscriber-grid li') && !pane(), 'collector switch');
    assert(one('.playlist-hero') === first && document.activeElement === collectors(), 'collector keeps hero and focus');
    click(songs());
    await wait(() => one('.track-row') && !one('.playlist-subscribers'), 'song switch');
    assert(one('.playlist-hero') === first && document.activeElement === songs(), 'songs keep hero and focus');
    assert(commentTab().querySelector('small')?.textContent === '12', 'observed count survives sibling switches');
    await emit('ux_03_tracks_again');

    const back = () => button('返回上一页');
    const openLibrary = async () => {
      click(button('我的歌单'));
      await wait(() => button('打开歌单链接') && !one('.playlist-hero'), 'leave for the library');
    };
    const tracks = () => one('.track-scroll');
    tracks().scrollTop = 340;
    const songsScroll = tracks().scrollTop;
    assert(songsScroll > 100, 'songs have a nonzero scroll checkpoint');
    await openLibrary();
    click(back());
    await wait(() => one('.playlist-hero') && songs()?.getAttribute('aria-pressed') === 'true' &&
      Math.abs((tracks()?.scrollTop ?? 0) - songsScroll) < 2, 'songs Back restores the visit scroll');
    await emit('ux_05_songs_back', { scrollTop: tracks().scrollTop });
    await verifyComment('第一张合成歌单的评论', '12', one('.playlist-hero'));
    pane().scrollTop = 120;
    const commentsScroll = pane().scrollTop;
    assert(commentsScroll > 20, 'embedded comments have a real scroll checkpoint');
    await openLibrary();
    click(back());
    await wait(() => pane()?.textContent.includes('第一张合成歌单的评论') &&
      commentTab().getAttribute('aria-pressed') === 'true' &&
      Math.abs(pane().scrollTop - commentsScroll) < 2 && !one('.track-row'),
      'ordinary Back restores comments and the visit scroll');
    await emit('ux_06_comments_back', { scrollTop: pane().scrollTop });
    const author = () => one('.comments-embedded [data-comment-author-control="name"]');
    click(author());
    await wait(() => one('.playlist-creator-page h1')?.textContent === '合成作者 3', 'comment author route');
    click(back());
    await wait(() => author() && document.activeElement === author() &&
      commentTab().getAttribute('aria-pressed') === 'true', 'author Back retains the exact focus and selected tab');
    await emit('ux_07_author_back');
    click(back());
    await wait(() => button('打开歌单链接') && !one('.playlist-hero'),
      'direct Back leaves the playlist without revisiting sibling tabs');
    await emit('ux_08_direct_back');

    await openPlaylist('3', '合成歌单 3');
    const second = one('.playlist-hero');
    assert(second !== first && !commentTab().querySelector('small') && !pane(), 'new playlist clears old count and panel');
    await verifyComment('第二张合成歌单的评论', '8', second);
    assert(!pane().textContent.includes('第一张合成歌单的评论'), 'no cross-playlist comments');
    await emit('ux_04_other_playlist');
    await emit('complete', { ok: true });
  } catch (error) {
    await emit('failed', { message: String(error) });
  }
})();
