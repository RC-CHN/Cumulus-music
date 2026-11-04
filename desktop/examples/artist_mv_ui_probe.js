// Real catalog WebView, continuation, comments, and synthetic libmpv video.
(async () => {
  const all = (s, root = document) => [...(root?.querySelectorAll(s) ?? [])];
  const one = (s, root = document) => root?.querySelector(s);
  const button = (name, root = document) => all('button', root).find((n) => (n.getAttribute('aria-label') || n.textContent.trim()) === name);
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, rest = {}) => invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...rest } });
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (condition, message) => { if (!condition) throw Error(message); };
  async function wait(test, label) {
    for (let i = 0; i < 160; i++) { const value = await test(); if (value) return value; await sleep(70); }
    throw Error(label);
  }
  function click(node) {
    assert(node && !node.disabled, 'unavailable action');
    node.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    node.focus({ preventScroll: true });
    node.click();
  }
  const scroller = () => one('.artist-mv-scroll');
  const video = () => button('播放 MV 合成原生 MV', scroller());
  const mvTab = () => one('[data-catalog-tab="videos"]');
  const menu = () => one('[role="menu"]:not([inert])');
  function scroll(top) {
    const node = scroller();
    node.scrollTop = top;
    node.dispatchEvent(new Event('scroll'));
  }
  let count = 0;
  async function capture(name, extra = {}) {
    await sleep(180);
    assert(document.documentElement.scrollWidth <= innerWidth, 'horizontal overflow');
    const mounted = all('.artist-mv-card').length;
    assert(mounted <= 30, 'MV DOM list not bounded');
    const stage = `ux_${String(++count).padStart(2, '0')}_${name}`;
    window.__CUMULUS_ALBUM_CAPTURE_ACK__ = '';
    await emit(stage, { width: innerWidth, total: Number(scroller()?.dataset.catalogTotal || 0), mounted,
      scroll: scroller()?.scrollTop ?? 0, focus: document.activeElement?.dataset.catalogKey ?? null, ...extra });
    await wait(() => window.__CUMULUS_ALBUM_CAPTURE_ACK__ === stage, 'capture acknowledgement');
  }
  try {
    await wait(() => one('.track-credit-links button'), 'daily artist entry');
    await emit('resize', { width: 900, height: 640 });
    await wait(() => innerWidth === 900, 'resize');
    click(one('.track-credit-links button'));
    await wait(() => mvTab()?.textContent.includes('MV') && one('.artist-header h1')?.textContent.includes('测试歌手'), 'positive MV tab');
    click(mvTab());
    await wait(() => scroller()?.dataset.catalogTotal === '20', 'first artist MV page');
    const grid = one('.artist-mv-grid');
    assert(getComputedStyle(grid).gridTemplateColumns.split(' ').length === 3, 'three columns');
    const cover = one('.artist-mv-cover');
    const rect = cover.getBoundingClientRect();
    assert(Math.abs(rect.height / rect.width - .565) < .02, '56.5% artist MV cover');
    assert(cover.textContent.includes('02:05') && cover.textContent.includes('34.6万'), 'duration and play count');
    assert(!grid.textContent.includes('不得重复展示的合成歌手'), 'artist credit is not duplicated');
    const unsupported = button('暂不可播放 合成普通视频', scroller());
    assert(unsupported?.disabled, 'ordinary video has no playback authority');
    unsupported.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    assert(!menu(), 'ordinary video has no MV menu');
    await capture('initial');
    scroll(scroller().scrollHeight);
    const retry = await wait(() => button('重试', scroller()) && scroller().textContent.includes('保留已加载内容') && button('重试', scroller()), 'automatic next failure');
    assert(scroller().dataset.catalogTotal === '20', 'failed continuation keeps prior rows');
    await capture('next_error');
    click(retry);
    await wait(() => scroller()?.dataset.catalogTotal === '39' && one('[data-artist-mv-slot="90071992547409931233"]')?.textContent.includes('合成替换交叠 MV'), 'retry uses cursor and replaces overlap');
    await capture('retry');
    const target = await wait(video, 'exact target present at nonzero scroll');
    target.focus({ preventScroll: true });
    const saved = scroller().scrollTop;
    assert(saved > 0, 'nonzero return context');
    target.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    await wait(() => button('查看评论', menu()) && button('播放', menu()), 'official MV menu');
    await capture('menu');
    click(button('查看评论', menu()));
    await wait(() => document.body.textContent.includes('合成 MV 评论读回'), 'exact MV thread comments');
    await capture('comments');
    click(button('返回上一页'));
    await wait(() => video() && document.activeElement === video() && Math.abs(scroller().scrollTop - saved) <= 2, 'comments back restores exact MV and scroll');
    await capture('comments_back');
    video().dispatchEvent(new KeyboardEvent('keydown', { key: 'F10', shiftKey: true, bubbles: true, cancelable: true }));
    await wait(() => button('播放', menu()), 'keyboard MV menu');
    click(button('播放', menu()));
    const { epoch } = await invoke('account_snapshot');
    const native = await wait(async () => {
      if (!one('.video-canvas')) return;
      const view = await invoke('video_snapshot', { epoch });
      if (view.id === '601' && view.state === 'playing' && view.position > .15 && Number(one('.video-canvas').dataset.renderedFrames) > 1) return view;
    }, 'advancing native MV and rendered frames');
    const canvas = one('.video-canvas');
    const pixels = canvas.getContext('2d').getImageData(0, 0, canvas.width, canvas.height).data;
    assert(native.actualResolution === 480 && pixels.some((value, i) => i % 4 !== 3 && value > 20), 'decoded synthetic image on canvas');
    await capture('native_play', { nativeId: native.id, position: native.position, frames: Number(canvas.dataset.renderedFrames) });
    click(button('关闭视频'));
    await wait(async () => (await invoke('video_snapshot', { epoch })).state !== 'playing' && !scroller(), 'close stops video without navigating');
    click(button('返回上一页'));
    await wait(() => video() && document.activeElement === video() && Math.abs(scroller().scrollTop - saved) <= 2, 'MV back restores exact MV and scroll');
    await capture('play_back');
    click(one('[data-catalog-tab="similar"]'));
    const unknown = await wait(() => button('查看歌手：相似歌手 13'), 'similar artist card');
    click(unknown);
    await wait(() => one('.artist-header h1')?.textContent.includes('相似歌手 13') && one('[data-catalog-tab="albums"]'), 'unknown mvSize artist');
    assert(!mvTab() && !scroller(), 'unknown mvSize has no MV tab');
    await capture('unknown_count');
    await emit('complete', { ok: true, stageCount: count, realMusicRequests: 0, realAccountWrites: 0, nativeVideo: true });
  } catch (error) {
    await emit('failed', { error: String(error), stageCount: count });
  }
})();
