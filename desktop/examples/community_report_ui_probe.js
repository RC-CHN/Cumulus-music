// Live Tauri/WebView with a closed synthetic event-report API.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])];
  const button = (name, root = document) => all('button', root).find(
    (node) => (node.getAttribute('aria-label') || node.textContent.trim().replace(/\s*›$/, '')) === name,
  );
  const dialog = () => one('[data-moderation-report-dialog][open]');
  const outcome = () => one('[data-moderation-report-outcome]', dialog())?.textContent.trim();
  const card = (title) => all('article[data-event-key]').find(
    (node) => one('h2', node)?.textContent === title && node.getClientRects().length,
  );
  const opener = (row) => button('动态操作', row);
  const menu = (row) => row ? one(`[data-event-menu-key="${row.dataset.eventKey}"]`) : null;
  const reasons = (row) => one('[data-community-report-reasons]', menu(row));
  const action = (row) => one('[role="menuitem"][aria-haspopup="menu"]', menu(row));
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const paint = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const assert = (value, label) => { if (!value) throw Error(label); };
  const emit = (stage, payload = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...payload },
  });
  async function wait(test, label) {
    for (let i = 0; i < 200; i++) {
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
  function key(node, value) {
    assert(node && document.activeElement === node, `wrong keyboard focus for ${value}`);
    node.dispatchEvent(new KeyboardEvent('keydown', { key: value, bubbles: true, cancelable: true }));
  }
  function context(node, reveal = true) {
    if (reveal) node.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    node.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
  }
  let stageCount = 0;
  async function capture(label, element, detail = {}) {
    await Promise.race([paint(), sleep(220)]);
    await sleep(190);
    assert(document.documentElement.scrollWidth <= innerWidth, 'horizontal overflow');
    const rect = element.getBoundingClientRect();
    assert(rect.top >= 0 && rect.bottom <= innerHeight && rect.right <= innerWidth, `offscreen ${label}`);
    const hit = document.elementFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2);
    assert(element.contains(hit), `covered ${label}: ${JSON.stringify({
      connected: element.isConnected, rect: rect.toJSON(), hit: hit?.tagName,
      hitClass: hit?.getAttribute('class'), focus: document.activeElement?.getAttribute('aria-label'),
      activeTransitions: all('.page-enter-active,.page-leave-active').length,
    })}`);
    await emit(`ux_${String(++stageCount).padStart(2, '0')}_${label}`, { width: innerWidth, ...detail });
  }
  async function openMenu(row, rightClick = false) {
    await wait(() => !one('.page-enter-active,.page-leave-active'), 'community page settled');
    const anchor = rightClick ? row : opener(row);
    assert(anchor?.isConnected, 'event menu anchor is present');
    // Wait for the initial reveal scroll before opening a menu whose anchor
    // intentionally becomes invalid when that scroll moves it.
    anchor.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    const painted = await Promise.race([paint().then(() => true), sleep(2000).then(() => false)]);
    assert(painted && anchor.isConnected, 'event menu reveal painted');
    if (rightClick) context(anchor, false);
    else click(anchor, false);
  }
  async function openReasons(row, rightClick = false) {
    await openMenu(row, rightClick);
    const report = await wait(() => action(row), 'report submenu');
    await wait(() => document.activeElement === report, 'report menu focus');
    key(report, 'ArrowRight');
    return wait(() => reasons(row), 'reasons expanded');
  }
  const ready = () => {
    const confirm = button('确认', dialog());
    return confirm && !confirm.disabled && confirm;
  };
  async function choose(row, label, rightClick = false) {
    const list = await openReasons(row, rightClick);
    click(button(label, list));
    return wait(dialog, 'event confirmation');
  }
  async function close(row, label) {
    click(button(label, dialog()));
    await wait(() => !dialog() && document.activeElement === opener(row), 'origin focus restored');
  }
  try {
    await wait(() => button('关注') && !button('关注').disabled, 'app ready');
    await emit('resize');
    await wait(() => innerWidth === 760, 'small native window');
    click(button('关注'));
    let first = await wait(() => card('合成可举报笔记'), 'community feed');
    const own = card('合成自己笔记');
    const anonymous = card('合成匿名笔记');
    assert(own && anonymous && !opener(anonymous), 'anonymous target exposed');
    const nested = one('[data-community-forwarded]', card('合成可举报转发'));
    assert(nested && !opener(nested), 'nested forward target exposed');
    context(nested);
    assert(!menu(card('合成可举报转发')), 'nested context activated outer event');
    await openMenu(own);
    await wait(() => button('删除', menu(own)), 'own deletion retained');
    assert(!action(own), 'own report exposed');
    await capture('eligible', button('删除', menu(own)));
    key(button('删除', menu(own)), 'Escape');

    const list = await openReasons(first);
    const labels = ['广告或营销信息', '恶意人身攻击', '色情、低俗或有害内容', '政治敏感言论', '其他原因'];
    assert(JSON.stringify(all('button', list).map((node) => node.textContent.trim().replace(/\s*›$/, ''))) ===
      JSON.stringify(labels), 'official reasons or order changed');
    await capture('reasons', button(labels[0], list));
    click(button(labels[0], list));
    await wait(ready, 'preset explicit confirmation');
    await close(first, '取消');
    await capture('cancelled', opener(first), { openerFocused: document.activeElement === opener(first) });

    click(one('[data-community-opener]', first));
    const detail = await wait(() => one('.community-heading h1')?.textContent === '动态详情' &&
      card('合成可举报笔记'), 'verified event detail');
    await choose(detail, labels[3], true);
    await capture('detail', await wait(ready, 'detail report confirm'));
    click(ready());
    await wait(() => outcome() === '举报成功', 'accepted event report');
    await capture('accepted', button('关闭', dialog()));
    await close(detail, '关闭');
    click(button('返回上一页'));
    first = await wait(() => one('.community-heading h1')?.textContent !== '动态详情' &&
      card('合成可举报笔记'), 'return leaves event intact');
    await choose(first, labels[1]);
    await wait(() => outcome() === '举报成功' && !button('确认', dialog()), 'receipt shared across card and detail');
    await close(first, '关闭');

    const forward = card('合成可举报转发');
    await choose(forward, labels[4], true);
    const field = one('textarea', dialog());
    assert(field?.placeholder === '举报原因：' && document.activeElement === field, 'custom autofocus');
    field.value = ' \uFEFF\t\n';
    field.dispatchEvent(new Event('input', { bubbles: true }));
    await sleep(80);
    assert(button('确认', dialog()).disabled, 'empty raw reason enabled');
    field.value = ' \u3000合成转发举报 100%\n末行 \t';
    field.dispatchEvent(new Event('input', { bubbles: true }));
    await capture('forward_custom', await wait(ready, 'custom ready'));
    click(ready());
    await wait(() => outcome()?.includes('请勿重复提交') && !button('确认', dialog()), 'unknown receipt');
    await capture('uncertain', button('核对本机回执', dialog()));
    click(button('核对本机回执', dialog()));
    await wait(() => dialog()?.getAttribute('aria-busy') === 'false' && outcome()?.includes('请勿重复提交'),
      'read-only local lookup');
    await close(forward, '关闭');
    await choose(forward, labels[0]);
    await wait(() => outcome()?.includes('请勿重复提交') && !button('确认', dialog()), 'reopen without replay');
    await capture('reopened', button('关闭', dialog()), { uncertain: true });
    await close(forward, '关闭');

    const duplicate = card('合成已举报动态');
    await choose(duplicate, labels[1]);
    click(await wait(ready, 'duplicate confirm'));
    await wait(() => outcome() === '重复举报', 'official duplicate outcome');
    await capture('duplicate', button('关闭', dialog()));
    await close(duplicate, '关闭');
    await emit('complete', { ok: true, stageCount, realMusicRequests: 0, remoteWrites: 0 });
  } catch (error) {
    await emit('failed', { message: String(error), stageCount });
  }
})();
