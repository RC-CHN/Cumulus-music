// Production Vue and IPC using the isolated UX fixture.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const count = (selector) => document.querySelectorAll(selector).length;
  const button = (label, root = document) =>
    [...root.querySelectorAll('button')].find(
      (item) => item.getAttribute('aria-label') === label || item.textContent.trim() === label,
    );
  const assert = (condition, label) => {
    if (!condition) throw Error(label);
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
  try {
    await wait(() => count('.track-row'), 'daily tracks');
    click(button('我的歌单'));
    await wait(() => button('打开歌单链接'), 'library');
    click(button('打开歌单链接'));
    await wait(() => one('[aria-label="歌单分享链接"]'), 'playlist link dialog');
    const input = one('[aria-label="歌单分享链接"]');
    input.value = '2';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    click(one('.playlist-dialog button.primary'));
    await wait(() => one('.playlist-hero') && count('.track-row'), 'playlist tracks');
    const hero = one('.playlist-hero');
    const sections = [...document.querySelectorAll('.playlist-section')];
    const [songs] = sections;
    const subscribers = sections.find((section) => section.textContent.trim().startsWith('收藏者'));
    assert(subscribers.textContent.includes('25'), 'authoritative tab count');
    assert(songs.getAttribute('aria-pressed') === 'true', 'songs selected');
    await emit('ux_01_tracks');
    click(subscribers);
    await wait(() => count('.playlist-subscriber-grid li') === 24, 'first collector page');
    assert(
      one('.playlist-hero') === hero && !count('.track-row'),
      'same hero, collector rows only',
    );
    assert(document.activeElement === subscribers, 'tab focus');
    assert(subscribers.getAttribute('aria-pressed') === 'true', 'collectors selected');
    const grid = one('.playlist-subscriber-grid');
    const firstPage = one('.playlist-subscribers');
    assert(!grid.querySelector('button'), 'missing IDs are display-only');
    firstPage.scrollTop = firstPage.scrollHeight;
    assert(firstPage.scrollTop > 0, 'collector rows and pagination are reachable by scrolling');
    firstPage.scrollTop = 0;
    assert(
      getComputedStyle(grid).gridTemplateColumns.split(/\s+/).length === 3,
      'three-column grid',
    );
    assert(grid.scrollWidth <= grid.clientWidth, 'collector grid stays in its column');
    await emit('ux_02_collectors');
    firstPage.scrollTop = 380;
    const ordinaryScroll = firstPage.scrollTop;
    click(button('我的歌单'));
    await wait(() => button('打开歌单链接') && !one('.playlist-hero'), 'left for the library');
    click(button('返回上一页'));
    await wait(() => count('.playlist-subscriber-grid li') === 24 &&
      Math.abs((one('.playlist-subscribers')?.scrollTop ?? 0) - ordinaryScroll) < 2 &&
      [...document.querySelectorAll('.playlist-section')].at(-1)?.getAttribute('aria-pressed') === 'true',
      'ordinary Back restores collectors and scroll');
    await emit('ux_06_ordinary_back');
    click(button('下一页'));
    await wait(() => one('.playlist-subscribers [role="alert"]'), 'failed page');
    assert(count('.playlist-subscriber-grid li') === 24, 'preserved page after failure');
    click(button('重试', one('.playlist-subscribers')));
    await wait(
      () => one('.playlist-subscriber-grid')?.textContent.includes('合成收藏者 25'),
      'retry page',
    );
    assert(count('.playlist-subscriber-grid li') === 1, 'only second page rendered');
    assert(button('下一页').disabled, 'last page');
    await emit('ux_03_retried');
    click(button('查看合成收藏者 25的主页'));
    await wait(
      () => one('.playlist-creator-page h1')?.textContent === '合成作者 3',
      'opened the subscriber profile',
    );
    await emit('ux_04_profile');
    click(button('返回上一页'));
    await wait(
      () => document.activeElement === button('查看合成收藏者 25的主页'),
      'restored focus to the subscriber card',
    );
    assert(count('.playlist-subscriber-grid li') === 1, 'restored the second page');
    await emit('ux_05_profile_back');
    const restoredSongs = [...document.querySelectorAll('.playlist-section')][0];
    const restoredHero = one('.playlist-hero');
    click(button('上一页'));
    await wait(() => count('.playlist-subscriber-grid li') === 24, 'previous page');
    click(restoredSongs);
    await wait(() => count('.track-row') && !one('.playlist-subscribers'), 'return to tracks');
    assert(one('.playlist-hero') === restoredHero, 'hero still mounted');
    await emit('complete', { ok: true });
  } catch (error) {
    await emit('failed', { message: String(error) });
  }
})();
