// Album tabs and persistent header in the production WebView, synthetic APIs only.
(async () => {
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const one = selector => document.querySelector(selector);
  const all = selector => [...document.querySelectorAll(selector)];
  const assert = (ok, message) => { if (!ok) throw Error(message); };
  const emit = (stage, extra = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...extra } });
  const wait = async (predicate, label) => { for (let i = 0; i < 180; i++) { if (predicate()) return; await sleep(100); } throw Error(label); };
  const button = (text, scope = document) => [...scope.querySelectorAll('button')].find(node => node.textContent.trim() === text);
  const click = node => { assert(node && !node.disabled, `unavailable control ${node?.outerHTML}`); node.scrollIntoView({ block: 'nearest' }); node.focus({ preventScroll: true }); node.click(); };
  const tab = key => one(`.album-tabs [id$="-${key}"]`);
  const select = async key => { click(tab(key)); await wait(() => tab(key)?.getAttribute('aria-selected') === 'true' && one(`[role="tabpanel"][aria-labelledby="${tab(key).id}"]`), `tab ${key}`); await sleep(400); assert(!one('[role="tabpanel"]').inert, 'active tab remained inert'); };
  const mode = async value => { await emit('fixture', { mode: value }); await wait(() => window.__CUMULUS_ALBUM_MODE__ === value, 'fixture acknowledgement'); };
  const playing = () => one('.playerbar')?.dataset.state === 'playing' && Number(one('.playerbar')?.dataset.position) > 0.2;
  let stageCount = 0;
  async function capture(name, nativePlayback = false) {
    await wait(() => !one('.modal-enter-active, .modal-leave-active'), 'modal transition settled');
    await sleep(300);
    assert(document.documentElement.scrollWidth <= innerWidth, 'document horizontal overflow');
    assert(one('.catalog-page').scrollWidth <= one('.catalog-page').clientWidth + 1, 'album horizontal overflow');
    assert(one('.album-header') && one('.album-tabs'), 'album header was lost');
    assert(all('.album-tabs [aria-selected="true"]').length === 1, 'ambiguous selected tab');
    assert(all('.album-tabs [tabindex="0"]').length === 1, 'tab stop count');
    assert(all('.track-row').length <= 30, 'unbounded songs');
    assert(one('.catalog-page').getBoundingClientRect().bottom <= one('.playerbar').getBoundingClientRect().top + 1, 'album overlaps player');
    const dialog = one('dialog[open]');
    const close = dialog?.querySelector('[aria-label*="关闭"]');
    if (dialog) {
      const box = dialog.getBoundingClientRect(), hit = close.getBoundingClientRect();
      assert(box.top >= 0 && box.bottom <= innerHeight + 1 && box.left >= 0 && box.right <= innerWidth + 1, 'modal outside window');
      assert(dialog.contains(document.elementFromPoint(hit.x + hit.width / 2, hit.y + hit.height / 2)), 'modal close button cannot be hit');
    }
    const stage = `ux_${String(++stageCount).padStart(2, '0')}_${name}`;
    window.__CUMULUS_ALBUM_CAPTURE_ACK__ = '';
    await emit(stage, {
      width: innerWidth, height: innerHeight, ratio: devicePixelRatio,
      theme: document.documentElement.dataset.theme, visible: document.visibilityState === 'visible', focused: document.hasFocus(),
      selected: one('.album-tabs [aria-selected="true"]').id.split('-').at(-1),
      title: one('.album-header h1').textContent, playing: nativePlayback,
      commentIds: all('[data-group="page"] .comment-card').map(node => node.dataset.commentId),
      draft: one('[aria-label="评论内容"]')?.value ?? '',
      dialog: dialog ? { rect: dialog.getBoundingClientRect().toJSON(), opacity: getComputedStyle(dialog).opacity, transform: getComputedStyle(dialog).transform } : null,
    });
    await wait(() => window.__CUMULUS_ALBUM_CAPTURE_ACK__ === stage, 'native capture acknowledgement');
    await sleep(250);
  }
  async function openAlbum(index = 1) {
    click(button('每日推荐')); await sleep(500); await wait(() => one('.track-credit-links button'), 'daily');
    click(one('.track-credit-links button')); await wait(() => one('.catalog-heading h1')?.textContent === '测试歌手', 'artist');
    click(button('专辑', one('.catalog-tabs'))); await wait(() => all('.catalog-card').length > index, 'albums');
    click(all('.catalog-card')[index]); await wait(() => one('.album-tabs'), 'album'); await sleep(400);
  }
  try {
    await wait(() => one('.track-credit-links button'), 'initial ready');
    for (const [index, [width, height, color]] of [[1120, 760, 'light'], [760, 540, 'light'], [760, 540, 'dark'], [1120, 760, 'dark']].entries()) {
      await emit('resize', { width, height }); await wait(() => innerWidth === width && innerHeight === height, 'resize');
      click(one('[aria-label="外观主题"]')); await wait(() => one('.action-menu'), 'theme menu'); click(button(color === 'light' ? '浅色' : '深色')); await wait(() => document.documentElement.dataset.theme === color, 'theme');
      await openAlbum();
      one('.catalog-page').scrollTop = 0;
      assert(all('.album-actions > button').map(n => n.textContent.trim()).join('|') === '播放全部|收藏|下载', 'header action order');
      await capture(`songs_${width}_${color}`);
      const first = tab('songs'); first.focus(); first.dispatchEvent(new KeyboardEvent('keydown', { key: 'End', bubbles: true }));
      await wait(() => tab('details').getAttribute('aria-selected') === 'true' && one('.album-description'), 'End selects details');
      assert(document.activeElement === tab('details'), 'keyboard focus not moved');
      assert(one('.album-description p').textContent.includes('\n'), 'multiline introduction lost');
      await capture(`details_${width}_${color}`);
      click(button('播放全部', one('.album-actions'))); await wait(playing, 'album native playback from details');
      await capture(`details_playing_${width}_${color}`, true);
      click(one('[aria-label="停止播放"]')); await wait(() => one('.playerbar').dataset.state !== 'playing', 'stop');
      if (index === 0) await mode('comments-offline');
      await select('comments');
      if (index === 0) {
        await wait(() => one('.comments-embedded [role="alert"]'), 'comments error');
        await capture('comments_read_failure');
        await mode('normal'); click(button('刷新评论'));
      }
      await wait(() => all('[data-group="page"] .comment-card').length === 20, 'album comments');
      assert(one('.comments-embedded') && !one('.comments-embedded h1'), 'duplicate page heading');
      assert(one('.album-tabs [id$="-comments"] small').textContent === '25', 'comment count');
      const draft = one('[aria-label="评论内容"]'); draft.value = `只保留草稿 ${index}`; draft.dispatchEvent(new Event('input', { bubbles: true }));
      await capture(`comments_${width}_${color}`);
      click(button('下一页', one('.comment-pagination'))); await wait(() => all('[data-group="page"] .comment-card').length === 5, 'comment second page');
      await select('songs'); await select('comments');
      assert(one('[aria-label="评论内容"]').value === `只保留草稿 ${index}`, 'draft lost during tab switch');
      one('[aria-label="评论内容"]').focus();
      assert(document.activeElement === one('[aria-label="评论内容"]'), 'cached comment panel could not take focus');
      assert(one('[data-group="page"] .comment-card').dataset.commentId === '21', 'comment page reset');
      await capture(`draft_return_${width}_${color}`);
      click(button('下载', one('.album-actions'))); await wait(() => one('dialog[open]'), 'download from comments');
      await capture(`download_${width}_${color}`);
      click(one('dialog[open] [aria-label*="关闭"]')); await wait(() => !one('dialog[open]'), 'close download');
    }
    click(one('[aria-label="更多专辑操作"]')); await wait(() => one('.action-menu'), 'album menu'); click(button('分享专辑'));
    await wait(() => one('[aria-label="专辑分享链接"]'), 'share album');
    assert(one('[aria-label="专辑分享链接"]').value === 'https://music.163.com/album?id=8', 'wrong public album link');
    const shareBody = one('.track-share-body');
    assert(shareBody.scrollWidth <= shareBody.clientWidth + 1, 'share content overflow');
    assert(all('.track-share-body > *').every(node => node.getBoundingClientRect().right <= shareBody.getBoundingClientRect().right + 1), 'share child clipped');
    await capture('share_public_link'); click(one('[aria-label="关闭分享"]'));
    await wait(() => !one('dialog[open]'), 'share close animation');
    assert(document.activeElement === one('[aria-label="更多专辑操作"]'), 'share did not return focus to menu');
    await mode('album-offline'); click(one('[aria-label="更多专辑操作"]')); await wait(() => one('.action-menu'), 'refresh menu'); click(button('刷新详情'));
    await wait(() => one('.catalog-error'), 'album refresh error');
    assert(one('.catalog-error').textContent.includes('保留上次内容'), 'failed refresh discarded metadata');
    const failureBox = one('.catalog-detail-error').getBoundingClientRect();
    assert(failureBox.top >= one('.main-content').getBoundingClientRect().top && failureBox.bottom <= one('.playerbar').getBoundingClientRect().top, 'refresh error is outside the visible page');
    assert(one('[aria-label="评论内容"]').value === '只保留草稿 3', 'failed refresh discarded comments draft');
    await capture('metadata_refresh_failure');
    await mode('normal'); click(button('重试', one('.catalog-error'))); await wait(() => !one('.catalog-error'), 'metadata retry');
    await sleep(400);
    assert(tab('comments').getAttribute('aria-selected') === 'true', 'metadata refresh reset selected tab');
    assert(one('[aria-label="评论内容"]').value === '只保留草稿 3', 'metadata refresh discarded comments draft');
    assert(one('[data-group="page"] .comment-card').dataset.commentId === '21', 'metadata refresh reset comment page');
    await capture('metadata_recovered');
    await openAlbum(2); await select('details');
    assert(one('.album-description p').textContent === '暂无专辑简介', 'empty introduction not explicit');
    assert(one('.catalog-credits button').textContent === '测试歌手', 'primary artist fallback lost');
    await capture('empty_description');
    click(one('.catalog-credits button')); await wait(() => one('.catalog-section-heading'), 'primary artist navigation');
    await openAlbum(0); await select('comments'); await wait(() => all('[data-group="page"] .comment-card').length === 20, 'other album comments');
    assert(one('[aria-label="评论内容"]').value === '', 'another album inherited draft');
    click(button('播放全部', one('.album-actions'))); await wait(playing, 'native playback from comments');
    await capture('comments_playing_other_album', true);
    await emit('complete', { ok: true, stageCount, realMusicRequests: 0, playbackVerified: true, decoder: 'libmpv', audioOutput: 'null', draftIsolation: true, mediaSubstitution: 'exact synthetic CDN URL to generated PCM' });
  } catch (error) {
    await emit('failed', { ok: false, stageCount, error: String(error), stack: error.stack });
  }
})();
