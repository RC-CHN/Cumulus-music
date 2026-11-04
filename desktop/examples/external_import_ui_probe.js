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
  async function snap(name) {
    const state = { visible: document.visibilityState === 'visible' };
    if (window.__CUMULUS_PROOF_INVOKE__) {
      await wait(() => dialog() && getComputedStyle(dialog()).opacity === '1', 'import dialog is opaque');
      await Promise.race([
        new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))),
        new Promise((resolve) => setTimeout(resolve, 220)),
      ]);
      const panel = dialog();
      const style = getComputedStyle(panel);
      const tab = one('[role="tab"][aria-selected="true"]', panel);
      const rect = tab?.getBoundingClientRect();
      const paintSamples = rect?.width && rect.height
        ? [{ x: rect.x + rect.width / 2, y: rect.bottom - 1,
          color: getComputedStyle(tab).borderBottomColor }]
        : [];
      Object.assign(state, { screenX, screenY, ratio: devicePixelRatio, paintSamples,
        dialogs: [{ open: true, opacity: style.opacity, background: style.backgroundColor,
          rect: panel.getBoundingClientRect().toJSON() }] });
    }
    return send(`ux_${name}`, state);
  }
  const observe = (state) => one(`[data-import-state="${state}"]`);
  const favorite = () => one('.external-import-destination input[type=checkbox]', dialog());
  const textarea = () => one('[data-import-text]', dialog());
  const setText = (text) => {
    textarea().value = text;
    textarea().dispatchEvent(new InputEvent('input', { bubbles: true }));
  };
  const proceed = () => button('提交导入…');
  const hittable = (node) => {
    if (!node) return false;
    const rect = node.getBoundingClientRect();
    return [rect.y + rect.height / 2, rect.bottom - 2].every((y) =>
      node.contains(document.elementFromPoint(rect.x + rect.width / 2, y)));
  };
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
      assert(observe('unknown').textContent.includes('我喜欢的音乐') && !favorite(),
        'unknown favorites receipt survives reopen without enabling either destination');
      await preview('https://www.kuwo.cn/synthetic/another');
      assert(!button('授权并导入…') && !button('提交导入…'), 'reopen forbids resubmission');
      click(button('文字导入'));
      assert(textarea().value === '', 'text is not retained after process reopen');
      setText('不会重复提交的合成文字');
      await wait(() => textarea().value.length > 0, 'reopen text input');
      assert(!button('授权并导入…') && !proceed(), 'unknown receipt blocks both lanes');
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
    assert(favorite() && !favorite().checked, 'new dialog defaults to creating a playlist');
    click(button('提交导入…'));
    await wait(() => observe('confirm') && document.activeElement === button('取消提交'), 'safe confirm focus');
    assert(observe('confirm').dataset.importDestination === 'new_playlist' &&
      observe('confirm').textContent.includes('将尝试创建'), 'default confirmation names its destination');
    await snap('confirm');
    click(button('取消提交'));
    await wait(() => !observe('confirm'), 'confirmation cancelled');
    await snap('cancelled');

    click(button('提交导入…'));
    await wait(() => observe('confirm') && !favorite().disabled, 'new confirmation');
    click(favorite());
    await wait(() => !observe('confirm') && favorite().checked && proceed() && !proceed().disabled,
      'switching destination after final input revokes the existing confirmation');
    await snap('destination_changed');
    click(proceed());
    await wait(() => observe('confirm')?.dataset.importDestination === 'favorite_songs' &&
      document.activeElement === button('取消提交') && hittable(button('确认提交导入')),
      'favorites confirmation binds the new destination, defaults to cancel focus, and stays visible');
    assert(observe('confirm').textContent.includes('我喜欢的音乐') &&
      !observe('confirm').textContent.includes('将尝试创建'), 'favorites confirmation explains the library change');
    await snap('favorite_confirm');
    click(button('确认提交导入'));
    await wait(() => observe('accepted'), 'receipt says accepted only');
    assert(observe('accepted').textContent.includes('我喜欢的音乐'), 'receipt names favorites');
    assert(!dialog().textContent.includes('fixture-task-id'), 'native task id is private');
    await snap('accepted');
    click(button('关闭'));
    await wait(() => !dialog(), 'close import after accepted receipt');
    await open();
    await wait(() => favorite() && !favorite().checked, 'reopening resets the destination selection');
    const linksTab = button('链接导入');
    linksTab.focus();
    linksTab.dispatchEvent(new KeyboardEvent('keydown', {key: 'ArrowRight', bubbles: true}));
    await wait(() => button('文字导入')?.getAttribute('aria-selected') === 'true' &&
      document.activeElement === button('文字导入'), 'keyboard switches and focuses the text tab');
    setText('歌'.repeat(6000));
    await wait(() => textarea()?.getAttribute('aria-invalid') === 'true' && proceed()?.disabled, 'bounded text paste');
    const originalText = '  合成歌手 A&B=#?+ 50%\n合成曲目 🎵  ';
    setText(originalText);
    await wait(() => proceed() && !proceed().disabled, 'text ready without preflight');
    click(proceed());
    await wait(() => observe('confirm')?.dataset.importSource === 'text', 'typed text confirmation');
    assert(!observe('confirm').textContent.includes('合成歌手'), 'raw text is not echoed by receipt');
    setText(originalText + '\n编辑后的合成文字');
    await wait(() => !observe('confirm') && proceed() && !proceed().disabled, 'editing revokes text confirmation');
    await snap('text_edited');

    click(proceed());
    await wait(() => observe('confirm')?.dataset.importSource === 'text' &&
      document.activeElement === button('取消提交') && hittable(button('确认提交导入')),
      'text confirmation defaults to cancel focus and is not covered by the footer');
    await snap('text_confirm');
    click(button('确认提交导入'));
    await wait(() => observe('accepted') && textarea().value === '' && !observe('confirm'), 'text accepted and draft cleared');
    assert(!dialog().textContent.includes('fixture-text-task'), 'text task id remains native');
    await snap('text_accepted');

    setText('未知回执的合成文字');
    await wait(() => proceed() && !proceed().disabled, 'new text input');
    click(favorite());
    click(proceed());
    await wait(() => observe('confirm')?.dataset.importSource === 'text' &&
      observe('confirm')?.dataset.importDestination === 'favorite_songs',
      'next explicit text confirmation binds favorites too');
    await snap('text_favorite_confirm');
    click(button('确认提交导入'));
    await wait(() => observe('unknown'), 'missing task id is unknown');
    assert(observe('unknown').textContent.includes('我喜欢的音乐'), 'unknown receipt names submitted destination');
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
