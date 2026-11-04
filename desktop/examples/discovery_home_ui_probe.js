(async () => {
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => Array.from(document.querySelectorAll(selector));
  const assert = (ok, message) => { if (!ok) throw Error(message); };
  const emit = (stage, rest = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'discovery-home-proof', payload: { stage, ...rest },
  });
  async function wait(test, label) {
    for (let attempt = 0; attempt < 150; ++attempt) {
      if (test()) return;
      await sleep(100);
    }
    throw Error(label);
  }
  function click(node) {
    assert(node && !node.disabled, 'action unavailable');
    node.focus({ preventScroll: true });
    node.click();
  }
  const sidebarHome = () => all('.sidebar button').find((button) => button.textContent.trim() === '首页');
  const roots = () => all('[data-home-section]').map((element) => element.dataset.homeSection);
  const refresh = () => one('[aria-label="刷新首页"]');
  const retry = () => one('.discovery-home-error button');
  async function theme(label, value) {
    click(one('[aria-label="外观主题"]'));
    const option = () => all('button').find((button) => button.textContent.trim() === label);
    await wait(option, 'theme menu');
    click(option());
    await wait(() => document.documentElement.dataset.theme === value, 'theme change');
    await wait(() => !option(), 'theme menu dismissed');
  }
  async function capture(stage) {
    await sleep(360);
    await emit(stage, { sections: roots(), width: innerWidth, height: innerHeight });
    await sleep(1000);
  }
  try {
    await wait(() => sidebarHome() && !sidebarHome().disabled, 'sidebar home ready');
    click(sidebarHome());
    await wait(() => roots().length === 12, 'classic homepage sections with official ranking');
    assert(roots().join(',') === 'playlists,featured,rankings,all_listen,recent,heartbeat,radar,vip,style,daily_voice,personal_voices,books', 'classic home order');
    assert(one('[data-home-section=featured] [data-home-card]').dataset.homeCard === 'featured:daily:', 'daily comes first in featured');
    assert(!one('.discovery-home img[src*="private.invalid"]'), 'unsafe artwork');
    assert(document.documentElement.scrollWidth <= innerWidth, 'horizontal overflow');
    await wait(() => one('[aria-label="推荐歌单上一组"]')?.getAttribute('aria-disabled') === 'true', 'carousel edge state');
    assert(!one('[aria-label="最近常听下一组"]'), 'no inactive carousel arrows');
    await theme('浅色', 'light');
    await capture('home');
    await theme('深色', 'dark');
    await capture('dark');
    const style = one('[aria-label="打开 合成首页风格"]');
    style.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    await sleep(180);
    const styleRail = one('[data-home-rail=featured]');
    const styleOffset = styleRail.scrollLeft;
    click(style);
    await wait(() => !one('.discovery-home') && one('.main-content')?.textContent.includes('合成首页种子歌曲'), 'native seeded style list');
    await capture('seeded_style');
    click(one('[aria-label="返回上一页"]'));
    await wait(() => document.activeElement?.dataset.homeCard === style.dataset.homeCard &&
      Math.abs(one('[data-home-rail=featured]').scrollLeft - styleOffset) <= 2, 'style focus and horizontal position after Back');
    const ranks = () => one('[data-home-section=rankings]');
    ranks().scrollIntoView({ block: 'start' });
    assert(ranks().querySelectorAll('.ranking-card').length === 3, 'official rankings and two sales panels');
    assert(ranks().querySelectorAll('[data-rank-id="701"] .rank-preview-row').length === 3, 'only top three');
    assert(!ranks().textContent.includes('全球榜'), 'only official ranking group');
    await capture('ranking');
    click(one('[data-home-card="rankings:home"]'));
    await wait(() => one('.rankings-page h1')?.textContent === '排行榜' &&
      one('.rankings-page [data-rank-id="701"]'), 'rank heading opens rankings homepage');
    click(one('[aria-label="返回上一页"]'));
    await wait(() => document.activeElement?.dataset.homeCard === 'rankings:home', 'ranking heading focus after Back');
    click(one('[data-home-card="rankings:card:701"]'));
    await wait(() => !one('.discovery-home') && one('.main-content')?.textContent.includes('合成歌单701'), 'ranking opens its native playlist');
    click(one('[aria-label="返回上一页"]'));
    await wait(() => document.activeElement?.dataset.homeCard === 'rankings:card:701', 'ranking card focus after Back');
    one('.discovery-home').scrollTop = 0;
    const rail = one('[data-home-rail=playlists]');
    rail.scrollLeft = 80;
    one('.discovery-home').scrollTop = 40;
    const opener = one('[data-home-card="playlists:playlist:701"]');
    await sleep(150);
    click(opener);
    await wait(() => !one('.discovery-home') && document.querySelector('main')?.textContent.includes('合成歌单701'), 'open playlist');
    click(one('[aria-label="返回上一页"]'));
    await wait(() => one('[data-home-rail=playlists]')?.scrollLeft === 80, 'return restores rail scroll');
    await wait(() => one('.discovery-home')?.scrollTop === 40, 'return restores page scroll');
    await wait(() => document.activeElement?.dataset.homeCard === opener.dataset.homeCard, 'return restores card focus');

    const styleCard = one('[data-home-card^="recent:recent_style:"]');
    assert(styleCard && styleCard.textContent.includes('合成最近风格') &&
      !styleCard.outerHTML.includes('PRIVATE'), 'bounded recent style row');
    styleCard.scrollIntoView({ block: 'center' });
    await sleep(200);
    const beforeStyle = one('.discovery-home').scrollTop;
    click(styleCard);
    await wait(() => !one('.discovery-home') && one('.main-content h1')?.textContent === '风格推荐' &&
      one('.main-content')?.textContent.includes('合成风格歌曲'), 'recent style opens the unseeded style page');
    await capture('recent_style');
    click(one('[aria-label="返回上一页"]'));
    await wait(() => document.activeElement?.dataset.homeCard === styleCard.dataset.homeCard &&
      Math.abs(one('.discovery-home').scrollTop - beforeStyle) <= 1, 'recent style Back restores card and scroll');

    await emit('mode', { mode: 4 });
    click(refresh());
    await wait(() => !ranks() && !refresh()?.disabled, 'tag group hides ranking independently');
    assert(roots().length === 11, 'recommendations stay visible without ranking');
    await emit('mode', { mode: 1 });
    click(refresh());
    await wait(retry, 'first-party failure shows retry');
    assert(roots().includes('playlists'), 'last usable homepage kept while refresh failed');
    await wait(ranks, 'ranking updates independently while feed refresh fails');
    await capture('retry');
    await emit('mode', { mode: 3 });
    click(retry());
    await wait(() => roots().length === 10 && !retry(), 'optional sections omitted');
    assert(one('[data-home-section=playlists] [data-home-card]').dataset.homeCard === 'playlists:daily:', 'daily fallback to recommendations');
    await emit('mode', { mode: 2 });
    click(refresh());
    await wait(() => one('.discovery-home')?.textContent.includes('个性化推荐被关闭'), 'privacy disabled');
    assert(!roots().length, 'no stale personalized cards');
    await capture('privacy');
    click(one('.discovery-home .empty-state button'));
    await wait(() => document.activeElement?.id === 'settings-privacy' || one('#settings-privacy'), 'official privacy settings anchor');
    await emit('complete', { ok: true, realAccount: false });
  } catch (error) {
    await emit('failed', { ok: false, error: String(error), sections: roots(),
      page: one('.main-content')?.textContent.trim().slice(0, 300),
      active: document.activeElement?.outerHTML.slice(0, 300) });
  }
})();
