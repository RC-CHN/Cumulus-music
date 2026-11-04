// Native WebView with the production typed IPC and synthetic PC assignments.
(async () => {
  const mode = window.__externalImportSidebarMode;
  const one = (selector, root = document) => root.querySelector(selector);
  const wait = async (predicate, reason, retries = 100) => {
    for (let i = 0; i < retries; i++) {
      const value = predicate();
      if (value) return value;
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    throw Error(reason);
  };
  const assert = (condition, reason) => { if (!condition) throw Error(reason); };
  const plus = () => one('[data-sidebar-group="created"] button[aria-label="新建歌单"]');
  const hint = () => one('.sidebar-import-hint');
  const menu = () => one('[role=menu][aria-label="新建歌单"]:not([inert])');
  const choice = (key) => menu()?.querySelector(`[data-menu-key="${key}"]`);
  const importDialog = () => one('dialog[open].external-playlist-preview:not([inert])');
  const createDialog = () => one('dialog[open][aria-labelledby="playlist-dialog-title"]:not([inert])');
  const loginDialog = () => one('dialog[open].login-dialog:not([inert])');
  const button = (label, root = document) => [...root.querySelectorAll('button')]
    .find((node) => node.textContent.trim() === label);
  const press = (node, key) => node.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }));
  const click = (node) => {
    assert(node && !node.disabled, 'click target unavailable');
    node.focus({ preventScroll: true });
    node.click();
  };
  const send = (stage, extra = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, mode, ...extra },
  });
  const snap = async (stage) => {
    // DOM readiness precedes the native compositor's menu/modal frame.
    await new Promise((resolve) => setTimeout(resolve, 260));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await send(`ux_${stage}`);
  };
  const closeWithEscape = (dialog) => dialog.dispatchEvent(new Event('cancel', { cancelable: true }));
  try {
    const library = await wait(() => button('我的歌单')?.disabled === false && button('我的歌单'),
      'account bootstrap settled');
    await wait(plus, 'created playlists plus');
    plus().scrollIntoView({ block: 'center' });
    if (mode === 'absent' || mode === 'ambiguous') {
      // The native proof also asserts the request actually completed.
      await new Promise((resolve) => setTimeout(resolve, 450));
      assert(!plus().hasAttribute('aria-haspopup'), 'invalid assignment cannot enable the menu');
      click(plus());
      await wait(createDialog, 'default plus opens existing create dialog directly');
      assert(!menu() && !hint() && !importDialog(), 'no experimental entry in the default group');
      await snap('default_create');
      closeWithEscape(createDialog());
      await wait(() => !createDialog() && document.activeElement === plus(), 'direct create returns focus');
      click(library);
      await wait(() => button('歌单导入预览'), 'library fallback exists outside the experiment');
      await snap('library_fallback');
    } else {
      await wait(() => plus()?.getAttribute('aria-haspopup') === 'menu', 'literal t1 opens the experiment');
      if (mode === 'guest') {
        await new Promise((resolve) => setTimeout(resolve, 2250));
        assert(!hint(), 'guest must not see the one-time hint');
        click(plus());
        await wait(() => choice('import'), 'guest sees the import menu entry');
        click(choice('import'));
        await wait(loginDialog, 'guest import opens the ordinary sign-in dialog');
        assert(!importDialog() && !createDialog(), 'guest cannot open the import form or dispatch');
        await snap('guest_sign_in');
        click(one('[aria-label="关闭登录"]', loginDialog()));
        await wait(() => !loginDialog() && document.activeElement === plus(), 'guest sign-in returns focus');
      } else if (mode === 'reopen') {
        await new Promise((resolve) => setTimeout(resolve, 2300));
        assert(!hint(), 'one-time hint does not repeat after process restart');
        await snap('hint_not_repeated');
      } else {
        const started = performance.now();
        assert(!hint(), 'hint starts hidden');
        await wait(hint, 'one-time hint appears for t1');
        assert(performance.now() - started >= 1650, 'hint is delayed approximately two seconds');
        assert(hint().textContent.trim() === '支持导入歌单啦', 'official hint copy');
        const viewport = one('.sidebar-scroll').getBoundingClientRect();
        const bounds = hint().getBoundingClientRect();
        assert(bounds.top >= viewport.top && bounds.bottom <= viewport.bottom,
          'hint is displayed in the visible scroller, not consumed outside');
        const shown = performance.now();
        await snap('hint');
        await wait(() => !hint(), 'hint auto-hides');
        assert(performance.now() - shown >= 4500, 'hint remains visible approximately five seconds');
        plus().focus();
        press(plus(), 'ArrowDown');
        await wait(() => document.activeElement === choice('create'), 'menu first action receives focus');
        assert(choice('create').textContent.trim() === '创建歌单' &&
          choice('import').textContent.trim() === '歌单导入', 'official action names and order');
        await snap('menu');
        press(choice('create'), 'Escape');
        await wait(() => !menu() && document.activeElement === plus(), 'menu escape restores plus');
        click(plus());
        await wait(menu, 'menu reopens after Escape');
        document.body.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }));
        await wait(() => !menu(), 'outside click closes menu');
        click(plus());
        await wait(() => choice('create'), 'menu reopens');
        click(choice('create'));
        await wait(createDialog, 'create choice opens the existing create dialog');
        closeWithEscape(createDialog());
        await wait(() => !createDialog() && document.activeElement === plus(), 'create returns to plus');

        click(plus());
        await wait(() => choice('import'), 'import choice');
        click(choice('import'));
        await wait(importDialog, 'sidebar import opens the existing dialog');
        assert(importDialog().querySelector('h2').textContent.trim() === '歌单导入' &&
          document.activeElement === one('[data-import-input="0"]', importDialog()),
          'sidebar origin and initial input focus');
        const favorite = one('.external-import-destination input[type=checkbox]', importDialog());
        assert(favorite && !favorite.checked && favorite.getBoundingClientRect().width >= 14,
          'default unchecked favorites control retains its visible hit target');
        assert(document.querySelectorAll('dialog[open].external-playlist-preview').length === 1,
          'the app has only one import dialog');
        await snap('sidebar_import');
        closeWithEscape(importDialog());
        await wait(() => !importDialog() && document.activeElement === plus(), 'sidebar import returns to plus');
        click(library);
        const fallback = await wait(() => button('歌单导入预览'), 'library fallback remains present');
        click(fallback);
        await wait(importDialog, 'library route opens shared dialog');
        assert(importDialog().querySelector('h2').textContent.trim() === '歌单导入预览', 'library origin retained');
        closeWithEscape(importDialog());
        await wait(() => !importDialog() && document.activeElement === fallback, 'library import returns focus');
        await snap('library_route');
      }
    }
    await send('complete', { ok: true, realMusicRequests: 0, remoteAccountWrites: 0 });
  } catch (error) {
    await send('failed', { ok: false, error: String(error), stack: String(error.stack) });
  }
})();
