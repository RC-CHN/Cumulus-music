(async () => {
  const one = selector => document.querySelector(selector);
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, payload = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...payload },
  });
  const assert = (value, message) => { if (!value) throw Error(message); };
  async function wait(read, message) {
    const end = performance.now() + 20000;
    while (performance.now() < end) {
      const result = await read();
      if (result) return result;
      await new Promise(resolve => setTimeout(resolve, 60));
    }
    throw Error(message);
  }
  const action = () => one('.lyrics-panel [data-lyric-contribution]');
  const openError = () => one('.lyrics-panel .lyric-contribution-error');
  const available = label => action()?.textContent.trim() === label && action()?.offsetWidth > 0;
  const operator = () => one('.lyrics-panel [aria-label="歌词操作"]');
  const font = () => one('.lyrics-panel [aria-label="调整歌词字号"]');
  const slider = () => one('.lyrics-panel [aria-label="歌词字号增量"]');
  const fontKey = 'cumulus.playback.lyric-font-gain.v1';
  const lyricSize = () => parseFloat(getComputedStyle(one('.lyrics-panel .lyric-line')).fontSize);
  async function showFont(value) {
    const trigger = await wait(() => font()?.offsetWidth && font(), 'visible lyric font button');
    trigger.focus({ preventScroll: true });
    trigger.click();
    return wait(() => slider()?.value === String(value) && document.activeElement === slider() &&
      slider(), 'font slider opens and receives focus');
  }
  async function closeFont() {
    slider().dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await wait(() => font()?.getAttribute('aria-expanded') === 'false' &&
      document.activeElement === font(), 'Escape closes the font popup and restores focus');
  }
  const menu = () => {
    const id = operator()?.getAttribute('aria-controls');
    return id && [...document.querySelectorAll('.action-menu:not([inert])')].find(node => node.id === id);
  };
  const rows = () => [...(menu()?.querySelectorAll('[data-menu-key]') ?? [])];
  async function showMenu(expected) {
    await wait(() => {
      if (!operator()?.offsetWidth) return false;
      if (operator().getAttribute('aria-expanded') !== 'true') operator().click();
      const panel = menu();
      return panel && getComputedStyle(panel).visibility === 'visible' &&
        panel.getAnimations().every(animation => animation.playState !== 'running' && !animation.pending) &&
        JSON.stringify(rows().map(row => row.textContent.trim())) === JSON.stringify(expected);
    }, 'visible toolbar rows ' + expected.join(' / '));
  }
  async function selectMenu(key) {
    const row = await wait(() => rows().find(row => row.dataset.menuKey === key && !row.disabled),
      'enabled toolbar action ' + key);
    row.click();
    await wait(() => !menu() && one('.lyric-tools')?.getAttribute('aria-busy') === 'false',
      'toolbar selection and browser handoff finish');
    assert(!openError(), 'toolbar browser handoff succeeds');
  }
  let epoch, count = 0;
  const playback = () => invoke('player_snapshot', { epoch });
  async function select(song) {
    await invoke('play_selection', {
      epoch, selection: { kind: 'songs', ids: [song], index: 0, quality: 'standard' },
    });
    const current = await wait(async () => {
      const state = await playback();
      return state.state === 'playing' && state.track?.id === song && state;
    }, 'actual native playback ' + song);
    await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    await wait(async () => (await playback()).state === 'paused', 'paused native playback');
    if (!one('.lyrics-panel')) one('.playerbar [aria-label="歌词"]').click();
    return current.playId;
  }
  async function capture(name, song) {
    await wait(() => !one('.lyrics-panel-enter-active,.lyrics-panel-leave-active'), 'settled lyrics');
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const current = await playback();
    assert(current.state === 'paused' && current.track?.id === song, 'same paused native song');
    const stage = 'ux_' + String(++count).padStart(2, '0') + '_' + name;
    await emit(stage, { captureHandshake: true, song, playId: current.playId });
  }
  async function rejected(playId) {
    try { await invoke('lyric_contribution_open', { epoch, playId }); }
    catch { return; }
    throw Error('unavailable contribution opened');
  }
  async function rejectToolbar(playId, action, activeEpoch = epoch) {
    try { await invoke('lyric_toolbar_contribution_open', { epoch: activeEpoch, playId, action }); }
    catch { return; }
    throw Error('unavailable toolbar action opened: ' + action);
  }
  try {
    const account = await wait(async () => {
      const value = await invoke('account_snapshot');
      return value.status === 'authenticated' && one('.playerbar') && value;
    }, 'sealed signed-in account');
    epoch = account.epoch;
    const first = await select('1');
    await wait(() => available('上传歌词'), 'ordinary empty lyrics upload action');
    await capture('empty_upload', '1');
    const emptyBeforeFont = await playback();
    assert(!one('.lyrics-panel .lyric-line'), 'empty lyrics have no parsed lines');
    await showFont(0);
    one('.lyrics-panel [aria-label="增大歌词字号"]').click();
    await wait(() => slider()?.value === '1', 'font control moves by one step');
    one('.lyrics-panel [aria-label="增大歌词字号"]').click();
    await wait(() => slider()?.value === '2' && localStorage.getItem(fontKey) === '2',
      'empty-track adjustment is stored for subsequent tracks');
    await capture('empty_lyric_font', '1');
    await closeFont();
    const emptyAfterFont = await playback();
    assert(emptyBeforeFont.playId === emptyAfterFont.playId &&
      emptyBeforeFont.queueRevision === emptyAfterFont.queueRevision &&
      emptyBeforeFont.position === emptyAfterFont.position, 'font adjustment leaves native playback alone');
    action().click();
    await wait(() => openError() && action()?.getAttribute('aria-disabled') === 'false',
      'first shell failure leaves the original retry action');
    await capture('open_failed', '1');
    action().click();
    await wait(() => !openError() && action()?.getAttribute('aria-disabled') === 'false',
      'browser retry succeeds');
    assert((await playback()).playId === first, 'opening browser does not reselect playback');
    const second = await select('2');
    await rejected(first);
    await wait(() => available('上传滚动歌词或翻译'), 'missing scrolling and translation');
    action().click();
    await wait(() => action()?.getAttribute('aria-disabled') === 'false' && !openError(),
      'timed and translation handoff accepted');
    await capture('scroll_and_translation', '2');
    const third = await select('3');
    await rejected(second);
    await wait(() => available('上传翻译'), 'missing translation only');
    await capture('translation_only', '3');
    await wait(() => one('.lyrics-panel .lyric-line'), 'timed lyrics are visible');
    await showFont(2);
    const increased = lyricSize();
    await capture('timed_lyric_font_carried', '3');
    one('.lyrics-panel [aria-label="减小歌词字号"]').click();
    await wait(() => slider()?.value === '1', 'font control moves by one step');
    one('.lyrics-panel [aria-label="减小歌词字号"]').click();
    await wait(() => slider()?.value === '0' && localStorage.getItem(fontKey) === '0' &&
      increased - lyricSize() === 2, 'the carried setting changes the timed line by two CSS pixels');
    const fontPixelDelta = increased - lyricSize();
    await closeFont();
    const pure = await select('4');
    await rejected(third);
    await wait(() => !action() && one('.lyrics-panel .empty-state')?.textContent.includes('纯音乐，请欣赏'),
      'instrumental has no contribution entry');
    assert(font()?.offsetWidth, 'instrumental lyrics still offer a font control');
    await rejected(pure);
    await capture('instrumental', '4');
    const failed = await select('5');
    await wait(() => !action() && one('.lyrics-panel .empty-state')?.textContent.includes('歌词加载失败')
      && one('.lyrics-panel .empty-state button')?.textContent.trim() === '刷新',
      'lyric fetch failure has retry only');
    assert(font()?.offsetWidth, 'lyric failure still offers a font control');
    await rejected(failed);
    await capture('lyric_failed', '5');
    await showMenu(['歌词报错']);
    await rejectToolbar(failed, 'upload');
    const noContent = await select('1');
    await wait(() => available('上传歌词'), 'body can show an upload for a missing raw document');
    await showMenu(['歌词报错']);
    await rejectToolbar(noContent, 'upload');
    await capture('body_only_no_raw_document', '1');

    const both = await select('6');
    assert(!menu(), 'old menu retires on a new play id');
    await showMenu(['翻译歌词', '上传歌词', '歌词报错']);
    assert(!one('.lyrics-panel .lyric-line') && !action(), 'toolbar options do not require parsed lines or a body prompt');
    await capture('toolbar_both_no_lines', '6');
    await rejectToolbar(both, 'forged');
    await rejectToolbar(both, 'upload', '0');
    await selectMenu('translation');
    await showMenu(['翻译歌词', '上传歌词', '歌词报错']);
    await selectMenu('upload');
    assert((await playback()).playId === both, 'both handoffs preserve the playback identity');

    const upload = await select('7');
    await rejectToolbar(both, 'upload');
    await showMenu(['上传歌词', '歌词报错']);
    await capture('toolbar_upload_only', '7');
    await rejectToolbar(upload, 'translation');
    await selectMenu('upload');
    await showMenu(['上传歌词', '歌词报错']);
    await selectMenu('report');
    const report = await wait(() => one('.lyric-report-dialog[open]'), 'report keeps its own action');
    assert(report.querySelector('h2')?.textContent.trim() === '歌词报错', 'native report dialog');
    await wait(() => !report.classList.contains('modal-enter-active') &&
      getComputedStyle(report).opacity === '1', 'native report dialog has painted');
    await capture('toolbar_report_preserved', '7');
    const close = await wait(() => report.querySelector('[aria-label="关闭歌词报错"]:not(:disabled)'), 'report can close');
    close.click();
    await wait(() => !one('.lyric-report-dialog[open]'), 'close report without submission');

    const translation = await select('8');
    await showMenu(['翻译歌词', '歌词报错']);
    await capture('toolbar_translation_only', '8');
    await rejectToolbar(translation, 'upload');
    await selectMenu('translation');
    const wikiTab = one('.lyrics-panel button[aria-controls="vinyl-wiki-content"]');
    const lyricsTab = one('.lyrics-panel button[aria-controls="vinyl-lyrics-content"]');
    assert(wikiTab && lyricsTab, 'classic playback tabs are available');
    wikiTab.click();
    await wait(() => wikiTab.getAttribute('aria-selected') === 'true' && !operator()?.offsetWidth && !font()?.offsetWidth,
      'wiki hides the lyrics operator');
    lyricsTab.click();
    await wait(() => lyricsTab.getAttribute('aria-selected') === 'true' && operator()?.offsetWidth && font()?.offsetWidth,
      'operator returns on the lyrics tab');
    assert((await playback()).playId === translation, 'switching tabs preserves current native playback');
    await emit('complete', { ok: true, stageCount: count, staleRejected: true, toolbarVerified: true,
      wikiHidesToolbar: true, emptyLyricFont: true, fontPixelDelta });
  } catch (error) {
    await emit('failed', { error: String(error), stack: error.stack,
      panel: one('.lyrics-panel')?.innerText.slice(0, 400), stageCount: count });
  }
})();
