// Production playlist creator navigation with an isolated synthetic service.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const button = (label) =>
    [...document.querySelectorAll('button')].find(
      (item) => item.getAttribute('aria-label') === label || item.textContent.trim() === label,
    );
  const assert = (value, label) => { if (!value) throw Error(label); };
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
    window.__TAURI_INTERNALS__.invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...values } });
  const card = (id) => one(`.playlist-creator-page [data-library-open="${id}"]`);
  try {
    await wait(() => one('.track-row'), 'daily tracks');
    click(button('我的歌单'));
    await wait(() => button('打开歌单链接'), 'library');
    click(button('打开歌单链接'));
    await wait(() => one('[aria-label="歌单分享链接"]'), 'playlist link dialog');
    const input = one('[aria-label="歌单分享链接"]');
    input.value = '20';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    click(one('.playlist-dialog button.primary'));
    await wait(() => one('button.playlist-creator') && one('.playlist-hero h1')?.textContent === '合成歌单 20', 'creator control');
    const publicBadge = one('.playlist-art .playlist-cover-count');
    assert(publicBadge?.getAttribute('aria-label') === '播放次数：12,600' &&
      publicBadge.textContent.trim() === '1.3万' && !one('.playlist-art .playlist-privacy'),
      'public cover shows a compact play count only');
    const publicCover = one('.playlist-art').getBoundingClientRect();
    const publicIcon = publicBadge.querySelector('svg').getBoundingClientRect();
    assert(publicCover.right - publicIcon.left < publicCover.width / 2 &&
      publicIcon.top - publicCover.top < publicCover.height / 3, 'count appears at the top right');
    await emit('ux_01_source');
    click(one('button.playlist-creator'));
    await wait(() => card(20), 'public created list');
    assert(one('.playlist-creator-page h1').textContent === '合成作者 3', 'verified profile');
    assert(document.activeElement === one('.playlist-creator-page h1'), 'profile heading focus');
    assert(!card(21) && !card(2), 'private excluded and groups separate');
    assert(!one('.playlist-creator-page .library-card-play'), 'no disconnected play affordance');
    await emit('ux_02_profile');
    click(button('收藏的歌单 1'));
    await wait(() => card(2) && !card(20), 'collected list');
    click(card(2));
    await wait(() => one('.playlist-hero h1')?.textContent.includes('午后散步'), 'open collected playlist');
    const mask = one('.playlist-art .playlist-privacy');
    assert(mask?.getAttribute('aria-label') === '私密歌单' &&
      !one('.playlist-art .playlist-cover-count'), 'private cover masks the count');
    const privateCover = one('.playlist-art').getBoundingClientRect();
    const lock = mask.querySelector('svg').getBoundingClientRect();
    assert(Math.abs(privateCover.left + privateCover.right - lock.left - lock.right) < 2 &&
      Math.abs(privateCover.top + privateCover.bottom - lock.top - lock.bottom) < 2,
      'private lock is centered on the cover');
    await emit('ux_03_private_cover');
    click(button('返回上一页'));
    await wait(() => card(2) && document.activeElement === card(2), 'return to focused card');
    assert(!card(20), 'group preserved');
    click(button('返回上一页'));
    await wait(() => one('.playlist-hero h1')?.textContent === '合成歌单 20', 'return to source playlist');
    await emit('complete', { ok: true });
  } catch (error) {
    await emit('failed', { message: String(error) });
  }
})();
