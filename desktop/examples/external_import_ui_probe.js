// Closed external import round trip. No actual login or network writes.
(async () => {
  const all = (query, root = document) => [...root.querySelectorAll(query)];
  const one = (query, root = document) => root.querySelector(query);
  const dialog = () => one('dialog[open].external-playlist-preview');
  const button = (label, root = dialog() ?? document) =>
    all('button', root).find((node) => node.textContent.trim() === label);
  const wait = async (check, reason) => {
    for (let i = 0; i < 210; i++) {
      if (check()) return;
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    throw Error(reason);
  };
  const assert = (yes, reason) => { if (!yes) throw Error(reason); };
  const click = (node) => {
    assert(node && !node.disabled, `unavailable button: ${node?.outerHTML}`);
    node.focus({ preventScroll: true });
    node.click();
  };
  const send = (stage, extra = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...extra },
  });
  const snap = (name) => send(`ux_${name}`, { visible: document.visibilityState === 'visible' });
  const observe = (state) => one(`[data-import-state="${state}"]`);
  async function open() {
    click(button('歌单导入预览', document));
    await wait(dialog, 'external import dialog');
  }
  async function preview(link) {
    const input = one('[data-import-input="0"]', dialog());
    input.value = link;
    input.dispatchEvent(new InputEvent('input', { bubbles: true }));
    await wait(() => button('预览链接') && !button('预览链接').disabled, 'preview input committed');
    click(button('预览链接'));
    await wait(() => dialog()?.textContent.includes('已识别 1 个链接'), 'native preview');
  }
  try {
    const library = () => button('我的歌单', document);
    await wait(() => library() && !library().disabled, 'playlist library entry');
    click(library());
    await wait(() => button('歌单导入预览', document), 'external import entry');
    await open();
    if (window.__externalImportReopen) {
      await wait(() => observe('unknown'), 'unknown receipt survives process reopen');
      await preview('https://www.kuwo.cn/synthetic/another');
      assert(!button('授权并导入…') && !button('提交导入…'), 'reopen forbids resubmission');
      click(button('核对本机回执'));
      await wait(() => observe('unknown'), 'reopen inspect remains unknown');
      await snap('reopened');
      await send('complete', { ok: true, reopened: true, realMusicRequests: 0, remoteAccountWrites: 0 });
      return;
    }
    await preview('分享链接 https://y.qq.com/synthetic/playlist?list=1');
    await wait(() => button('授权并导入…'), 'authorization affordance');
    await snap('preview');
    click(button('授权并导入…'));
    await wait(() => one('[aria-label="导入授权二维码"]'), 'QR rendered');
    const instruction = one('.external-import-auth [role=status]');
    const footer = one('footer', dialog());
    assert(instruction?.getBoundingClientRect().bottom <= footer.getBoundingClientRect().top,
      'QR instructions must stay visible above the footer');
    assert(one('[data-import-input="0"]').getBoundingClientRect().height === 0,
      'link inputs are retained but do not crowd the authorization panel');
    await snap('qr');
    await wait(() => dialog()?.textContent.includes('已扫码，请在网易云音乐 App 确认'), 'confirmation step');
    await snap('scanned');
    await wait(() => button('提交导入…') && !button('提交导入…').disabled, 'same owner authorized');
    click(button('提交导入…'));
    await wait(() => observe('confirm') && document.activeElement === button('取消提交'), 'safe confirm focus');
    await snap('confirm');
    click(button('取消提交'));
    await wait(() => !observe('confirm'), 'confirmation cancelled');
    await snap('cancelled');

    click(button('提交导入…'));
    await wait(() => button('确认提交导入'), 'new confirmation');
    click(button('确认提交导入'));
    await wait(() => observe('accepted'), 'receipt says accepted only');
    assert(!dialog().textContent.includes('fixture-task-id'), 'native task id is private');
    await snap('accepted');
    await preview('https://www.kuwo.cn/synthetic/playlist?list=2');
    click(button('提交导入…'));
    await wait(() => button('确认提交导入'), 'second explicit confirmation');
    click(button('确认提交导入'));
    await wait(() => observe('unknown'), 'missing task id is unknown');
    await snap('unknown');
    assert(!button('提交导入…'), 'unknown blocks submitting again');
    click(button('核对本机回执'));
    await wait(() => observe('unknown'), 'read-only local status');
    await snap('checked');
    await send('complete', { ok: true, reopened: false, realMusicRequests: 0, remoteAccountWrites: 0 });
  } catch (error) {
    await send('failed', { ok: false, error: String(error), stack: String(error.stack) });
  }
})();
