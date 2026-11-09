// Exercise the actual Search menu, native receipt and Linux shell handoff.
(async () => {
  const query = '合成';
  const one = (selector) => document.querySelector(selector);
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, extra = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...extra } });
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const settled = () =>
    !one('.page-enter-active,.page-leave-active,.search-results-enter-active,.search-results-leave-active');
  const assert = (value, label) => { if (!value) throw Error(label); };
  async function wait(read, label) {
    for (let n = 0; n < 175; n++) {
      const value = await read();
      if (value) return value;
      await sleep(40);
    }
    throw Error(label);
  }
  async function control(action) {
    window.__searchVoiceMenuAck = undefined;
    await emit('search-voice-menu-fixture', { action });
    return wait(() => window.__searchVoiceMenuAck, 'native menu fixture acknowledgement');
  }
  const trigger = (id) => one(`[data-result-key="voice:${id}"] .menu-trigger`);
  const menu = (button) => {
    const id = button?.getAttribute('aria-controls');
    const panel = id && document.getElementById(id);
    return panel && !panel.closest('[inert]') && button.getAttribute('aria-expanded') === 'true' && panel;
  };
  const item = (button, key) => menu(button)?.querySelector(`[data-menu-key="${key}"]`);
  async function open(button) {
    button.scrollIntoView({ block: 'center', behavior: 'instant' });
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    button.focus({ preventScroll: true });
    button.click();
  }
  function escape(button) {
    document.activeElement.dispatchEvent(new KeyboardEvent('keydown', {
      key: 'Escape', bubbles: true, cancelable: true,
    }));
    return wait(() => !menu(button) && document.activeElement === button, 'focus returned to this row');
  }
  function pointer(button) {
    const rect = button.getBoundingClientRect();
    const x = rect.left + rect.width / 2, y = rect.top + rect.height / 2;
    assert(button.contains(document.elementFromPoint(x, y)), 'trusted pointer hits the menu action');
    return { x: Math.round((screenX + x) * devicePixelRatio),
      y: Math.round((screenY + y) * devicePixelRatio) };
  }
  let count = 0, step = 'startup';
  const capture = async (label, extra = {}, panel, anchor) => {
    const stage = `ux_${String(++count).padStart(2, '0')}_${label}`;
    const paintSamples = [];
    if (panel) {
      await wait(() => !panel.classList.contains('menu-enter-active') &&
        getComputedStyle(panel).opacity === '1', 'menu transition settled');
      const rect = panel.getBoundingClientRect();
      paintSamples.push({ x: rect.left + 3, y: rect.top + rect.height / 2,
        color: getComputedStyle(panel).backgroundColor });
    }
    if (anchor) {
      // Use the source cover, outside the popup shadow. A solid menu alone
      // does not prove that the scrolled search row has painted underneath.
      const cover = anchor.closest('[data-result-key]')?.querySelector('.search-result-art');
      assert(cover, 'original voice row cover is present');
      const rect = cover.getBoundingClientRect();
      paintSamples.push({ x: rect.left + 8, y: rect.top + rect.height / 2,
        color: getComputedStyle(cover).backgroundColor });
    }
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await emit(stage, { ...extra, screenX, screenY, ratio: devicePixelRatio, paintSamples });
  };
  let trustedFolderClicks = 0;
  document.addEventListener('click', (event) => {
    if (event.isTrusted && event.target instanceof Element &&
      event.target.closest('[data-menu-key="voice-folder"]')) trustedFolderClicks++;
  }, true);
  try {
    await wait(() => settled() && one('.track-row') && one('[aria-label="搜索音乐"]'), 'Daily and search header');
    const { epoch, status } = await invoke('account_snapshot');
    assert(status === 'authenticated', 'isolated synthetic account');
    const before = await invoke('player_snapshot', { epoch });
    window.__searchFixtureAck = false;
    await emit('search-fixture', { mode: 'voice_next' });
    await wait(() => window.__searchFixtureAck, 'search fixture selected');
    const input = one('[aria-label="搜索音乐"]');
    input.value = query;
    input.dispatchEvent(new Event('input', { bubbles: true }));
    input.form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    step = 'comprehensive-complete';
    const completed = await wait(() => settled() && trigger('401'), 'ordinary comprehensive voice card');
    // The search page restores its navigation scroll a few frames after the
    // result nodes arrive; start this action from a settled user-visible row.
    await sleep(200);
    await open(completed);
    const folder = await wait(() => item(completed, 'voice-folder'), 'completed voice folder');
    assert(item(completed, 'comments')?.textContent.trim() === '查看评论(54)' &&
      !item(completed, 'download'), 'signed count and native completion gate');
    await wait(() => {
      const bounds = folder.getBoundingClientRect();
      return folder.contains(document.elementFromPoint(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2));
    }, 'folder hit test ready');
    await capture('completed_menu', { pointer: pointer(folder), count: 54 }, menu(completed), completed);
    await wait(() => !menu(completed) && document.activeElement === completed, 'folder returns focus to its trigger');
    assert(trustedFolderClicks === 1, 'real pointer activated the folder action');
    await capture('folder_closed', { triggerFocused: true });
    step = 'classified-fallback';
    one('#search-tab-voices').click();
    const fallback = await wait(() => settled() &&
      one('#search-tab-voices')?.getAttribute('aria-selected') === 'true' && trigger('402'),
    'classified voice row 402');
    await open(fallback);
    await wait(() => item(fallback, 'comments'), 'plain fallback after native count fails');
    assert(item(fallback, 'comments').textContent.trim() === '查看评论' &&
      item(fallback, 'download')?.textContent.trim() === '下载' &&
      !item(fallback, 'voice-folder'), 'failed count and no completed download remain independent');
    await capture('classified_fallback', { plain: true }, menu(fallback));
    await escape(fallback);
    step = 'late-prepare';
    await control('hold');
    const old = trigger('401');
    await open(old);
    await wait(async () => (await control('check')).held, 'native old comment request held');
    assert(!menu(old), 'old menu remains closed until its count read returns');
    await open(fallback);
    const live = await wait(() => item(fallback, 'comments'), 'new row can open while old read waits');
    const focus = document.activeElement;
    await control('release');
    await wait(async () => !(await control('check')).held, 'old reply released');
    await sleep(150);
    assert(item(fallback, 'comments') === live && !menu(old) &&
      document.activeElement === focus, 'old reply cannot replace the new menu or steal focus');
    await capture('late_ignored', { freshRow: '402', focusKept: true }, menu(fallback));
    await escape(fallback);
    const after = await invoke('player_snapshot', { epoch });
    assert(after.state === before.state && after.playId === before.playId &&
      after.queueRevision === before.queueRevision, 'menu does not edit playback');
    await emit('complete', { ok: true, stageCount: count, trustedFolderClicks });
  } catch (error) {
    await emit('failed', { step, count, message: String(error) });
  }
})();
