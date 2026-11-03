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
    await wait(() => one('.playlist-hero h1')?.textContent.includes(name) && one('.track-row'), 'playlist tracks');
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
    await emit('ux_01_tracks');
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
