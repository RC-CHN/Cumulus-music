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
    click(button('歌单广场', one('.sidebar')));
    const catalogue = () => one('.playlist-catalogue-panel');
    const more = () => one('.playlist-catalogue-trigger');
    const heading = () => one('.playlist-tag-browse .page-heading p')?.textContent;
    const feed = () => one('.playlist-square-feed');
    const groups = () => [...document.querySelectorAll('.playlist-square-group')];
    await wait(() => heading() === '推荐' && groups().length === 1 &&
      one('[data-square-card="0:3"]') && button('合成推荐分组：向右浏览', feed()),
      'discovery defaults to the grouped recommendation route');
    assert(!feed().textContent.includes('不显示活动') && cards().length === 0, 'only known groups, no category alias');
    const rail = () => groups()[0].querySelector('.playlist-square-rail');
    click(button('合成推荐分组：向右浏览', feed()));
    await wait(() => rail().scrollLeft > 24, 'group controls browse independently');
    await emit('ux_06_recommended');
    click(button('加载更多推荐歌单'));
    await wait(() => button('重新加载推荐') && groups().length === 1, 'continuation failure keeps the first group');
    click(button('重新加载推荐'));
    await wait(() => groups().length === 2 && !button('加载更多推荐歌单'), 'continuation appends second group in service order');
    assert(groups().map((group) => group.querySelector('h2').textContent).join('|') === '合成推荐分组|合成下一分组',
      'both official group shapes preserve titles and order');
    const source = () => one('[data-square-card="0:3"]');
    assert(one('[data-square-card="1:3"]'), 'the same playlist can occur in two different groups');
    click(source());
    await wait(() => one('.playlist-hero h1')?.textContent === '合成歌单 3', 'recommended card opens exact detail');
    click(button('返回上一页'));
    await wait(() => groups().length === 2 && document.activeElement === source() && rail().scrollLeft > 24,
      'Back restores loaded groups, the exact duplicate instance and horizontal position');
    await emit('ux_07_recommended_back');
    await emit('playlist-square-disable');
    await wait(() => window.__squareDisabledAck, 'synthetic privacy change is installed');
    click(button('刷新推荐'));
    await wait(() => feed()?.textContent.includes('个性化服务已关闭') && groups().length === 0 &&
      button('打开个性化设置'), 'refresh honors disabled recommendation setting');
    await emit('ux_08_recommended_disabled');
    click(button('官方', one('.playlist-catalogue-controls')));
    await wait(() => cards().length === 24 && heading() === '官方', 'discovery enters official category');
    await emit('ux_04_square');
    click(more());
    await wait(() => catalogue()?.textContent.includes('网络请求失败') &&
      button('重新加载', catalogue()), 'catalogue error can retry');
    click(button('重新加载', catalogue()));
    await wait(() => catalogue() && button('语种', catalogue()) && button('华语', catalogue()),
      'catalogue uses the first official group and displays its tags');
    assert(!button('官方', catalogue()) && !button('推荐', catalogue()),
      'outside and personalized tags do not become normal categories');
    click(button('风格', catalogue()));
    await wait(() => button('摇滚精选', catalogue()) && button('民谣', catalogue()) &&
      !button('华语', catalogue()), 'choose the next group without mixing category tags');
    await emit('ux_05_catalogue');
    click(button('摇滚精选', catalogue()));
    await wait(() => !catalogue() && heading() === '摇滚' && cards().length === 24,
      'display label selects raw category');
    click(more());
    await wait(() => catalogue() && button('摇滚精选', catalogue())?.getAttribute('aria-pressed')==='true',
      'reopening restores the group and selected tag');
    assert(button('风格', catalogue()).getAttribute('aria-pressed')==='true', 'restored exact group');
    catalogue().dispatchEvent(new KeyboardEvent('keydown', {key:'Escape',bubbles:true,cancelable:true}));
    await wait(() => !catalogue() && document.activeElement===more(), 'Escape restores trigger focus');
    await emit('complete', { ok: true, catalogueVerified: true, recommendationVerified: true });
  } catch (error) {
    await emit('failed', { message: String(error) });
  }
})();
