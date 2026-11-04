// Production tag navigation and both official read shapes via a synthetic API.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const button = (label, scope = document) =>
    [...scope.querySelectorAll('button')].find(
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
  async function capture(stage) {
    await wait(() => !one('.page-enter-active, .page-leave-active'), 'page transition settles: ' + stage);
    await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));
    return emit(stage);
  }
  const cards = () => [...document.querySelectorAll('[data-tag-playlist-id]')].map((item) => item.dataset.tagPlaylistId);
  const loaded = () => Number(one('.playlist-tag-window')?.dataset.loadedCount);
  const scroller = () => one('.playlist-tag-browse');
  const nearEnd = (gap = 50) => {
    const viewport = scroller();
    viewport.scrollTop = viewport.scrollHeight - viewport.clientHeight - gap;
  };
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
    await capture('ux_01_source');
    click(one('.playlist-hero-tags button'));
    await wait(() => cards().length === 24, 'first bounded page');
    const topPills = () => [...document.querySelectorAll('.playlist-catalogue-top')];
    await wait(() => button('重试推荐标签'), 'top tags failure does not block ordinary lists');
    assert(!topPills().length && cards().length === 24, 'ordinary browsing stays usable without personalized tags');
    assert(one('.playlist-tag-browse').textContent.includes('轻音乐'), 'tag preserved');
    const promo = () => one('.playlist-essential-promo');
    assert(promo()?.textContent.includes('合成精品入口') && promo() === one('.playlist-tag-cards button'),
      'high-quality promotion is the first category card');
    assert(document.activeElement === one('.playlist-tag-browse h1'), 'category focus');
    await capture('ux_02_first');
    assert(scroller().scrollHeight > scroller().clientHeight, 'the ordinary category has a native vertical scroller');
    nearEnd(95);
    await new Promise((done) => setTimeout(done, 220));
    assert(loaded() === 24 && !button('重试', scroller()), '95 pixels from the bottom does not request another page');
    nearEnd();
    await wait(() => loaded() === 24 && button('重试', scroller()), 'scroll threshold starts continuation; failure keeps prefix');
    click(button('重试', scroller()));
    await wait(() => loaded() === 48 && cards().indexOf('144') === cards().indexOf('145') + 1 &&
      cards().includes('145'), 'authoritative continuation order appends to the first page');
    await capture('ux_03_next');
    const firstContinuation = () => one('[data-tag-playlist-id="145"]');
    firstContinuation().scrollIntoView({block:'center'});
    const ordinaryScroll = scroller().scrollTop;
    click(firstContinuation());
    await wait(() => one('.playlist-hero h1')?.textContent === '合成歌单 145', 'selected playlist detail');
    click(button('返回上一页'));
    await wait(() => loaded() === 48 && document.activeElement === firstContinuation() &&
      Math.abs(scroller().scrollTop - ordinaryScroll) < 4, 'Back restores accumulated results, exact scroll and focus');
    for (let size = 72; size <= 240; size += 24) {
      nearEnd();
      await wait(() => loaded() === size, 'automatic ordinary continuation reaches ' + size);
    }
    nearEnd(0);
    await wait(() => cards().includes('314'), 'terminal ordinary card is mounted at the bottom');
    assert(cards().length < 90 && !cards().includes('100') && cards().includes('314') &&
      !button('加载更多歌单', scroller()), '240 items remain reachable with bounded DOM and no terminal request');
    await capture('ux_15_ordinary_long');
    scroller().scrollTop = 0;
    await wait(() => promo() && cards().includes('100') && loaded() === 240, 'scrolling to top restores first ordinary entries and promo');
    click(promo());
    const essential = () => one('.playlist-essential-page');
    const essentialCards = () => [...document.querySelectorAll('[data-essential-card]')];
    const essentialTitle = () => one('.playlist-essential-current')?.textContent;
    const moreCategories = () => button('更多分类', essential());
    await wait(() => essential() && essentialTitle() === '轻音乐' && essentialCards().length === 36,
      'promotion opens current category even with catalogue failure');
    click(moreCategories());
    await wait(() => button('重试分类', essential()), 'category error has independent retry');
    await capture('ux_09_essential_initial');
    click(button('重试分类', essential()));
    await wait(() => button('全部分类', essential()) && button('轻音乐精选', essential()), 'official category names');
    assert(button('轻音乐精选', essential()).getAttribute('aria-pressed') === 'true', 'selected raw category');
    await capture('ux_10_essential_categories');
    one('#playlist-essential-categories').dispatchEvent(new KeyboardEvent('keydown', {key:'Escape',bubbles:true,cancelable:true}));
    await wait(() => !one('#playlist-essential-categories') && document.activeElement===moreCategories(), 'category popup restores focus');
    click(moreCategories());
    await wait(() => one('#playlist-essential-categories'), 'reopen category popup for pointer dismissal');
    essential().querySelector('h1').dispatchEvent(new PointerEvent('pointerdown', {bubbles:true,pointerType:'mouse'}));
    await wait(() => !one('#playlist-essential-categories'), 'clicking outside closes the category popup');
    button('加载更多精品歌单', essential()).scrollIntoView({block:'end'});
    await wait(() => essentialCards().length === 36 && button('重试精品歌单', essential()), 'scroll loads exact cursor; failure keeps prior cards');
    click(button('重试精品歌单', essential()));
    await wait(() => essentialCards().length === 37 && !button('加载更多精品歌单', essential()), 'retry appends and respects terminal');
    const essentialCard = () => one('[data-essential-card="3"]');
    click(essentialCard());
    await wait(() => one('.playlist-hero h1')?.textContent === '合成歌单 3', 'opens exact essential playlist');
    click(button('返回上一页'));
    await wait(() => essentialCards().length === 37 && document.activeElement === essentialCard(), 'Back restores loaded prefix and exact essential card');
    await capture('ux_11_essential_return');
    click(moreCategories());
    await wait(() => button('全部分类', essential()), 'reopen category popup');
    click(button('全部分类', essential()));
    await wait(() => essentialTitle() === '全部分类' && essentialCards().length === 1 &&
      one('[data-essential-card="4"]'), 'display label selects raw 全部');
    await capture('ux_12_essential_all');
    click(button('返回上一页'));
    await wait(() => essentialTitle() === '轻音乐精选' && essentialCards().length === 37,
      'category Back restores first category without another native read');
    click(button('返回上一页'));
    await wait(() => loaded() === 240 && document.activeElement === promo(), 'return to loaded ordinary category and exact promo focus');
    click(button('重试推荐标签'));
    await wait(() => topPills().map((pill) => pill.textContent.trim()).join('|') === '民谣|纯音乐',
      'top tags preserve raw names and service order without duplicate fixed pills');
    await capture('ux_13_top_tags');
    click(button('歌单广场', one('.sidebar')));
    const catalogue = () => one('.playlist-catalogue-panel');
    const more = () => one('.playlist-catalogue-trigger');
    const heading = () => one('.playlist-tag-browse .page-heading p')?.textContent;
    const feed = () => one('.playlist-square-feed');
    const groups = () => [...document.querySelectorAll('.playlist-square-group')];
    const loadedGroups = () => Number(one('.playlist-square-window')?.dataset.loadedGroups);
    await wait(() => heading() === '推荐' && loadedGroups() === 3 &&
      one('[data-square-card="0:3"]') && button('合成推荐分组：向右浏览', feed()),
      'discovery defaults to the grouped recommendation route');
    assert(!feed().textContent.includes('不显示活动') && cards().length === 0, 'only known groups, no category alias');
    const rail = () => groups()[0].querySelector('.playlist-square-rail');
    click(button('合成推荐分组：向右浏览', feed()));
    await wait(() => rail().scrollLeft > 24, 'group controls browse independently');
    await capture('ux_06_recommended');
    const railScroll = rail().scrollLeft;
    nearEnd();
    await wait(() => button('重新加载推荐') && loadedGroups() === 3, 'scroll starts recommendation continuation and preserves first groups on failure');
    click(button('重新加载推荐'));
    await wait(() => loadedGroups() === 11, 'recommendation retry appends grouped results');
    for (const size of [19,27]) {
      nearEnd();
      await wait(() => loadedGroups() === size, 'automatic recommendation continuation reaches ' + size);
    }
    assert(groups().length < 14 && !one('[data-square-card="0:3"]') && !button('加载更多推荐歌单'),
      '27 groups retain scroll history with a bounded DOM and honor the terminal cursor');
    await capture('ux_16_recommendation_long');
    scroller().scrollTop = 0;
    await wait(() => one('[data-square-card="0:3"]') && one('[data-square-card="3:3"]'),
      'scrolling back reveals both original duplicate instances without fetching');
    assert(groups()[0].querySelector('h2').textContent === '合成推荐分组' &&
      groups()[3].querySelector('h2').textContent === '合成下一分组', 'both official group shapes preserve titles and order');
    const source = () => one('[data-square-card="0:3"]');
    assert(one('[data-square-card="3:3"]'), 'the same playlist can occur in two different groups');
    click(source());
    await wait(() => one('.playlist-hero h1')?.textContent === '合成歌单 3', 'recommended card opens exact detail');
    click(button('返回上一页'));
    await wait(() => loadedGroups() === 27 && document.activeElement === source() && Math.abs(rail().scrollLeft - railScroll) < 4,
      'Back restores loaded groups, the exact duplicate instance and horizontal position');
    await capture('ux_07_recommended_back');
    click(button('民谣', one('.playlist-catalogue-controls')));
    await wait(() => heading() === '民谣' && cards().length === 24 &&
      button('民谣', one('.playlist-catalogue-controls')).getAttribute('aria-pressed') === 'true',
      'top tag opens the exact ordinary category');
    click(more());
    await wait(() => catalogue()?.textContent.includes('网络请求失败') &&
      button('重新加载', catalogue()), 'catalogue error can retry independently');
    click(button('重新加载', catalogue()));
    await wait(() => catalogue() && button('语种', catalogue()) && button('华语', catalogue()),
      'non-promoted categories still display');
    click(button('风格', catalogue()));
    await wait(() => button('摇滚精选', catalogue()) && !button('民谣', catalogue()),
      'More Tags excludes the promoted raw category without hiding its neighbors');
    await capture('ux_14_top_tags_dedup');
    catalogue().dispatchEvent(new KeyboardEvent('keydown', {key:'Escape',bubbles:true,cancelable:true}));
    await wait(() => !catalogue(), 'close the catalogue before leaving the category');
    click(button('返回上一页'));
    await wait(() => heading() === '推荐' && loadedGroups() === 27, 'top pill Back keeps the prior recommendation results');
    await emit('playlist-square-disable');
    await wait(() => window.__squareDisabledAck, 'synthetic privacy change is installed');
    click(button('刷新推荐'));
    await wait(() => feed()?.textContent.includes('个性化服务已关闭') && groups().length === 0 &&
      button('打开个性化设置') && !topPills().length, 'refresh clears recommendation and personalized top pills');
    const provides = one('#app').__vue_app__._context.provides;
    const core = provides[Reflect.ownKeys(provides).find((key) => typeof key === 'symbol' && key.description === 'core-client')];
    const disabled = await core.playlistTopTags((await core.accountSnapshot()).epoch);
    assert(!disabled.value.recommendationEnabled && !disabled.value.tags.length, 'native refuses the optional read while privacy is disabled');
    await capture('ux_08_recommended_disabled');
    click(button('官方', one('.playlist-catalogue-controls')));
    await wait(() => cards().length === 24 && heading() === '官方', 'discovery enters official category');
    await capture('ux_04_square');
    click(more());
    await wait(() => catalogue() && button('语种', catalogue()) && button('华语', catalogue()),
      'catalogue uses the first official group and displays its tags');
    assert(!button('官方', catalogue()) && !button('推荐', catalogue()),
      'outside and personalized tags do not become normal categories');
    click(button('风格', catalogue()));
    await wait(() => button('摇滚精选', catalogue()) && button('民谣', catalogue()) &&
      !button('华语', catalogue()), 'choose the next group without mixing category tags');
    await capture('ux_05_catalogue');
    click(button('摇滚精选', catalogue()));
    await wait(() => !catalogue() && heading() === '摇滚' && cards().length === 24,
      'display label selects raw category');
    click(more());
    await wait(() => catalogue() && button('摇滚精选', catalogue())?.getAttribute('aria-pressed')==='true',
      'reopening restores the group and selected tag');
    assert(button('风格', catalogue()).getAttribute('aria-pressed')==='true', 'restored exact group');
    catalogue().dispatchEvent(new KeyboardEvent('keydown', {key:'Escape',bubbles:true,cancelable:true}));
    await wait(() => !catalogue() && document.activeElement===more(), 'Escape restores trigger focus');
    await emit('complete', { ok: true, catalogueVerified: true, recommendationVerified: true, essentialVerified: true, topTagsVerified: true, waterfallVerified: true });
  } catch (error) {
    await emit('failed', { message: String(error), viewport: scroller()?.clientHeight,
      scrollTop: scroller()?.scrollTop, scrollHeight: scroller()?.scrollHeight,
      loaded: loaded() || 0, cards: cards(), active: document.activeElement?.getAttribute('aria-label') });
  }
})();
