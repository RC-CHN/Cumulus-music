// Native primary name parts projected into the production Songs and Lyrics rows.
window.__searchPrimaryMarksRun = async ({ one, wait, button, click, fresh, choose, capture, fixture, assert, ready }) => {
  const raw = '原词', corrected = 'Ａ＋Ｂ 樂隊';
  const names = {
    title: 'Ａ＋Ｂ AB A+B 夜场 原词 错误词',
    artist: '樂隊 乐队 + A+B 原词',
    idlessArtist: '无链接 樂隊 乐队 + A+B 原词',
    album: '<img> 乐队 / A+B 原词',
  };
  const panel = () => one('#search-result-panel');
  const row = () => panel()?.querySelector('.track-row[data-track-id="31501"]');
  const active = () => one('[data-category][aria-selected="true"]')?.dataset.category;
  const settled = () => !one('.page-leave-active,.page-enter-active');
  const readyCategory = (category) => active() === category && ready() && settled();
  const primary = (root, kind) => root?.querySelector(`[data-search-primary-match="${kind}"]`);
  const artistLink = () => row()?.querySelector('button[data-result-action="artist:31502"]');
  const correction = () => one('.search-correction');

  function observed(node) {
    return node ? { text: node.textContent.trim(), marks: [...node.querySelectorAll('mark')].map((mark) => mark.textContent) } : null;
  }
  function exact(node, text, marks, label) {
    const actual = observed(node);
    assert(actual?.text === text && JSON.stringify(actual.marks) === JSON.stringify(marks) &&
      [...node.querySelectorAll('mark')].every((mark) => mark.classList.contains('search-match-text')),
    `${label}: ${JSON.stringify(actual)}`);
    return actual;
  }

  function evidence(category, rewritten, focusAction = null) {
    assert(readyCategory(category) && row(), `visible dedicated ${category} visit`);
    assert(rewritten ? correction()?.textContent.includes(corrected) : !correction(),
      'correction status matches the active visit');
    const title = primary(row(), 'title');
    const artist = primary(artistLink(), 'artist');
    const idlessArtist = [...row().querySelectorAll('[data-search-primary-match="artist"]')]
      .find((node) => !node.closest('button,a'));
    const album = primary(row().querySelector('.track-album'), 'album');
    const exactMarks = {
      title: exact(title, names.title, rewritten ? ['A+B'] : [raw], 'native title segments'),
      artist: exact(artist, names.artist, rewritten ? ['乐队', 'A+B'] : [raw], 'native linked artist segments'),
      idlessArtist: exact(idlessArtist, names.idlessArtist, [], 'unlinked artist stays plain even when original word matches'),
      album: exact(album, names.album, rewritten ? ['乐队', 'A+B'] : [raw], 'native ID-less album segments'),
    };
    const albumCell = album.closest('.track-album');
    const noAlbumLink = !albumCell.querySelector('button,a') && !album.closest('button,a');
    const literalMarkup = !albumCell.querySelector('img') && album.textContent.startsWith('<img>');
    assert(noAlbumLink && literalMarkup && !row().querySelector('[data-search-inline-credit]'),
      'ID-less album is safe plain markup text without a fabricated route or secondary label');
    const footer = observed(row().querySelector('[data-search-title-lyric]'));
    const excerpt = observed(row().closest('.search-lyric-result')?.querySelector('.search-lyric-lines'));
    if (category === 'songs') {
      assert(!excerpt && footer?.text === '前原词后' && footer.marks.join(',') === raw,
        'independent Songs footer still uses the service range rather than the effective name query');
    } else {
      assert(!footer && excerpt?.text === 'Ａ＋Ｂ 乐队 A+B 原词\n第二行 <img>' && !excerpt.marks.length &&
        !row().closest('.search-lyric-result').querySelector('.search-lyric-lines img'),
      'Lyrics keeps its raw multiline excerpt and literal markup without primary-name marks');
    }
    return { category, keyword: rewritten ? corrected : raw, id: row().dataset.trackId, exactMarks,
      noAlbumLink, literalMarkup, footer, excerpt, focusAction };
  }

  await wait(() => one('.track-row'), 'daily ready');
  await fixture('primary_marks');
  await fresh(raw);
  const preview = () => panel()?.querySelector('.search-track-card[data-result-key="song:31501"]');
  await wait(() => readyCategory('all') && preview(), 'same synthetic Song is visible in comprehensive preview');
  assert(!panel().querySelector('[data-search-primary-match]') &&
    preview().querySelector('.search-track-title mark')?.textContent === raw,
  'comprehensive preview retains its own primary label policy');

  await choose('songs');
  await wait(() => readyCategory('songs') && row() && correction(), 'corrected Songs visit');
  await capture('primary_marks_songs_corrected', evidence('songs', true));
  const link = artistLink(), mark = primary(link, 'artist')?.querySelector('mark');
  await wait(() => {
    const box = mark?.getBoundingClientRect();
    return box?.width > 0 && box.height > 0 &&
      link.contains(document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2));
  }, 'marked artist primary remains in the real hit area of its original link');
  link.focus({ preventScroll: true });
  assert(document.activeElement === link, 'exact artist link is keyboard focusable');
  mark.click();
  await wait(() => one('.artist-header h1')?.textContent.trim() === names.artist && settled(),
    'primary name click opens the native linked artist');
  await capture('primary_marks_artist', { sourceAction: link.dataset.resultAction, id: '31502', heading: names.artist });
  click(button('返回上一页'));
  await wait(() => readyCategory('songs') && artistLink() && document.activeElement === artistLink(),
    'Back restores the exact marked artist control');
  await capture('primary_marks_songs_back', evidence('songs', true, document.activeElement.dataset.resultAction));

  await choose('lyrics');
  await wait(() => readyCategory('lyrics') && row(), 'Lyrics uses the effective request');
  await capture('primary_marks_lyrics_effective', evidence('lyrics', true));

  click(button('仍然搜索“原词”'));
  await wait(() => readyCategory('all') && preview() && !correction(), 'original-word comprehensive visit');
  await choose('songs');
  await wait(() => readyCategory('songs') && row(), 'original-word Songs visit');
  await capture('primary_marks_songs_original', evidence('songs', false));
  await choose('lyrics');
  await wait(() => readyCategory('lyrics') && row(), 'original-word Lyrics visit');
  await capture('primary_marks_lyrics_original', evidence('lyrics', false));
  return { searchPrimaryMarksVerified: true, comprehensiveRetained: true };
};
