// DOM actions in the production Tauri WebView, with a private fixture account.
(async () => {
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const all = selector => [...document.querySelectorAll(selector)];
  const one = selector => document.querySelector(selector);
  const assert = (ok, message) => { if (!ok) throw Error(message); };
  const emit = (stage, rest = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...rest } });
  const wait = async (test, label) => { for (let i = 0; i < 150; i++) { if (test()) return; await sleep(100); } throw Error(label); };
  const button = text => all('button').find(node => node.getAttribute('aria-label') === text || node.textContent.trim() === text);
  const click = node => { assert(node && !node.disabled, `unavailable control: ${node?.outerHTML}`); node.focus({ preventScroll: true }); node.click(); };
  const key = (node, value) => node.dispatchEvent(new KeyboardEvent('keydown', { key: value, bubbles: true, cancelable: true }));
  const rect = node => { const r = node.getBoundingClientRect(); return { x: r.x, y: r.y, right: r.right, bottom: r.bottom, width: r.width, height: r.height }; };
  const visible = node => { const r = rect(node); return r.x >= 0 && r.y >= 0 && r.right <= innerWidth + 1 && r.bottom <= innerHeight + 1 && r.width > 0 && r.height > 0; };
  const hittable = node => { const r = rect(node); return visible(node) && document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2)?.closest('button') === node; };
  const editorActions = () => all('.playlist-dialog footer button, .playlist-editor-actions > button, .playlist-editor-actions .menu-trigger').map(node => ({ label: node.textContent.trim(), ...rect(node) }));
  function editorActionsVisible() {
    const page=one('.playlist-editor-page');
    const dialog = rect(page||one('.playlist-dialog[open]'));
    if(page) { assert(all('.playlist-editor-actions > button, .playlist-editor-actions .menu-trigger').every(hittable),'editor footer controls must be visible and hittable');return; }
    assert(editorActions().every(action => action.x >= dialog.x + 12 && action.right <= dialog.right - 12 && action.y >= dialog.y && action.bottom <= dialog.bottom - 12), 'editor actions clipped inside dialog or missing bottom inset');
  }
  const equalizerBands = () => all('.equalizer-band').map(node => ({ value: rect(node.querySelector('output')), slider: rect(node.querySelector('input')), label: rect(node.lastElementChild) }));
  const trackActions = () => all('.track-more').map(node => {
    const range = document.createRange(); range.selectNodeContents(node);
    const glyph = range.getBoundingClientRect();
    return { button: rect(node), row: rect(node.closest('.track-row')),
      duration: rect(node.parentElement.querySelector('.track-duration')),
      glyph: { x: glyph.x, right: glyph.right } };
  });
  const facts = () => ({ width: innerWidth, height: innerHeight, theme: document.documentElement.dataset.theme,
    overflow: document.documentElement.scrollWidth > innerWidth,
    background: getComputedStyle(document.documentElement).backgroundColor, trackActions: trackActions(),
    focused: document.activeElement?.getAttribute('aria-label'), editorActions: editorActions(), equalizerBands: equalizerBands(),
    dialogs: all('dialog[open]').map(node => ({ title: node.querySelector('h2')?.textContent, ...rect(node) })) });
  let stageCount = 0;
  async function capture(label) {
    await sleep(500);
    assert(!facts().overflow, `horizontal document overflow: ${label}`);
    assert(all('dialog[open], .action-menu, .window-controls').every(visible), `clipped dialog/menu/window controls: ${label}`);
    assert(trackActions().every(({ button, row, duration, glyph }) => glyph.x >= button.x - 1 && glyph.right <= button.right + 1 && button.right <= row.right + 1 && duration.right <= button.x), `track action glyph clipped or overlaps duration: ${label}`);
    assert(equalizerBands().every(({ value, slider, label }) => Math.abs((value.x + value.right - slider.x - slider.right) / 2) < 1 && value.bottom < slider.y && slider.bottom < label.y), `equalizer labels overlap or lose column alignment: ${label}`);
    await emit(`ux_${String(++stageCount).padStart(2, '0')}_${label}`, facts());
    await sleep(1300);
  }
  async function theme(value) {
    click(one('[aria-label="外观主题"]'));
    await wait(() => one('.action-menu'), 'appearance menu');
    click(button({ light: '浅色', dark: '深色', system: '跟随系统' }[value]));
    const expected = value === 'system' ? (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light') : value;
    await wait(() => document.documentElement.dataset.theme === expected && !one('.action-menu'), `theme ${value}`);
    assert(localStorage.getItem('cumulus.appearance.v1') === value, 'theme was not saved');
    assert(getComputedStyle(document.documentElement).colorScheme === expected, 'native input color scheme');
  }
  async function closeEditor() {
    click(one('[aria-label="取消编辑歌单"]'));
    await sleep(120);
    if(one('.playlist-editor-confirm')) click(button('放弃修改并离开'));
    await wait(() => !one('.playlist-editor-page')&&!one('dialog[open]'), 'editor close');
  }
  async function edit() {
    click(one('[aria-label="编辑歌单"]'));
    await wait(() => one('.playlist-editor-page input[aria-label="歌单名称"]')?.value, 'editor data');
    await sleep(250);
    assert(document.activeElement === one('[aria-label="歌单名称"]'), 'editor must focus the name');
  }
  async function page(label, selector) {
    click(button(label));
    await wait(() => one(selector), `page ${label}`);
    await sleep(500);
  }
  async function playlistConfirmation(label, suffix) {
    click(button('其他歌单操作')); await wait(()=>one('.action-menu'),'editor management menu');
    click(button(label));
    await wait(() => button('返回编辑'), 'playlist confirmation');
    await capture(`playlist_confirmation_${suffix}`);
    assert(document.activeElement === button('返回编辑'), 'playlist confirmation must focus return to editing');
    assert(one('.playlist-editor-confirm h2').textContent !== '编辑歌单信息', 'playlist dialog must announce its current confirmation');
    if (suffix === 'small_dark') one('.playlist-dialog').dispatchEvent(new Event('cancel', { cancelable: true }));
    else click(button('返回编辑'));
    await wait(() => !one('.playlist-editor-confirm') && one('[aria-label="歌单名称"]'), 'return to playlist editor after confirmation leaves');
    assert(document.activeElement === button('其他歌单操作'), 'return to editor must restore the menu trigger');
  }
  try {
    await wait(() => all('.track-row').length, 'daily');
    await sleep(700);
    assert(one('.track-duration')?.textContent === '100:00', 'long-duration fixture not loaded');
    assert(all('.track-title .ellipsis').every(node => node.dataset.tooltip && !node.hasAttribute('title')), 'truncated track credits need app-owned full text hints');
    if (localStorage.getItem('cumulus.ux.finished')) {
      assert(localStorage.getItem('cumulus.appearance.v1') === 'dark' && facts().theme === 'dark', 'dark preference did not survive a process restart');
      await capture('persisted_dark');
      await theme('system'); await capture('system');
      await emit('complete', { ok: true, reopened: true, stageCount, realMusicRequests: 0 }); return;
    }
    assert(facts().theme === (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'), 'initial system theme');
    await theme('light'); await capture('daily_light');
    await page('我的歌单', '.playlist-card'); await capture('library_light');
    assert(!one('.playlist-card-controls'), 'old card action clutter remains');
    const firstMenu = one('[data-library-open="2"]').closest('article').querySelector('.menu-trigger'); click(firstMenu);
    await wait(() => one('.action-menu'), 'playlist menu');
    assert(button('上移歌单').disabled, 'first owned playlist cannot move up');
    key(document.activeElement, 'End'); assert(document.activeElement === button('下移歌单'), 'menu End navigation');
    key(document.activeElement, 'ArrowUp'); assert(document.activeElement === button('删除歌单'), 'menu skips disabled items and separator');
    key(document.activeElement, 'ArrowUp'); assert(document.activeElement === button('编辑歌单信息'), 'previous editable action');
    await capture('card_menu');
    click(button('编辑歌单信息')); await wait(() => one('[aria-label="歌单名称"]')?.value, 'card editor');
    await closeEditor(); assert(document.activeElement === firstMenu, 'card menu modal focus restoration');
    click(all('.playlist-card')[1]); await wait(() => one('.playlist-title'), 'playlist');
    const heading = rect(one('.playlist-title h1')), editor = rect(one('.playlist-edit'));
    assert(editor.x >= heading.right - 1 && Math.abs(editor.y - heading.y) < 8, 'edit action must sit next to title');
    assert(one('.playlist-title h1').title === one('.playlist-title h1').textContent, 'truncated playlist heading needs a full title hint');
    await capture('playlist_light');
    await edit();
    assert(one('[aria-label="歌单名称"]').getAttribute('aria-invalid') === 'true' && button('保存修改').disabled, 'overlong fixture name must be explained and rejected');
    await capture('invalid_editor');
    const name = one('[aria-label="歌单名称"]'); name.value = '午后散步'; name.dispatchEvent(new Event('input', { bubbles: true }));
    await wait(() => !button('保存修改').disabled, 'valid edited name');
    await capture('editor_light');
    click(button('选择标签')); await wait(() => one('.tag-picker section'), 'playlist tag choices');
    await capture('editor_tags_light'); click(button('选择标签'));
    for (const [label, suffix] of [['清空歌曲', 'clear'], ['设为公开', 'publish'], ['删除歌单', 'delete']]) {
      await playlistConfirmation(label, suffix);
      assert(one('[aria-label="歌单名称"]').value === '午后散步', 'return from confirmation must preserve unsaved edits');
    }
    await capture('editor_returned');
    window.__coverFixtureAck=null;await emit('cover-fixture',{kind:'landscape',playlistId:'2'});await wait(()=>window.__coverFixtureAck,'cover fixture');assert(window.__coverFixtureAck.ok,'cover fixture prepared');
    click(one('[aria-label="替换歌单封面"]'));
    await wait(() => one('.cover-dialog[open]'), 'nested cover editor'); await capture('cover_dialog');
    click(one('[aria-label="关闭封面编辑"]')); await wait(() => !one('.cover-dialog'), 'close nested cover');
    assert(document.activeElement === one('[aria-label="替换歌单封面"]'), 'nested dialog focus restoration');
    await closeEditor();
    assert(document.activeElement === one('.playlist-edit'), 'title edit focus restoration');
    await emit('resize', { width: 760, height: 540 }); await wait(() => innerWidth === 760, 'small window');
    await edit(); await capture('small_editor_light');
    assert(visible(button('保存修改')), 'small editor save is offscreen');
    editorActionsVisible();
    const smallName = one('[aria-label="歌单名称"]'); smallName.value = '午后散步'; smallName.dispatchEvent(new Event('input', { bubbles: true }));
    await wait(() => !button('保存修改').disabled, 'valid small editor');
    await capture('small_editor_valid_light'); editorActionsVisible();
    await closeEditor(); await theme('dark'); await capture('small_playlist_dark');
    click(one('[aria-label="外观主题"]')); await wait(() => one('.action-menu'), 'small theme menu');
    await capture('small_theme_menu'); key(document.activeElement, 'Escape');
    await wait(() => !one('.action-menu'), 'Escape closes menu');
    assert(document.activeElement === one('[aria-label="外观主题"]'), 'theme menu Escape focus');
    await edit(); await capture('small_editor_dark'); editorActionsVisible(); await closeEditor();
    await edit(); await playlistConfirmation('删除歌单', 'small_dark'); await closeEditor();
    await page('我的歌单', '.playlist-card');
    const grid = one('.playlist-grid'); grid.scrollTop = grid.scrollHeight; await sleep(250);
    all('.playlist-shell').at(-1).dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2 }));
    await wait(() => one('.action-menu'), 'bottom card menu');
    assert(button('下移歌单').disabled, 'last collected playlist cannot move down');
    assert(!button('编辑歌单信息'), 'collected playlist cannot be edited');
    await capture('small_menu_edge');
    document.body.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }));
    await wait(() => !one('.action-menu'), 'outside pointer closes menu');
    await emit('resize', { width: 1120, height: 760 }); await wait(() => innerWidth === 1120, 'restore window');
    await page('每日推荐', '.track-row'); await capture('daily_dark');
    const search = one('[aria-label="搜索音乐"]'); search.value = '合成'; search.dispatchEvent(new Event('input', { bubbles: true }));
    search.form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await wait(() => one('.search-heading-copy h1')?.textContent === '合成' && one('.search-track-card'), 'search'); await capture('search_dark');
    await page('下载管理', '.downloads-page'); await capture('downloads_empty');
    await page('本地音乐', '.local-page'); await capture('local_empty');
    await emit('resize', { width: 760, height: 540 }); await wait(() => innerWidth === 760, 'small local');
    one('.local-page').scrollTop = one('.local-page').scrollHeight;
    await capture('small_local_empty');
    assert(hittable(button('选择音乐目录')), 'local library bottom action cannot be reached by scrolling');
    await emit('resize', { width: 1120, height: 760 }); await wait(() => innerWidth === 1120, 'restore after local');
    await page('音乐云盘', '.cloud-page'); await capture('cloud_error');
    assert(one('.cloud-page').textContent.includes('暂时无法读取音乐云盘') && !one('.cloud-page').textContent.includes('把喜欢的音乐带在身边'), 'failed cloud load presented as empty library');
    await emit('resize', { width: 760, height: 540 }); await wait(() => innerWidth === 760, 'small cloud');
    await capture('small_cloud_error');
    assert(all('.cloud-toolbar > button, .cloud-toolbar form').every(visible), 'small cloud toolbar is clipped');
    one('.cloud-page').scrollTop = one('.cloud-page').scrollHeight; await sleep(100);
    assert(hittable(button('重新加载')), 'cloud retry cannot be reached by scrolling');
    one('.cloud-page').scrollTop = 0;
    click(button('上传音乐')); await wait(() => one('.cloud-upload-dialog[open]'), 'cloud upload dialog');
    await capture('small_cloud_upload'); editorActionsVisible();
    const sourceButtons = all('.cloud-source-actions button').map(rect);
    assert(sourceButtons.length === 2 && Math.abs(sourceButtons[0].y - sourceButtons[1].y) < 1 && sourceButtons[1].x - sourceButtons[0].right >= 8, 'upload source controls overlap or are misaligned');
    click(one('[aria-label="关闭上传窗口"]')); await wait(() => !one('.cloud-upload-dialog'), 'close cloud upload dialog');
    await emit('resize', { width: 1120, height: 760 }); await wait(() => innerWidth === 1120, 'restore after cloud');
    await page('会员中心', '.member-page'); await capture('member_error');
    await page('排行榜', '.rankings-page'); await capture('rankings_error');
    await page('私人漫游', '.fm-page'); await capture('fm_error');
    await page('随心听 FM', '.scene-fm-page'); await capture('scene_error');
    await page('我的播客', '.podcast-page'); await capture('podcast_error');
    await page('收藏的 MV', '.mv-collection'); await capture('mv_error');
    await page('一起听', '.together-page'); await capture('together_idle');
    click(one('[aria-label="播放队列"]')); await wait(() => one('.queue-panel'), 'empty queue'); await capture('queue_empty');
    click(one('[aria-label="关闭播放队列"]')); await wait(() => !one('.queue-panel'), 'close queue');
    await theme('light');
    click(one('[aria-label="均衡器"]')); await wait(() => one('.equalizer-dialog'), 'light equalizer'); await capture('equalizer_light');
    click(one('[aria-label="关闭均衡器"]')); await wait(() => !one('.equalizer-dialog'), 'close light equalizer');
    await theme('dark');
    click(one('[aria-label="均衡器"]')); await wait(() => one('.equalizer-dialog'), 'equalizer'); await capture('equalizer_dark');
    await emit('resize', { width: 760, height: 540 }); await wait(() => innerWidth === 760, 'small equalizer');
    await capture('equalizer_small');
    assert(visible(button('恢复平直')), 'small equalizer footer clipped');
    click(one('[aria-label="关闭均衡器"]')); await wait(() => !one('.equalizer-dialog'), 'close equalizer');
    assert(document.activeElement === one('[aria-label="均衡器"]'), 'equalizer focus restoration');
    click(one('[aria-label="均衡器"]')); await wait(() => one('.equalizer-dialog'), 'reopen small equalizer'); await capture('equalizer_small_reopened');
    click(one('[aria-label="关闭均衡器"]')); await wait(() => !one('.equalizer-dialog'), 'close reopened equalizer');
    await theme('system'); await theme('dark');
    localStorage.setItem('cumulus.ux.finished', '1');
    await emit('complete', { ok: true, stageCount, realMusicRequests: 0, menuKeyboard: true, modalFocusRestored: true, smallEditorSaveVisible: true, smallEditorActionsVisible: true, equalizerColumnsAligned: true, explicitThemeSaved: true, trackActionsContained: true, longDuration: true, confirmationFocus: true, unsavedEditsPreserved: true });
  } catch (error) { await emit('failed', { ok: false, error: String(error), stageCount, ...facts() }); }
})();
