// Explicit own rank mutation through the actual WebView, with synthetic data.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (value, label) => { if (!value) throw Error(label); };
  async function wait(check, label) {
    for (let i = 0; i < 180; i++) {
      const result = check();
      if (result) return result;
      await sleep(80);
    }
    throw Error(label);
  }
  const emit = (stage, detail = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...detail },
  });
  const button = (label, root = document) => [...root.querySelectorAll('button')].find(
    (item) => item.getAttribute('aria-label') === label || item.textContent.trim() === label,
  );
  function click(item) {
    assert(item && !item.disabled, 'button unavailable');
    item.focus({ preventScroll: true });
    item.click();
  }
  function context(row) {
    const box = row.getBoundingClientRect();
    row.dispatchEvent(new MouseEvent('contextmenu', {
      bubbles: true, cancelable: true, button: 2, clientX: box.left + 10, clientY: box.bottom,
    }));
  }
  const key = (element, key, options = {}) => element.dispatchEvent(
    new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...options }),
  );
  const rank = () => one('.listening-rank');
  const row = (id) => one(`[data-rank-song="${id}"]`, rank());
  const notice = () => one('[data-rank-removal-notice]')?.textContent.trim() ?? '';
  const remove = () => one('[role=menu] [data-menu-key=remove]');
  const reconcileMenu = () => one('[role=menu] [data-menu-key=check]');
  const check = () => row(85)?.querySelector('.rank-reconcile');
  let step = 'boot';
  let count = 0;
  async function capture(name) {
    const stage = `ux_${String(++count).padStart(2, '0')}_${name}`;
    await emit(stage, { width: innerWidth, height: innerHeight });
  }
  async function openProfile(playlist, name) {
    click(button('我的歌单'));
    click(await wait(() => button('打开歌单链接'), 'playlist library'));
    const input = await wait(() => one('[aria-label="歌单分享链接"]'), 'playlist link dialog');
    input.value = String(playlist);
    input.dispatchEvent(new Event('input', { bubbles: true }));
    click(one('.playlist-dialog button.primary'));
    click(await wait(() => !one('.playlist-dialog[open]') && one('button.playlist-creator'), 'playlist creator'));
    await wait(() => one('.playlist-creator-page h1')?.textContent.trim() === name &&
      one('.playlist-creator-page').getAttribute('aria-busy') === 'false', 'profile: ' + name);
    click(await wait(() => one('[data-profile-rank-entry]'), 'rank entry'));
  }
  try {
    await wait(() => one('.track-row'), 'signed in with daily tracks');
    await emit('resize', { width: 960, height: 700 });
    await wait(() => innerWidth === 960 && innerHeight === 700, 'resized');

    step = 'guest';
    await openProfile(20, '合成听友 3');
    await wait(() => row(81), 'guest rank loaded');
    context(row(81));
    key(row(81), 'Delete');
    assert(!remove() && !reconcileMenu() && row(81), 'guest cannot delete');
    await capture('guest_no_removal');

    step = 'week-menu';
    await openProfile(2, '合成自己');
    await wait(() => row(84) && row(86), 'own week loaded');
    context(row(84));
    await wait(remove, 'row context menu');
    assert(remove().textContent.includes('从列表中删除') &&
      remove().textContent.includes('删除我的最近一周听歌排行记录'), 'period is in the action name');
    await capture('owner_context');
    key(remove(), 'Escape');
    await wait(() => !remove() && document.activeElement === row(84), 'escape returns row focus');

    step = 'confirmed';
    row(84).focus();
    key(row(84), 'F10', { shiftKey: true });
    click(await wait(remove, 'keyboard context menu'));
    await wait(() => rank()?.getAttribute('aria-busy') === 'true', 'awaiting native acknowledgement');
    assert(row(84) && !one('dialog[open]'), 'direct action awaits 200 without optimism or confirmation');
    await capture('awaiting_ack');
    await emit('profile-rank-release-delete');
    await wait(() => !row(84) && row(86) && notice().includes('已从我的最近一周'), 'only confirmed row disappears');
    await wait(() => document.activeElement === row(86), 'focus moves to next row');
    await capture('confirmed_week');

    step = 'keyboard';
    const nested = one('.rank-song', row(86));
    nested.focus();
    key(nested, 'Delete');
    row(86).focus();
    key(row(86), 'Backspace', { repeat: true });
    assert(row(86) && rank().getAttribute('aria-busy') === 'false', 'nested and repeated keys ignored');
    await capture('keyboard_ignored');
    key(row(86), 'Backspace');
    await wait(() => !row(86) && rank().textContent.includes('暂无听歌排行'), 'focused row Backspace removes');
    await wait(() => document.activeElement === one('h1', rank()), 'empty rank restores heading focus');
    await capture('keyboard_confirmed');

    step = 'ambiguous';
    click(button('所有时间', rank()));
    await wait(() => row(84) && row(85), 'all-time still has the week song');
    context(row(85));
    await wait(remove, 'all-time removal menu');
    assert(remove().textContent.includes('删除我的所有时间听歌排行记录'), 'all-time action is scoped');
    click(remove());
    await wait(() => check() && notice().includes('删除结果尚未确认'), 'lost reply is visible and row retained');
    await capture('uncertain_all');
    click(button('最近一周', rank()));
    await wait(() => !row(85) && rank().textContent.includes('暂无听歌排行'), 'week stays empty');
    click(button('所有时间', rank()));
    await wait(check, 're-enter all-time preserves pending state');
    context(row(85));
    await wait(reconcileMenu, 'only readback offered for pending row');
    assert(!remove(), 'pending item cannot resubmit');
    await capture('reentered_pending');
    key(reconcileMenu(), 'Escape');
    click(check());
    await wait(() => check() && !check().disabled && notice().includes('删除结果尚未确认'), 'present readback remains uncertain');
    await capture('readback_present');

    await emit('profile-rank-displace-row');
    await wait(() => window.__profileRankDisplaced, 'synthetic list change acknowledged');
    click(check());
    await wait(() => !row(85) && row(84) && notice().includes('无法确认之前的删除是否生效'),
      'absent readback is explicitly not a confirmed write');
    await capture('readback_absent');
    await emit('complete', { ok: true, stageCount: count });
  } catch (error) {
    await emit('failed', { ok: false, step, error: String(error?.message || error),
      active: document.activeElement?.outerHTML.slice(0, 140),
      stack: String(error?.stack || '') });
  }
})();
