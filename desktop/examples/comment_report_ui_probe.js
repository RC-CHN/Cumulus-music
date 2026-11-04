// Real WebView, encrypted PC request plans and synthetic moderation replies.
(async () => {
  if (window.__commentReportStarted) return;
  window.__commentReportStarted = true;
  const one = (selector, scope = document) => scope?.querySelector(selector);
  const all = (selector, scope = document) => [...(scope?.querySelectorAll(selector) ?? [])];
  const button = (label, scope = document) => all('button', scope).find(
    (node) => node.textContent.trim().replace(/\s*›$/, '') === label,
  );
  const dialog = () => one('[data-moderation-report-dialog][open]:not([data-profile-report-dialog])');
  const menu = () => one('[data-comment-report-menu]');
  const outcome = () => one('[data-moderation-report-outcome]', dialog())?.textContent.trim();
  const main = (id) => one(`[data-group="page"] [data-comment-id="${id}"]`);
  const floor = (id) => one(`.comment-floor-panel [data-comment-id="${id}"]`);
  const trigger = (card) => one('[data-comment-report]', card);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (condition, label) => { if (!condition) throw Error(label); };
  const emit = (stage, detail = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...detail },
  });
  async function wait(test, label) {
    for (let i = 0; i < 220; i++) {
      const result = test();
      if (result) return result;
      await sleep(80);
    }
    throw Error(label);
  }
  async function click(node) {
    assert(node && !node.disabled, 'missing or disabled action');
    node.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    await sleep(90);
    node.focus({ preventScroll: true });
    node.click();
  }
  function key(node, name) {
    assert(document.activeElement === node, `wrong focus for ${name}`);
    node.dispatchEvent(new KeyboardEvent('keydown', { key: name, bubbles: true, cancelable: true }));
  }
  async function rightClick(card) {
    card.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    await sleep(90);
    const rect = card.getBoundingClientRect();
    card.dispatchEvent(new MouseEvent('contextmenu', {
      bubbles: true, cancelable: true, clientX: rect.left + 80, clientY: rect.top + 20,
    }));
    return wait(menu, 'right-click context');
  }
  let count = 0;
  async function stage(name, visible) {
    await Promise.race([
      new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))), sleep(220),
    ]);
    await sleep(180);
    const rect = visible.getBoundingClientRect();
    assert(rect.top >= 0 && rect.bottom <= innerHeight && rect.right <= innerWidth,
      `offscreen: ${name}`);
    assert(visible.contains(document.elementFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2)),
      `covered: ${name}`);
    await emit(`ux_${String(++count).padStart(2, '0')}_${name}`, {
      width: innerWidth, open: !!dialog(), menu: !!menu(),
    });
  }
  async function choose(card, name) {
    await click(trigger(card));
    await wait(() => menu()?.getAttribute('aria-label') === '举报原因', 'direct footer reasons');
    await click(button(name, menu()));
    await wait(dialog, 'comment confirmation');
  }
  const ready = () => button('确认', dialog()) && !button('确认', dialog()).disabled && button('确认', dialog());
  async function close(card, label) {
    await click(button(label, dialog()));
    await wait(() => !dialog() && document.activeElement === trigger(card), 'origin report focus');
  }
  try {
    await click(await wait(() => one('.track-row .track-more'), 'daily bootstrap'));
    await click(await wait(() => button('查看评论'), 'track comments action'));
    await wait(() => trigger(main('102')) && main('101'), 'comment page ready');
    const own = main('101'), guest = main('102');
    assert(!trigger(own) && button('删除', own), 'own comment must not show report');
    const ownMenu = await rightClick(own);
    assert(all('button', ownMenu).length === 1 && button('删除', ownMenu), 'own menu must only allow delete');
    await stage('own_menu', button('删除', ownMenu));
    key(button('删除', ownMenu), 'Escape');
    await wait(() => !menu() && document.activeElement === button('删除', own), 'own context close focus');

    await click(trigger(guest));
    const names = ['段子或者无意义的评论', '广告或营销信息', '恶意人身攻击',
      '色情、低俗或有害内容', '政治敏感言论', '其他原因'];
    await wait(() => menu() && button(names[0], menu()), 'guest reasons');
    assert(JSON.stringify(all('button', menu()).map((n) => n.textContent.trim().replace(/\s*›$/, ''))) ===
      JSON.stringify(names), 'official ordered reasons');
    await stage('direct_reasons', button(names[0], menu()));
    await click(button(names[1], menu()));
    await wait(ready, 'preset explicit confirm');
    await stage('preset_confirm', ready());
    await close(guest, '取消');
    await stage('cancelled', trigger(guest));

    const guestMenu = await rightClick(guest);
    const action = button('举报', guestMenu);
    assert(all('button', guestMenu).length === 1 && action, 'right-click root report submenu');
    await stage('guest_context', action);
    key(action, 'ArrowRight');
    await wait(() => menu() && document.activeElement === button(names[0], menu()), 'right-click submenu focus');
    await click(button(names[0], menu()));
    await click(await wait(ready, 'meaningless confirmation'));
    await wait(() => outcome() === '举报成功' && !ready(), 'preset accepted');
    await stage('accepted', button('关闭', dialog()));
    await close(guest, '关闭');

    await click(button('查看回复', guest));
    const reply = await wait(() => trigger(floor('201')) && floor('201'), 'reportable reply');
    await choose(reply, names[5]);
    const field = one('textarea', dialog());
    assert(field.placeholder === '举报原因：' && document.activeElement === field, 'custom autofocus');
    field.value = ' \uFEFF\t\n';
    field.dispatchEvent(new Event('input', { bubbles: true }));
    await sleep(80);
    assert(button('确认', dialog()).disabled, 'whitespace must not allow a request');
    field.value = ' \u3000合成楼层举报 100%\n末行 \t';
    field.dispatchEvent(new Event('input', { bubbles: true }));
    await stage('custom_floor', await wait(ready, 'raw confirm'));
    await click(ready());
    await wait(() => outcome()?.includes('请勿重复提交'), 'lost reply');
    await stage('uncertain', button('核对本机回执', dialog()));
    await click(button('核对本机回执', dialog()));
    await wait(() => dialog()?.getAttribute('aria-busy') === 'false' && outcome()?.includes('请勿重复提交'), 'local inspect');
    await close(reply, '关闭');
    await choose(reply, names[1]);
    await wait(() => outcome()?.includes('请勿重复提交') && !button('确认', dialog()), 'reopen no replay');
    await close(reply, '关闭');

    const next = floor('202');
    await choose(next, names[4]);
    await click(await wait(ready, 'duplicate confirmation'));
    await wait(() => outcome() === '重复举报', 'official duplicate result');
    await stage('duplicate', button('关闭', dialog()));
    await close(next, '关闭');
    await emit('complete', { ok: true, stageCount: count, realMusicRequests: 0, remoteWrites: 0 });
  } catch (error) {
    await emit('failed', { message: String(error), stageCount: count });
  }
})();
