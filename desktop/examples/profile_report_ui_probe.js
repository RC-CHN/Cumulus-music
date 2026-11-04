// Installed IPC and real WebView against a closed synthetic profile report service.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])];
  const button = (name, root = document) => all('button', root).find(
    (node) => (node.getAttribute('aria-label') || node.textContent.trim()) === name,
  );
  const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const dialog = () => one('[data-profile-report-dialog][open]');
  const outcome = () => one('[data-profile-report-outcome]', dialog())?.textContent.trim();
  const more = () => button('更多用户操作');
  const choices = () => one('[role="menu"][aria-label="举报原因"]');
  const confirm = () => button('确认', dialog());
  const assert = (value, label) => { if (!value) throw Error(label); };
  const emit = (stage, payload = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...payload },
  });
  let count = 0;
  async function capture(label, visible) {
    await Promise.race([
      new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))),
      pause(200),
    ]);
    await pause(160);
    const rect = visible.getBoundingClientRect();
    const element = document.elementFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2);
    assert(visible.contains(element), `unreachable capture element for ${label}: ${element?.outerHTML.slice(0, 140)}`);
    assert(document.documentElement.scrollWidth <= innerWidth, 'horizontal overflow');
    await emit(`ux_${String(++count).padStart(2, '0')}_${label}`, { width: innerWidth, open: !!dialog() });
  }
  async function wait(test, label) {
    for (let i = 0; i < 180; i++) {
      const result = test();
      if (result) return result;
      await pause(80);
    }
    throw Error(label);
  }
  function click(node) {
    assert(node && !node.disabled, 'missing or disabled action');
    node.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    node.focus({ preventScroll: true });
    node.click();
  }
  function key(node, value) {
    assert(node && document.activeElement === node, `wrong keyboard focus for ${value}`);
    node.dispatchEvent(new KeyboardEvent('keydown', { key: value, bubbles: true, cancelable: true }));
  }
  function input(node, value) {
    node.value = value;
    node.dispatchEvent(new Event('input', { bubbles: true }));
  }
  async function openProfile(playlist, uid) {
    click(button('我的歌单'));
    click(await wait(() => button('打开歌单链接'), 'playlist library'));
    const url = await wait(() => one('[aria-label="歌单分享链接"]'), 'playlist link dialog');
    input(url, String(playlist));
    click(one('.playlist-dialog button.primary'));
    click(await wait(() => one('.playlist-hero h1')?.textContent === `合成举报歌单 ${playlist}` &&
      !one('.playlist-dialog[open]') && one('button.playlist-creator'), 'playlist creator'));
    await wait(() => one('.playlist-creator-page h1')?.textContent === `合成举报用户 ${uid}` && more(),
      'verified reportable profile header');
  }
  async function menu() {
    more().focus();
    key(more(), 'ArrowDown');
    const blacklist = await wait(() => button('加黑名单'), 'blacklist');
    key(blacklist, 'ArrowDown');
    const report = one('[role="menuitem"][aria-haspopup="menu"]');
    key(report, 'ArrowRight');
    return wait(choices, 'report reason submenu');
  }
  async function choose(index) {
    const list = await menu();
    click(all('button', list)[index]);
    return wait(dialog, 'report dialog');
  }
  async function close(name) {
    click(button(name, dialog()));
    await wait(() => !dialog() && document.activeElement === more(), 'close and restore profile More focus');
  }
  try {
    await wait(() => button('我的歌单') && !button('我的歌单').disabled, 'application ready');
    await openProfile(31, 73);
    const list = await menu();
    const items = all('button', list);
    assert(JSON.stringify(items.map(item => item.textContent.trim().replace(/\s*›$/, ''))) ===
      JSON.stringify(['广告或营销信息', '恶意人身攻击', '色情、低俗或有害内容', '政治敏感言论', '其他原因']),
      'official report reasons or ordering changed');
    await capture('reasons', items[0]);
    key(items[0], 'ArrowLeft');
    key(one('[role="menuitem"][aria-haspopup="menu"]'), 'ArrowRight');
    await wait(() => choices() && document.activeElement === one('button', choices()), 'reason focus restored');
    click(all('button', choices())[1]);
    await wait(() => dialog() && confirm() && !confirm().disabled, 'preset confirmation');
    assert(one('.profile-report-preset', dialog())?.textContent === '恶意人身攻击', 'selected preset changed');
    await capture('preset_confirm', confirm());
    await close('取消');
    await capture('cancelled', more());

    await choose(4);
    const field = one('textarea', dialog());
    await wait(() => document.activeElement === field && confirm()?.disabled, 'textarea autofocus and blank confirm');
    assert(field.placeholder === '举报原因：', 'official text placeholder');
    input(field, ' \ufeff\t\n');
    await pause(60);
    assert(confirm().disabled, 'whitespace should not enable report');
    input(field, '音'.repeat(1366));
    await wait(() => field.getAttribute('aria-invalid') === 'true' && confirm().disabled, 'UTF-8 byte cap');
    input(field, ' \u3000合成描述 / 100%\n末行 \t');
    await wait(() => !confirm()?.disabled, 'custom confirmation enabled');
    await capture('custom_confirm', confirm());
    click(confirm());
    await wait(() => outcome() === '举报成功' && !confirm() && !one('textarea', dialog()), 'custom acknowledgement');
    await capture('accepted', button('关闭', dialog()));
    await close('关闭');
    await choose(1);
    await wait(() => outcome() === '举报成功' && !confirm(), 'successful receipt prevents further reports');
    await close('关闭');

    await openProfile(33, 75);
    await choose(0);
    click(await wait(() => confirm() && !confirm().disabled && confirm(), 'preset confirmation target 75'));
    await wait(() => outcome()?.includes('请勿重复提交') && !confirm(), 'lost acknowledgement and no replay');
    await capture('uncertain', button('核对本机回执', dialog()));
    click(button('核对本机回执', dialog()));
    await wait(() => dialog()?.getAttribute('aria-busy') === 'false' && outcome()?.includes('请勿重复提交'),
      'local-only receipt lookup');
    await close('关闭');
    await choose(4);
    await wait(() => outcome()?.includes('请勿重复提交') && !confirm() && !one('textarea', dialog()),
      'uncertain receipt guards another reason');
    await capture('reopened', button('关闭', dialog()));
    await emit('complete', { ok: true, stageCount: count, realMusicRequests: 0, remoteWrites: 0 });
  } catch (error) {
    await emit('failed', { message: String(error), stageCount: count });
  }
})();
