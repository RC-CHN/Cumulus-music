// Production artist UI/IPC/Rust and libmpv, with in-process synthetic services.
(async () => {
  const one = s => document.querySelector(s), all = s => [...document.querySelectorAll(s)];
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const assert = (ok, message) => { if (!ok) throw Error(message); };
  const emit = (stage, extra = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...extra } });
  const wait = async (fn, message) => { for (let i = 0; i < 200; i++) { if (fn()) return; await sleep(70); } throw Error(message); };
  const button = (label, root = document) => [...root.querySelectorAll('button')].find(n => n.getAttribute('aria-label') === label || n.textContent.trim() === label);
  const click = n => { assert(n && !n.disabled, `control unavailable ${n?.outerHTML}`); n.scrollIntoView({ block: 'nearest', behavior: 'instant' }); n.focus({ preventScroll: true }); n.click(); };
  const settled = () => !one('.catalog-tab-enter-active,.catalog-tab-leave-active,.page-enter-active,.page-leave-active,.menu-enter-active,.menu-leave-active,.modal-enter-active,.modal-leave-active');
  const tab = key => one(`[data-catalog-tab="${key}"]`);
  const pick = async key => { click(tab(key)); await wait(() => tab(key)?.getAttribute('aria-selected') === 'true' && settled(), `tab ${key}`); };
  const state = () => one('.playerbar')?.dataset.state;
  const playing = () => state() === 'playing' && Number(one('.playerbar')?.dataset.position) > .2;
  const stop = async () => { click(one('[aria-label="停止播放"]')); await wait(() => state() !== 'playing', 'stop'); };
  const mode = async value => { await emit('fixture', { mode: value }); await wait(() => window.__CUMULUS_ALBUM_MODE__ === value, 'fixture acknowledgement'); };
  const back = async () => { click(one('.back-button')); await wait(() => one('.artist-header') && settled(), 'all-songs back to artist'); };
  function cardPaintSamples() {
    const scroller = one('.catalog-card-scroll'), page = one('.catalog-page');
    if (!scroller || !page) return [];
    const bounds = scroller.getBoundingClientRect(), outer = page.getBoundingClientRect();
    // Only opaque, square album covers: disabled video opacity and circular
    // artist corners intentionally do not equal their raw background color.
    return all('.catalog-card:not(.artist) .catalog-card-placeholder').flatMap(node => {
      const r = node.getBoundingClientRect(), x = r.left + 12,
        y = Math.max(r.top + 12, bounds.top + 12, outer.top + 12);
      if (x >= bounds.right || y >= Math.min(r.bottom, bounds.bottom, outer.bottom) - 12) return [];
      return [{ x, y, color: getComputedStyle(node).backgroundColor }];
    }).slice(0, 2);
  }
  let count = 0, resizedAt = performance.now();
  window.addEventListener('resize', () => { resizedAt = performance.now(); });
  async function capture(label, queue) {
    await wait(settled, 'transition settled'); await sleep(150);
    assert(document.documentElement.scrollWidth <= innerWidth, 'document overflow');
    const page = one('.catalog-page');
    assert(page && page.scrollWidth <= page.clientWidth + 1, 'artist overflow');
    assert(page.getBoundingClientRect().bottom <= one('.playerbar').getBoundingClientRect().top + 1, 'page overlaps player');
    assert(all('.track-row').length <= 30 && all('.catalog-card').length <= 30, 'unbounded catalog rendering');
    if (one('.artist-tabs')) {
      assert(all('.artist-tabs [aria-selected="true"]').length === 1, 'selected tab count');
      assert(all('.artist-tabs [tabindex="0"]').length === 1, 'tab stop count');
    }
    const stage = `ux_${String(++count).padStart(2, '0')}_${label}`;
    const paintSamples = cardPaintSamples();
    if (/albums_return_|albums_refresh_failed|albums_failed_return/.test(label))
      assert(paintSamples.length > 0, 'restored album cards outside visible viewport');
    window.__CUMULUS_ALBUM_CAPTURE_ACK__ = '';
    await emit(stage, { width: innerWidth, height: innerHeight, ratio: devicePixelRatio, screenX, screenY, paintSamples,
      theme: document.documentElement.dataset.theme, visible: document.visibilityState === 'visible', focused: document.hasFocus(),
      playing: !!queue, expectedQueue: queue ?? 0, allSongs: !!one('.artist-all-title'),
      selected: one('.artist-tabs [aria-selected="true"]')?.dataset.catalogTab,
      title: one('.artist-header h1')?.textContent,
      listScroll: one('.track-scroll,.catalog-card-scroll')?.scrollTop ?? 0,
      lastAlbum: all('.catalog-card strong').at(-1)?.textContent,
    });
    await wait(() => window.__CUMULUS_ALBUM_CAPTURE_ACK__ === stage, 'capture acknowledgement');
    await sleep(150);
  }
  async function artist() {
    click(button('每日推荐')); await sleep(400); await wait(() => one('.track-credit-links button'), 'daily');
    click(one('.track-credit-links button')); await wait(() => one('.artist-header') && tab('albums') && settled(), 'artist');
  }
  async function allSongs() {
    click(button('查看全部歌曲 →')); await wait(() => one('.artist-all-title') && settled(), 'all songs');
  }
  async function albumsBottom(name) {
    const grid = one('.catalog-card-scroll'); grid.scrollTop = grid.scrollHeight; grid.dispatchEvent(new Event('scroll'));
    await wait(() => all('.catalog-card strong').at(-1)?.textContent === name, `last album ${name}`);
    return grid.scrollTop;
  }
  try {
    await wait(() => one('.track-credit-links button'), 'bootstrap');
    for (const [width, height, color] of [[1120,760,'light'],[760,540,'light'],[760,540,'dark'],[1120,760,'dark']]) {
      await emit('resize', { width, height }); await wait(() => innerWidth === width && innerHeight === height && performance.now() - resizedAt >= 500, 'resize settled');
      click(one('[aria-label="外观主题"]')); await wait(() => one('.action-menu'), 'theme');
      click(button(color === 'light' ? '浅色' : '深色')); await wait(() => !one('.action-menu') && document.documentElement.dataset.theme === color, 'theme applied');
      await artist();
      assert(one('.artist-header h1').textContent === '测试歌手', 'artist title');
      assert(tab('albums').querySelector('small').textContent === '62', 'album count');
      assert(tab('videos').textContent.trim().startsWith('MV'), 'official MV label');
      assert(tab('concerts').querySelector('small').textContent === '1', 'concert count');
      assert(one('.artist-header .catalog-cover').getBoundingClientRect().width === (width <= 900 ? 168 : 194), 'artist artwork dimensions');
      await capture(`header_${width}_${color}`);
      tab('songs').focus(); tab('songs').dispatchEvent(new KeyboardEvent('keydown', { key: 'End', bubbles: true }));
      await wait(() => tab('concerts').getAttribute('aria-selected') === 'true' && one('.catalog-concert-card') && settled(), 'keyboard End concerts');
      assert(document.activeElement === tab('concerts'), 'tab keyboard focus');
      await capture(`concerts_${width}_${color}`);
      await pick('songs'); await wait(() => !button('播放全部', one('.artist-actions')).disabled, 'hot play ready');
      click(button('播放全部', one('.artist-actions'))); await wait(playing, 'hot playback'); await capture(`hot_playing_${width}_${color}`, 50); await stop();
      await allSongs();
      assert(!one('.artist-header') && !one('.artist-tabs'), 'all-songs page uses separate controls');
      await wait(() => button('加载更多') && !button('加载更多').disabled, 'all songs first page');
      click(button('加载更多')); await wait(() => one('.catalog-track-actions .muted')?.textContent.includes('40 首'), 'all second page');
      click(button('加载更多')); await wait(() => one('.catalog-track-actions .muted')?.textContent.includes('55 首') && !button('加载更多'), 'all third page');
      const scroll = one('.track-scroll'); scroll.scrollTop = scroll.scrollHeight; scroll.dispatchEvent(new Event('scroll')); await sleep(200);
      const allTop = scroll.scrollTop; assert(allTop > 0, 'all-songs scrolled');
      await capture(`all_55_${width}_${color}`);
      click(button('播放已加载歌曲')); await wait(playing, 'all loaded playback'); await capture(`all_playing_${width}_${color}`, 55);
      await back();
      await capture(`after_all_back_${width}_${color}`, 55); await stop();
      await pick('albums'); await wait(() => one('.catalog-card') && button('加载更多'), 'album first page');
      click(button('加载更多')); await wait(() => one('.catalog-card-scroll')?.dataset.catalogTotal === '40', 'album more completed');
      const albumTop = await albumsBottom('合成专辑 46'); await capture(`albums_40_${width}_${color}`);
      await pick('intro'); await wait(() => one('.catalog-description h2'), 'introduction'); await capture(`intro_${width}_${color}`);
      click(button('播放全部', one('.artist-actions'))); await wait(playing, 'play from introduction'); await capture(`intro_playing_${width}_${color}`, 50); await stop();
      await pick('songs'); await allSongs();
      await wait(() => one('.catalog-track-actions .muted')?.textContent.includes('55 首') && Math.abs(one('.track-scroll').scrollTop - allTop) < 1, 'all prefix and viewport restored');
      await back();
      click(tab('albums')); click(tab('intro')); click(tab('albums'));
      await wait(() => tab('albums')?.getAttribute('aria-selected') === 'true' && settled(), 'rapid tab return');
      await wait(() => Math.abs(one('.catalog-card-scroll').scrollTop - albumTop) < 1 && all('.catalog-card strong').at(-1)?.textContent === '合成专辑 46', 'album prefix and viewport restored');
      await capture(`albums_return_${width}_${color}`);
    }
    await mode('artist-albums-offline'); click(one('[aria-label="刷新详情"]'));
    await wait(() => one('.catalog-list-error'), 'album refresh error');
    assert(all('.catalog-card strong').at(-1)?.textContent === '合成专辑 46', 'refresh failure discarded prefix');
    await capture('albums_refresh_failed');
    await pick('intro'); await pick('albums'); assert(one('.catalog-list-error'), 'return erased pending read error'); await capture('albums_failed_return');
    await mode('normal'); click(button('重试', one('.catalog-list-error'))); await wait(() => !one('.catalog-list-error') && one('.catalog-card-scroll')?.dataset.catalogTotal === '20', 'retry first page');
    await albumsBottom('合成专辑 26'); await capture('albums_refresh_recovered');
    await mode('artist-albums-offline'); click(button('加载更多')); await wait(() => one('.catalog-list-error'), 'more failure'); await capture('albums_more_failed');
    await mode('normal'); click(button('重试', one('.catalog-list-error'))); await wait(() => !one('.catalog-list-error') && one('.catalog-card-scroll')?.dataset.catalogTotal === '40', 'retry continuation');
    await albumsBottom('合成专辑 46'); await capture('albums_more_recovered');
    await pick('videos'); await wait(() => one('.catalog-video-card'), 'video types');
    assert(all('.catalog-video-card button').filter(n => n.disabled).length === 1, 'ordinary video boundary'); await capture('videos');
    await mode('artist-zero'); await pick('similar'); await wait(() => one('.catalog-card.artist'), 'similar artists'); click(one('.catalog-card.artist'));
    await wait(() => one('.artist-header h1')?.textContent === '相似歌手 13' && !one('.track-row') && settled(), 'empty artist');
    assert(button('播放全部', one('.artist-actions')).disabled && !tab('videos') && !tab('concerts'), 'empty artist eligibility'); await capture('empty_artist');
    await mode('artist-long'); await artist(); await wait(() => one('.artist-header h1')?.textContent.startsWith('长名称'), 'long artist'); await capture('long_artist');
    await emit('resize', { width:760, height:540 }); await wait(() => innerWidth === 760 && innerHeight === 540 && performance.now() - resizedAt >= 500, 'long small resize settled'); await capture('long_artist_small');
    click(button('播放全部', one('.artist-actions'))); await wait(playing, 'long artist playback'); await capture('long_artist_playing', 50);
    await emit('complete', { ok:true, stageCount:count, realMusicRequests:0, playbackVerified:true, decoder:'libmpv', audioOutput:'null', preservedPrefixes:true, mediaSubstitution:'exact synthetic CDN URL to generated PCM' });
  } catch(error) {
    const grid = one('.catalog-card-scroll');
    await emit('failed', { ok:false, stageCount:count, message:String(error), stack:error.stack,
      selected: one('.artist-tabs [aria-selected="true"]')?.dataset.catalogTab,
      grid: grid ? { scrollTop:grid.scrollTop, scrollHeight:grid.scrollHeight,
        height:grid.clientHeight, width:grid.clientWidth, total:grid.dataset.catalogTotal,
        first:one('.catalog-card strong')?.textContent, last:all('.catalog-card strong').at(-1)?.textContent } : null });
  }
})();
