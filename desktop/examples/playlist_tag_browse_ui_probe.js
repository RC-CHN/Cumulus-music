// Production tag navigation and both official read shapes via a synthetic API.
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
  const cards = () => [...document.querySelectorAll('[data-tag-playlist-id]')].map((item) => item.dataset.tagPlaylistId);
  try {
    await wait(() => one('.track-row'), 'daily tracks');
    click(button('我的歌单'));
    await wait(() => button('打开歌单链接'), 'library');
    click(button('打开歌单链接'));
    await wait(() => one('[aria-label="歌单分享链接"]'), 'playlist link dialog');
    const input = one('[aria-label="歌单分享链接"]');
    input.value = '2';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    click(one('.playlist-dialog button.primary'));
    await wait(() => one('.playlist-hero-tags button'), 'playlist tags');
    await emit('ux_01_source');
    click(one('.playlist-hero-tags button'));
    await wait(() => cards().length === 24, 'first bounded page');
    assert(one('.playlist-tag-browse').textContent.includes('轻音乐'), 'tag preserved');
    assert(document.activeElement === one('.playlist-tag-browse h1'), 'category focus');
    await emit('ux_02_first');
    click(button('下一页'));
    await wait(() => cards().join(',') === '3,4', 'authoritative continuation order');
    await emit('ux_03_next');
    click(one('[data-tag-playlist-id="3"]'));
    await wait(() => one('.playlist-hero h1')?.textContent === '合成歌单 3', 'selected playlist detail');
    click(button('返回上一页'));
    await wait(() => cards().join(',') === '3,4', 'back restores continuation');
    await wait(() => document.activeElement === one('[data-tag-playlist-id="3"]'), 'back restores card focus');
    click(button('上一页'));
    await wait(() => cards().length === 24, 'back to cached first page');
    await emit('complete', { ok: true });
  } catch (error) {
    await emit('failed', { message: String(error) });
  }
})();
