// Real Vue rows and native detail routes with sealed search responses.
window.__searchInlineCreditsRun = async ({ one, wait, button, click, fresh, choose, capture, fixture, assert, ready }) => {
  const panel = () => one('#search-result-panel');
  const active = () => one('[data-category][aria-selected="true"]')?.dataset.category;
  const row = (id) => panel()?.querySelector(`.track-row[data-track-id="${id}"]`);
  const suffix = (root, part) => root?.querySelector(`[data-search-inline-credit="${part}"]`);
  const artist = () => row('31401')?.querySelector('[data-result-action="artist:31402"]');
  const album = () => row('31401')?.querySelector('[data-result-action="album"]');
  const stageReady = (category) => active() === category && ready() &&
    !one('.page-leave-active,.page-enter-active') && row('31401') && row('31404');
  const marked = (node) => node ? {
    text: node.textContent.trim(),
    marks: [...node.querySelectorAll('mark')].map((mark) => mark.textContent),
  } : null;

  function expectedSuffix(root, kind, text) {
    const node = suffix(root, kind);
    const value = marked(node);
    assert(value?.text === `（${text}）` && value.marks.length === 1 && value.marks[0] === '合成' &&
      node.querySelector('mark').classList.contains('search-match-text'),
    `exact ${kind} suffix ${text}: ${JSON.stringify(value)}`);
    return value;
  }

  function evidence(category, focusAction = null) {
    assert(stageReady(category), `exact dedicated ${category} visit is visible`);
    const positive = row('31401'), negative = row('31404');
    const title = positive.querySelector('.track-row-title-text');
    const artistLink = artist(), albumLink = album();
    const plainArtist = [...positive.querySelectorAll('.track-credit-links > span')]
      .find((node) => node.textContent.trim() === '无链接艺人');
    const negativeAlbum = negative.querySelector('.track-album');
    assert(title?.textContent.trim() === '主曲名（合成译名）' &&
      artistLink?.textContent.trim() === '主艺人（合成译艺人）' &&
      albumLink?.textContent.trim() === '主专辑（合成译专辑）',
    'native title/artist/album suffixes remain inside their original cells and controls');
    assert(plainArtist && !plainArtist.closest('button') && !suffix(plainArtist, 'artist') &&
      positive.querySelectorAll('[data-search-inline-credit="artist"]').length === 1,
    'artist without an ID stays plain, independent of linked artist translation');
    assert(!suffix(negative, 'title') && !suffix(negative, 'artist') &&
      !negative.querySelector('.track-row-title-text')?.textContent.includes('合成不内联别名') &&
      !positive.querySelector('.track-row-title-text')?.textContent.includes('合成不内联别名'),
    'search song aliases are never substituted for the inline song translation');
    assert(negativeAlbum?.textContent.trim() === '无链接专辑（合成无链接译专辑）' &&
      !negativeAlbum.querySelector('button,a'), 'album without an ID keeps the suffix and has no link');
    const rows = {
      positive: {
        id: positive.dataset.trackId,
        title: expectedSuffix(title, 'title', '合成译名'),
        artist: expectedSuffix(artistLink, 'artist', '合成译艺人'),
        album: expectedSuffix(albumLink, 'album', '合成译专辑'),
        idlessArtistPlain: true,
      },
      negative: {
        id: negative.dataset.trackId,
        title: null,
        album: expectedSuffix(negativeAlbum, 'album', '合成无链接译专辑'),
        idlessArtistPlain: !negative.querySelector('[data-result-action^="artist:"]'),
      },
    };
    assert(rows.negative.idlessArtistPlain, 'negative artist row has no fabricated link');
    const footer = marked(positive.querySelector('[data-search-title-lyric]'));
    const excerpt = marked(positive.closest('.search-lyric-result')?.querySelector('.search-lyric-lines'));
    if (category === 'songs') {
      assert(!excerpt && footer?.text === '题合成词' && footer.marks.join(',') === '合成',
        'Songs keeps the separate server-marked lyric footer');
    } else {
      assert(!footer && excerpt?.text === '合成歌词保持原文\n第二行' && !excerpt.marks.length,
        'Lyrics keeps its separate multiline excerpt as plain text');
    }
    return { category, rows, footer, excerpt, focusAction };
  }

  async function visit(category, kind, id, heading, control) {
    const anchor = control();
    const target = suffix(anchor, kind)?.querySelector('mark');
    assert(anchor && target && !anchor.disabled, `visible ${kind} label within the original button`);
    const sourceAction = anchor.dataset.resultAction;
    anchor.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'instant' });
    await wait(() => {
      const bounds = target.getBoundingClientRect();
      return anchor === control() && target.isConnected && bounds.width > 0 && bounds.height > 0 &&
        anchor.contains(document.elementFromPoint(bounds.left + bounds.width / 2, bounds.top + bounds.height / 2));
    }, `${kind} inline label is actually within the visible button hit area`);
    anchor.focus({ preventScroll: true });
    assert(document.activeElement === anchor, `${kind} credit remains keyboard focusable`);
    // Click the nested label itself. Its event must reach the existing button.
    target.click();
    await wait(() => {
      const page = one(`.${kind}-header h1`);
      return page?.textContent.trim() === heading && !page.closest('[inert]') &&
        !one('.page-enter-active,.page-leave-active');
    }, `native ${kind} detail ${id}`);
    await capture(`inline_credits_${kind}`, { category, sourceAction, destination: { kind, id, heading } });
    click(button('返回上一页'));
    await wait(() => stageReady(category) && document.activeElement === control(),
      `Back restores the exact ${category} ${kind} link focus`);
    await capture(`inline_credits_${category}_back`, evidence(category, document.activeElement.dataset.resultAction));
  }

  await wait(() => one('.track-row'), 'daily ready');
  await fixture('inline_credits');
  await fresh('合成');
  assert(!panel().querySelector('[data-search-inline-credit]'), 'comprehensive preview is unchanged');
  await choose('songs');
  await wait(() => stageReady('songs'), 'exact dedicated Song rows');
  await capture('inline_credits_songs', evidence('songs'));
  await visit('songs', 'artist', '31402', '主艺人', artist);

  await choose('lyrics');
  await wait(() => stageReady('lyrics'), 'exact dedicated Lyric rows');
  await capture('inline_credits_lyrics', evidence('lyrics'));
  await visit('lyrics', 'album', '31403', '主专辑', album);
  return { searchInlineCreditsVerified: true };
};
