// Real WebView geometry and hit testing; the native host supplies synthetic reads.
(async () => {
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const one = selector => document.querySelector(selector);
  const all = selector => [...document.querySelectorAll(selector)];
  const assert = (condition, message) => { if (!condition) throw Error(message); };
  const emit = (stage, rest = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...rest },
  });
  const wait = async (predicate, label) => {
    for (let i = 0; i < 150; i++) { if (predicate()) return; await sleep(100); }
    throw Error(label);
  };
  const button = (text, scope = document) => [...scope.querySelectorAll('button')].find(node => node.textContent.trim() === text);
  const click = node => { assert(node && !node.disabled, `missing or disabled control: ${node?.outerHTML}`); node.click(); };
  const rect = node => {
    const r = node.getBoundingClientRect();
    return { x: r.x, y: r.y, right: r.right, bottom: r.bottom, width: r.width, height: r.height };
  };
  const hittable = node => {
    const r = rect(node);
    return r.y >= 0 && r.bottom <= innerHeight && document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2)?.closest('button') === node;
  };
  let stageCount = 0;
  const failures = [];
  async function capture(name, checks = {}) {
    await sleep(500);
    const page = one('.catalog-page, .comments-page');
    const cards = all('.catalog-card');
    checks.documentContained = document.documentElement.scrollWidth <= innerWidth;
    checks.pageContained = !!page && rect(page).bottom <= rect(one('.main-content')).bottom + 1;
    checks.boundedCards = cards.length <= 30;
    for (const [check, passed] of Object.entries(checks)) if (!passed) failures.push(`${name}: ${check}`);
    await emit(`ux_${String(++stageCount).padStart(2, '0')}_${name}`, {
      width: innerWidth, height: innerHeight, theme: document.documentElement.dataset.theme,
      checks, page: rect(page), scrollHeight: page.scrollHeight, clientHeight: page.clientHeight,
      artwork: cards.map(card => rect(card.querySelector('img, .catalog-card-placeholder'))),
      footer: one('.comment-pagination') ? rect(one('.comment-pagination')) : null,
    });
    await sleep(900);
  }
  async function theme(value) {
    click(one('[aria-label="外观主题"]'));
    await wait(() => one('.action-menu'), 'theme menu');
    click(button(value === 'light' ? '浅色' : '深色'));
    await wait(() => document.documentElement.dataset.theme === value && !one('.action-menu'), 'theme applied');
  }
  async function artist() {
    click(button('每日推荐'));
    await sleep(500);
    await wait(() => one('.track-credit-links button'), 'daily artist');
    click(one('.track-credit-links button'));
    await wait(() => one('.catalog-heading h1')?.textContent === '测试歌手', 'artist detail');
    await sleep(400);
  }
  async function albums() {
    click(button('专辑', one('.catalog-tabs')));
    await wait(() => all('.catalog-card').length >= 2, 'album cards');
    const page = one('.catalog-page');
    one('.catalog-tabs').scrollIntoView({ block: 'start' });
    await sleep(500);
    return page;
  }
  function cardChecks() {
    const cards = all('.catalog-card');
    return {
      squareArtwork: cards.every(card => {
        const r = rect(card.querySelector('img, .catalog-card-placeholder'));
        return Math.abs(r.width - r.height) < 1;
      }),
      titleHints: cards.every(card => card.querySelector('strong').title === card.querySelector('strong').textContent),
      noInternalOverflow: one('.catalog-card-scroll').scrollWidth <= one('.catalog-card-scroll').clientWidth + 1,
    };
  }
  try {
    await wait(() => one('.track-credit-links button'), 'daily ready');
    for (const [width, height, color] of [[1120, 760, 'light'], [760, 540, 'light'], [760, 540, 'dark'], [1120, 760, 'dark']]) {
      await emit('resize', { width, height });
      await wait(() => innerWidth === width && innerHeight === height, 'resize');
      await theme(color);
      await artist();
      const art = one('.catalog-cover');
      await capture(`artist_${width}_${color}`, { roundPlaceholder: getComputedStyle(art).borderTopLeftRadius === '50%' });
      await albums();
      await capture(`albums_${width}_${color}`, cardChecks());
      const grid = one('.catalog-card-scroll');
      grid.scrollTop = grid.scrollHeight; await sleep(500);
      await capture(`albums_bottom_${width}_${color}`, {
        ...cardChecks(), lastCardRendered: all('.catalog-card strong').at(-1)?.textContent === '合成专辑 26',
      });
      grid.scrollTop = 0; await sleep(500);
      click(all('.catalog-card')[1]);
      await wait(() => one('.catalog-heading h1')?.textContent.startsWith('漫长的午后'), 'long album');
      await sleep(400);
      await capture(`album_long_${width}_${color}`, {
        metadataContained: one('.catalog-heading').scrollWidth <= one('.catalog-heading').clientWidth + 1,
        actionsContained: all('.catalog-heading button').every(node => rect(node).right <= rect(one('.catalog-page')).right + 1),
      });
      click(one('[role="tab"][id$="-comments"]'));
      await wait(() => all('[data-group="page"] .comment-card').length === 20, 'comment page');
      const comments = one('.catalog-page');
      comments.scrollTop = 0; one('.comments-page').scrollIntoView({ block: 'start' }); await sleep(400);
      await capture(`comments_top_${width}_${color}`, {
        namesContained: all('.comment-author').every(node => node.scrollWidth <= node.clientWidth + 1),
        headerVisible: rect(one('.comments-page .page-heading')).y >= rect(one('.main-content')).y,
      });
      comments.scrollTop = comments.scrollHeight; await sleep(400);
      const readingPosition = comments.scrollTop;
      click(button('刷新评论')); await sleep(200);
      await wait(() => !button('刷新评论').disabled, 'comment refresh');
      await sleep(400);
      await capture(`comments_bottom_${width}_${color}`, {
        footerReachable: hittable(button('下一页', one('.comment-pagination'))),
        scrollMoved: comments.scrollTop > 0,
        mainStayedInPlace: one('.main-content').scrollTop === 0,
        refreshKeptPosition: Math.abs(comments.scrollTop - readingPosition) < 1,
      });
      click(button('下一页', one('.comment-pagination')));
      await wait(() => all('[data-group="page"] .comment-card').length === 5, 'next comments');
      await sleep(400);
      await capture(`comments_next_${width}_${color}`, {
        nextPageStartsVisible: hittable(button('刷新评论')),
        pageIds: one('[data-group="page"] .comment-card').dataset.commentId === '21',
      });
    }
    await artist(); await albums();
    for (const [width, height] of [[760, 540], [1120, 760]]) {
      const grid = one('.catalog-card-scroll');
      grid.scrollTop = grid.scrollHeight; await sleep(300);
      await emit('resize', { width, height });
      await wait(() => innerWidth === width && innerHeight === height, 'resize loaded grid');
      await sleep(500);
      grid.scrollTop = grid.scrollHeight; await sleep(500);
      await capture(`grid_resized_${width}`, {
        ...cardChecks(), lastCardRendered: all('.catalog-card strong').at(-1)?.textContent === '合成专辑 26',
        labelsInsideCards: all('.catalog-card').every(card => rect(card.querySelector('small')).bottom <= rect(card).bottom),
      });
    }
    assert(!failures.length, failures.join('; '));
    await emit('complete', { ok: true, stageCount, realMusicRequests: 0, failures, playbackVerified: false, layoutVerified: true });
  } catch (error) {
    await emit('failed', { ok: false, stageCount, error: String(error), stack: error.stack, failures });
  }
})();
