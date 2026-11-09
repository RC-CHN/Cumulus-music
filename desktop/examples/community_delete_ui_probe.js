// Explicit own-note delete and conservative readback through actual Tauri/WebView.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])];
  const button = (name, root = document) => all('button', root).find(
    (node) => (node.getAttribute('aria-label') || node.textContent.trim()) === name,
  );
  const dialog = () => one('.community-delete-dialog[open]');
  const activeCard = (node) => node?.isConnected && node.getClientRects().length &&
    !node.closest('[inert],.page-enter-active,.page-leave-active');
  const card = (title) => all('article[data-event-key]').find(
    (node) => activeCard(node) && one('h2', node)?.textContent === title,
  );
  const menu = (row) => row ? one(`[data-event-menu-key="${row.dataset.eventKey}"]`) : null;
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const paint = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const assert = (value, label) => { if (!value) throw Error(label); };
  const emit = (stage, payload = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...payload },
  });
  async function wait(test, label) {
    for (let i = 0; i < 150; i++) {
      const result = test();
      if (result) return result;
      await sleep(80);
    }
    throw Error(label);
  }
  function click(node, reveal = true) {
    assert(node && !node.disabled, 'missing or disabled action');
    if (reveal) node.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    node.focus({ preventScroll: true });
    node.click();
  }
  async function openMenu(row, rightClick = false) {
    await wait(() => activeCard(row) && !one('.page-enter-active,.page-leave-active'),
      'community page settled');
    const anchor = rightClick ? row : button('动态操作', row);
    assert(anchor?.isConnected, 'event menu anchor is present');
    // The real menu closes if the initial reveal scroll moves its anchor.
    anchor.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    const painted = await Promise.race([paint().then(() => true), sleep(2000).then(() => false)]);
    assert(painted && activeCard(row) && anchor.isConnected, 'event menu reveal painted');
    if (rightClick) anchor.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    else click(anchor, false);
  }
  let stageCount = 0;
  async function capture(label) {
    await sleep(180);
    assert(document.documentElement.scrollWidth <= innerWidth, 'horizontal overflow');
    await emit(`ux_${String(++stageCount).padStart(2, '0')}_${label}`, {
      width: innerWidth, confirming: !!dialog(), status: dialog()?.querySelector('[role="status"]')?.textContent ?? '',
    });
  }
  try {
    await wait(() => button('关注') && !button('关注').disabled, 'app ready');
    await emit('resize');
    await wait(() => innerWidth === 760, 'resize');
    click(button('关注'));
    const own = await wait(() => card('合成待删笔记'), 'own card');
    const guest = card('合成来宾笔记');
    assert(guest && button('动态操作', guest), 'guest report menu missing');
    await openMenu(guest, true);
    await wait(() => one('[aria-haspopup="menu"][role="menuitem"]', menu(guest)), 'guest report only');
    assert(!button('删除', menu(guest)), 'guest context menu exposed delete');
    await capture('owner_only');
    const opener = button('动态操作', own);
    await openMenu(own);
    click(await wait(() => button('删除', menu(own)), 'own ellipsis menu'));
    await wait(() => button('确定', dialog()) && !button('确定', dialog()).disabled, 'confirmation');
    await wait(() => document.activeElement === button('取消', dialog()), 'confirmation defaults to Cancel');
    await capture('confirm');
    click(button('取消', dialog()));
    await wait(() => !dialog() && document.activeElement === opener, 'cancel restores focus');
    await capture('cancel');
    click(one('[data-community-opener]', own));
    const detail = await wait(() => one('.community-heading h1')?.textContent === '动态详情' && card('合成待删笔记'), 'own note detail');
    await openMenu(detail);
    click(await wait(() => button('删除', menu(detail)), 'detail menu'));
    click(await wait(() => {
      const ok = button('确定', dialog()); return ok && !ok.disabled && ok;
    }, 'detail confirmation'));
    await wait(() => !dialog() && !card('合成待删笔记') && card('合成未知笔记') &&
      document.body.textContent.includes('动态已删除'), 'ack back without stale card');
    await capture('ack_detail_back');
    const unknown = card('合成未知笔记');
    await openMenu(unknown, true);
    click(await wait(() => button('删除', menu(unknown)), 'own right-click menu'));
    click(await wait(() => {
      const ok = button('确定', dialog()); return ok && !ok.disabled && ok;
    }, 'unknown confirmation'));
    await wait(() => dialog()?.textContent.includes('客户端不会再次发送删除请求'), 'uncertain state');
    await capture('uncertain');
    click(button('核对删除结果', dialog()));
    await wait(() => dialog()?.textContent.includes('无法据此确认删除成功'), '404 is not success');
    assert(!!card('合成未知笔记'), '404 removed unconfirmed card');
    await capture('absence');
    click(button('关闭', dialog()));
    await wait(() => !dialog(), 'dismiss unknown');
    await openMenu(unknown);
    click(await wait(() => button('删除', menu(unknown)), 'reopen own menu'));
    await wait(() => dialog()?.textContent.includes('客户端不会再次发送删除请求'), 'reopen durable pending');
    assert(!button('确定', dialog()), 'pending offered another delete');
    await capture('reopen_no_replay');
    click(button('核对删除结果', dialog()));
    await wait(() => !dialog() && !card('合成未知笔记') && !card('合成待删笔记') && card('合成来宾笔记') &&
      document.body.textContent.includes('核对时发现这条动态已移除'), 'exact tombstone keeps unrelated guest only');
    await capture('observed_removed');
    await emit('complete', { ok: true, stageCount, realMusicRequests: 0, remoteWrites: 0 });
  } catch (error) {
    await emit('failed', { error: String(error), stageCount });
  }
})();
