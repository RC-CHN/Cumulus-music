// Real production WebView, sealed metadata fixture and native libmpv/null.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, fields = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...fields },
  });
  const assert = (test, label) => { if (!test) throw new Error(label); };
  async function wait(test, label) {
    for (let i = 0; i < 175; i++) {
      const value = await test();
      if (value) return value;
      await sleep(60);
    }
    throw new Error(label);
  }
  const click = (node) => {
    assert(node && !node.disabled, 'expected enabled control');
    node.focus({ preventScroll: true });
    node.click();
  };
  const panel = () => one('.lyrics-panel');
  const mode = () => panel()?.dataset.vinylMode;
  const dialog = () => one('.vinyl-mode-sheet[open]');
  const entry = () => one('.vinyl-mode-entry');
  const footer = () => one('.playerbar [aria-label="歌词"]');
  const close = () => one('[aria-label="收起播放详情"]');
  const radio = (mode) => dialog()?.querySelector(`input[value="${mode}"]`);
  const coverLine = () => one('.vinyl-cover-line')?.textContent;
  const slideClasses = '.modal-enter-active,.modal-leave-active,.lyrics-panel-enter-active,.lyrics-panel-leave-active';
  let stageCount = 0;

  async function capture(label, data = {}) {
    await wait(() => !panel()?.matches(slideClasses) && !dialog()?.matches(slideClasses),
      'settled playback layout');
    await sleep(120);
    const stage = `ux_${String(++stageCount).padStart(2, '0')}_${label}`;
    await emit(stage, { mode: mode(), captureHandshake: true, ...data });
  }
  async function showSheet(expected) {
    click(entry());
    await wait(() => radio(expected)?.checked && document.activeElement === radio(expected),
      'active choice checked and focused on mode entry');
    assert(all('.vinyl-mode-choice').map((node) => node.textContent.trim()).join('/') ===
      '标准/黑胶/歌词', 'official PC label order');
    const rect = dialog().getBoundingClientRect();
    assert(rect.width === Math.min(386, innerWidth) && rect.height > 350,
      'mode picker is the recovered 386px right side sheet');
  }
  async function hideSheet() {
    await wait(() => !dialog() && document.activeElement === entry() && panel(),
      'closing the sheet returns focus to its trigger and keeps details open');
  }
  const select = (key) => click(radio(key).closest('label'));
  const centerVerified = () => {
    const line = one('.lyric-line.current');
    return mode() === 'lyric' && line && getComputedStyle(line).textAlign === 'center' &&
      !one('.vinyl-record') && !one('.lyric-tools');
  };

  try {
    const reopened = window.__vinylReopened;
    const { epoch } = await wait(async () => {
      const account = await invoke('account_snapshot');
      return account.status === 'authenticated' && account;
    }, 'synthetic authenticated profile ready');
    const player = () => invoke('player_snapshot', { epoch });
    const pause = async () => {
      click(one('.playerbar [aria-label="暂停"]'));
      return wait(async () => {
        const value = await player();
        return value.state === 'paused' && value;
      }, 'native pause');
    };
    const samePaused = async (baseline) => {
      const now = await player();
      assert(now.playId === baseline.playId && now.queueRevision === baseline.queueRevision &&
        now.state === 'paused' && Math.abs(now.position - baseline.position) < .1 &&
        now.zoneFmRuleId === baseline.zoneFmRuleId, 'layout preserves paused transport');
      return { playId: now.playId, queueRevision: now.queueRevision, position: now.position };
    };
    const openFooter = async (expected) => {
      click(footer());
      await wait(() => mode() === expected && document.activeElement === close(),
        'footer opens expected display preference');
    };
    const closeDetails = async (opener) => {
      click(close());
      await wait(() => !panel() && document.activeElement === opener(), 'focus returns to opener');
    };
    const beginOrdinary = async () => {
      const daily = () => all('.sidebar button:not(:disabled)').find((node) => node.textContent.trim() === '每日推荐');
      await wait(daily, 'enabled daily navigation');
      click(daily());
      await wait(() => one('.daily-track-actions') && one('.track-row') &&
        one('[aria-label="列表循环"]:not(:disabled)'), 'ordinary recommendations and player ready');
      one('.track-row').dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
      await wait(async () => {
        const now = await player();
        return now.state === 'playing' && !now.fm && now.track?.id === '1' && now.position > .16;
      }, 'synthetic ordinary libmpv output');
      return pause();
    };

    await emit('resize', { width: 1000, height: 680 });
    await wait(() => innerWidth === 1000 && innerHeight === 680, 'bounded viewport');
    if (!reopened) {
      const ordinary = await beginOrdinary();
      await openFooter('classic');
      await wait(() => one('.lyric-line.current') && one('.lyric-tools'), 'standard retains lyric tools');
      await showSheet('classic');
      await capture('standard', await samePaused(ordinary));
      if (!window.__vinylNativeKeys) select('cover');
      await wait(() => mode() === 'cover' && radio('cover')?.checked && dialog(),
        'choosing black vinyl applies immediately without closing the side sheet');
      await capture('cover_sheet', await samePaused(ordinary));
      if (!window.__vinylNativeKeys) dialog().dispatchEvent(new Event('cancel', { cancelable: true }));
      await hideSheet();
      await wait(() => coverLine()?.includes('合成歌词 0'), 'cover starts with a primary lyric line');
      assert(one('.vinyl-record') && !one('.lyric-scroll') && !one('.lyric-tools'),
        'cover only renders its record, metadata and current line');
      click(one('.playerbar [aria-label="播放"]'));
      await wait(() => coverLine()?.includes('合成歌词 1'), 'cover line follows actual native playback progress');
      const advanced = await pause();
      assert(advanced.playId === ordinary.playId && advanced.position > ordinary.position + .5,
        'same ordinary play ID advanced under cover layout');
      await capture('black_vinyl', await samePaused(advanced));

      await showSheet('cover');
      select('lyric');
      click(one('[aria-label="关闭播放器模式"]'));
      await hideSheet();
      await wait(centerVerified, 'centered lyric-only layout');
      await capture('centered_lyric', await samePaused(advanced));
      await closeDetails(footer);

      await emit('complete', { ok: true, stageCount, reopened });
      return;
    }

    const curated = () => all('.sidebar button:not(:disabled)').find((node) => node.textContent.trim() === '精选');
    await wait(curated, 'enabled curated entry');
    click(curated());
    await wait(() => one('[data-choice-zone="ROCK"]'), 'Rock zone available');
    click(one('[data-choice-zone="ROCK"]'));
    const fmCover = () => one('.vertical-zone button[aria-label="合成漫游首曲"]');
    await wait(fmCover, 'mounted zone FM card');
    click(fmCover());
    await wait(async () => {
      const now = await player();
      return now.state === 'playing' && now.fm && now.zoneFmRuleId === '11' &&
        now.track?.id === '81' && now.position > .2 && centerVerified();
    }, 'after process restart FM inherits the persisted centered mode and starts native playback');
    const fm = await pause();
    await capture('fm_centered', await samePaused(fm));

    await showSheet('lyric');
    select('cover');
    click(one('[aria-label="关闭播放器模式"]'));
    await hideSheet();
    await wait(() => mode() === 'cover' && coverLine()?.includes('合成歌词'), 'FM cover displays its current lyric');
    await capture('fm_cover', await samePaused(fm));
    await closeDetails(fmCover);
    await openFooter('cover');
    await wait(() => coverLine()?.includes('合成歌词'), 'same persisted mode after closing the panel');
    await capture('fm_reopen', await samePaused(fm));
    await closeDetails(footer);
    await emit('complete', { ok: true, stageCount, reopened });
  } catch (error) {
    await emit('failed', {
      error: error instanceof Error ? String(error) : JSON.stringify(error),
      stack: error?.stack, stageCount, mode: mode() ?? null,
      focus: document.activeElement?.outerHTML.slice(0, 350),
      line: coverLine() ?? null,
    });
  }
})();
