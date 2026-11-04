// Actual desktop UI and production IPC, with a closed synthetic service.
(async () => {
  const all = (selector, root = document) => [...root.querySelectorAll(selector)];
  const one = (selector) => document.querySelector(selector);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (condition, label) => { if (!condition) throw Error(label); };
  const emit = (stage, rest = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...rest },
  });
  async function wait(test, label) {
    for (let i = 0; i < 180; i++) {
      const value = test();
      if (value) return value;
      await sleep(70);
    }
    throw Error(label);
  }
  const page = () => one('.community-page');
  const composer = () => one('.community-composer[open]');
  const textarea = () => composer()?.querySelector('textarea');
  const button = (label, root = document) =>
    root && all('button', root).find((node) => node.textContent.trim() === label);
  const control = (label) => button(label, composer());
  const notice = () => one('[data-community-publication]')?.textContent.trim();
  const enabled = (node) => node?.isConnected && !node.disabled && node;
  function click(node) {
    assert(enabled(node), 'enabled control required: ' + node?.textContent);
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    node.click();
  }
  function input(node, value) {
    node.value = value;
    node.dispatchEvent(new Event('input', { bubbles: true }));
  }
  let count = 0;
  let active = 'startup';
  async function capture(label) {
    active = label;
    await Promise.race([
      new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))),
      sleep(180),
    ]);
    await sleep(230);
    assert(document.documentElement.scrollWidth <= innerWidth, 'horizontal overflow');
    await emit(`ux_${String(++count).padStart(2, '0')}_${label}`, { width: innerWidth });
  }
  async function choose(label, type) {
    click(control('添加音乐附件'));
    const field = await wait(() => one('[aria-label="搜索音乐附件"]'), 'attachment search available');
    if (type !== '单曲') click(control(type));
    input(field, '合成附件');
    click(await wait(() => enabled(control('搜索')), 'search enabled'));
    click(await wait(() => enabled(control(label)), 'exact attachment result'));
    await wait(() => composer()?.textContent.includes(type + '：' + label), 'attachment persisted in editor');
  }
  async function ask() {
    click(await wait(() => enabled(control('发布笔记')), 'exact eligible note may publish'));
    await wait(() => one('[data-community-publication-confirm]') &&
      document.activeElement === control('取消'), 'explicit confirmation and default cancel focus');
  }
  async function open() {
    click(await wait(() => enabled(button('写笔记', page())), 'composer opener'));
    await wait(() => textarea() && document.activeElement === textarea(), 'durable composer ready');
  }
  async function close() {
    composer().dispatchEvent(new Event('cancel', { cancelable: true }));
    await wait(() => !composer(), 'composer fully closed');
  }
  try {
    click(await wait(() => enabled(button('关注')), 'authenticated community entry'));
    await wait(() => page()?.querySelector('[role=alert]'), 'isolated first feed failure');
    click(button('重试', page()));
    await wait(() => button('写笔记', page()) && page().textContent.includes('合成首条'),
      'original feed ready');
    await emit('resize');
    await wait(() => innerWidth === 760, 'narrow native WebView');
    await open();
    input(textarea(), '公开发布的合成正文');
    await wait(() => control('发布笔记')?.disabled, 'missing attachment cannot publish');
    await choose('合成附件专辑', '专辑');
    await ask();
    await capture('publication_confirm');
    click(control('取消'));
    await wait(() => enabled(control('发布笔记')) === document.activeElement,
      'confirmation cancel returns focus');
    await capture('publication_cancel');

    await ask();
    click(control('确认公开发布'));
    await wait(() => one('.account-binding-qr-dialog[open] [data-account-binding-qr] path[d]'),
      'missing binding opens fixed native QR');
    assert(textarea().value === '公开发布的合成正文' && notice()?.includes('需要绑定手机号'),
      'phone gate keeps the draft');
    await capture('publication_phone');
    click(one('[aria-label="关闭扫码设置"]'));
    await wait(() => !one('.account-binding-qr-dialog[open]') &&
      document.activeElement === control('发布笔记'), 'QR closes and restores explicit publish action');

    await ask();
    click(control('确认公开发布'));
    await wait(() => notice()?.includes('保存发布成功回执') && textarea().value === '' &&
      document.activeElement === control('查看已发布笔记'),
      'positive event acknowledgement consumes exact draft only');
    await capture('publication_ack');
    await close();
    await open();
    await wait(() => enabled(control('查看已发布笔记')), 'stored acknowledgement survives reopening');
    click(control('查看已发布笔记'));
    await wait(() => page()?.textContent.includes('公开发布的合成正文') &&
      one('.community-heading h1')?.textContent === '动态详情', 'canonical published note detail');
    await capture('publication_detail');
    click(one('[aria-label="返回上一页"]'));
    await wait(() => document.activeElement === button('写笔记', page()), 'Back restores opener');

    await open();
    input(textarea(), '结果未知的合成正文');
    await choose('合成附件播客', '播客');
    await ask();
    click(control('确认公开发布'));
    await wait(() => notice()?.includes('无法确认是否已经发布') && textarea().disabled &&
      document.activeElement === control('核对本机回执'),
      'lost receipt keeps the exact draft read-only');
    assert(textarea().value === '结果未知的合成正文' && control('发布笔记').disabled &&
      control('删除本机草稿').disabled, 'unknown publication cannot be resent or discarded');
    await capture('publication_unknown');
    click(control('核对本机回执'));
    await wait(() => enabled(control('核对本机回执')) &&
      composer().textContent.includes('已核对本机记录，发布结果仍未确认'),
      'read-only local reconciliation stays uncertain');
    await close();
    click(button('写笔记', page()));
    await wait(() => notice()?.includes('无法确认是否已经发布') &&
      textarea()?.value === '结果未知的合成正文' && textarea().disabled,
      'unknown durable journal and body are restored independently');
    await capture('publication_reopened');
    click(control('核对最近笔记'));
    await wait(() => composer()?.textContent.includes('最近笔记里没有找到可确认的回执') &&
      textarea().disabled && textarea().value === '结果未知的合成正文', 'no matching UUID stays uncertain');
    await capture('publication_readback_absent');
    click(control('核对最近笔记'));
    await wait(() => notice()?.includes('保存发布成功回执') && textarea().value === '' &&
      document.activeElement === control('查看已发布笔记'), 'positive UUID readback settles exact draft');
    await capture('publication_recovered');
    click(control('查看已发布笔记'));
    await wait(() => page()?.textContent.includes('结果未知的合成正文') &&
      one('.community-heading h1')?.textContent === '动态详情', 'canonical recovered detail');
    await capture('publication_recovered_detail');
    await emit('complete', { ok: true, stageCount: count, publicationVerified: true,
      realMusicRequests: 0, remoteWrites: 0 });
  } catch (error) {
    await emit('failed', { ok: false, stageCount: count, active, error: String(error) });
  }
})();
