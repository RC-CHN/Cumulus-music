// Production Vue/IPC and clipboard, isolated library mutations and null audio.
(async () => {
  const one = (s) => document.querySelector(s), all = (s) => [...document.querySelectorAll(s)];
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const assert = (v, m) => { if (!v) throw Error(m); };
  const wait = async (fn, m) => { for (let i = 0; i < 240; i++) { if (fn()) return; await sleep(50); } throw Error(m); };
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const emit = (stage, data = {}) => invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...data } });
  const button = (label, root = document) => [...root.querySelectorAll('button')].find((n) => n.getAttribute('aria-label') === label || n.textContent.trim() === label);
  const click = (node) => { assert(node && !node.disabled, 'enabled control'); node.scrollIntoView({ block: 'nearest' }); node.focus({ preventScroll: true }); node.click(); };
  const key = (node, value, extra = {}) => node.dispatchEvent(new KeyboardEvent('keydown', { key: value, bubbles: true, cancelable: true, ...extra }));
  const menu = () => one('.action-menu');
  let step = 'startup', count = 0, failCopy = false, holdCopy = false, copyCalls = 0;
  const originalFetch = window.fetch;
  window.fetch = async function(url, options) {
    if (String(url).endsWith('/copy_text')) {
      copyCalls++;
      if (holdCopy) await sleep(900);
      if (failCopy) return new Response(JSON.stringify('synthetic clipboard failure'), { headers: { 'Content-Type': 'application/json', 'Tauri-Response': 'error' } });
    }
    return originalFetch.call(window, url, options);
  };
  const fixture = async (mode) => {
    window.__libraryFixtureAck = null; await emit('library-fixture', { mode });
    await wait(() => window.__libraryFixtureAck, 'fixture ack'); return window.__libraryFixtureAck;
  };
  async function clipboard() {
    window.__searchClipboardAck = null;
    await emit('search-clipboard-check', { expected: 'https://music.163.com/#/playlist?id=2' });
    await wait(() => window.__searchClipboardAck !== null, 'clipboard check');
    assert(window.__searchClipboardAck, 'actual native clipboard value');
  }
  async function capture(label) {
    step = label; await sleep(200);
    assert(document.documentElement.scrollWidth <= innerWidth, 'document containment');
    for (const n of all('.action-menu, dialog[open]')) {
      const r = n.getBoundingClientRect();
      assert(r.left >= 0 && r.top >= 0 && r.right <= innerWidth + 1 && r.bottom <= innerHeight + 1, 'overlay containment');
    }
    for (const n of all('dialog[open] footer button, dialog[open] header button')) {
      const r = n.getBoundingClientRect();
      const hit = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
      assert(hit && (n.contains(hit) || n === hit), 'dialog controls reachable');
    }
    await emit(`ux_${String(++count).padStart(2,'0')}_${label}`, { width: innerWidth, height: innerHeight, theme: document.documentElement.dataset.theme, items: menu() ? [...menu().querySelectorAll('button')].map(n => ({ text: n.textContent.trim(), disabled: n.disabled })) : [] });
    await sleep(420);
  }
  async function library() {
    click(button('我的歌单')); await wait(() => one('.playlist-library-page') && !one('.page-enter-active,.page-leave-active'), 'library');
  }
  async function open(id, context = false) {
    const card = one(`[data-library-open="${id}"]`), trigger = context ? card : card.closest('article').querySelector('.menu-trigger');
    trigger.scrollIntoView({ block: 'center' }); await sleep(80); trigger.focus({ preventScroll: true });
    if (context) key(trigger, 'F10', { shiftKey: true }); else click(trigger);
    await wait(() => menu()?.style.visibility === 'visible', 'resource menu'); await sleep(140);
    return trigger;
  }
  async function choose(label) { click(button(label, menu())); await wait(() => !menu(), 'menu selection'); }
  async function escape(trigger) { key(menu(), 'Escape'); await wait(() => !menu(), 'escape'); assert(document.activeElement === trigger, 'menu focus restored'); }
  async function closeDialog(trigger, label = '取消') {
    click(button(label, one('dialog[open]'))); await wait(() => !one('dialog[open]'), 'dialog closes'); await sleep(180);
    assert(document.activeElement === trigger, 'dialog focus restored');
  }
  async function mutation(id, label) {
    const trigger = await open(id); await choose(label); await wait(() => one('.playlist-mutation-dialog'), 'confirmation');
    await wait(() => one('.playlist-mutation-dialog').getAttribute('aria-busy') === 'false', 'confirmation ready');
    assert(document.activeElement === button('取消', one('.playlist-mutation-dialog')), 'safe initial focus');
    return trigger;
  }
  try {
    await wait(() => one('.track-more'), 'daily'); await library();
    for (const [w,h,t] of [[1120,760,'light'],[1120,760,'dark'],[760,540,'light'],[760,540,'dark']]) {
      step = `setup_${w}_${t}`;
      await emit('resize', { width:w, height:h }); await wait(() => innerWidth === w && innerHeight === h, 'resize');
      click(button('外观主题')); await wait(() => menu(), 'theme'); click(button(t === 'dark' ? '深色' : '浅色')); await wait(() => !menu(), 'theme closes');
      const suffix = `${w}_${t}`;
      let trigger = await open(2, true);
      assert(all('.action-menu button').map(n => n.textContent.trim()).join('|') === '查看歌单|播放|下一首播放|分享…|复制链接|下载全部|编辑歌单信息|删除歌单|上移歌单|下移歌单', 'owned menu order');
      key(menu(),'End'); assert(document.activeElement === button('下移歌单'), 'End focus');
      key(menu(),'ArrowUp'); assert(document.activeElement === button('删除歌单'), 'skips disabled up and separator');
      await capture(`owned_${suffix}`); await escape(trigger);
      trigger = await open(6); assert(button('分享…').disabled && button('复制链接').disabled, 'private sharing disabled');
      await capture(`private_${suffix}`); await escape(trigger);
      trigger = await open(5); assert(button('播放').disabled && button('分享…').disabled && button('下载全部').disabled, 'empty disabled');
      await capture(`empty_${suffix}`); await escape(trigger);
      trigger = await open(1); assert(!button('删除歌单') && !button('编辑歌单信息') && !button('上移歌单'), 'favorite protected');
      await capture(`favorite_${suffix}`); await escape(trigger);
      trigger = await open(2); await choose('分享…'); await wait(() => one('.track-share-dialog'), 'share');
      assert(one('.track-share-dialog h2').textContent === '分享歌单', 'resource share heading');
      click(button('复制链接', one('.track-share-dialog'))); await wait(() => one('.track-share-status')?.textContent.includes('已复制'), 'share copied'); await clipboard();
      await capture(`share_${suffix}`); await closeDialog(trigger, '关闭分享');
      trigger = await mutation(2,'删除歌单'); await capture(`delete_${suffix}`); await closeDialog(trigger);
      trigger = await mutation(61,'取消收藏'); assert(one('.playlist-mutation-body').textContent.includes('之后可重新收藏'), 'unfollow consequence'); await capture(`unsubscribe_${suffix}`); await closeDialog(trigger);
      trigger = await open(2); await choose('查看歌单'); await wait(() => one('.playlist-hero'), 'detail');
      click(button('更多歌单操作')); await wait(() => menu(), 'detail menu');
      assert(all('.action-menu button').map(n=>n.textContent.trim()).join('|') === '分享…|批量操作|添加全部至播放列表', 'detail avoids duplicated or unconnected actions');
      await capture(`detail_${suffix}`); await escape(button('更多歌单操作'));
      await library();
    }
    step = 'sidebar menu';
    one('[data-sidebar-group="created"] header').scrollIntoView({ block: 'start' });
    await wait(() => one('[data-sidebar-id="2"]'), 'virtual sidebar row');
    let side = one('[data-sidebar-id="2"]'); side.focus(); key(side,'F10',{shiftKey:true}); await wait(() => menu()?.style.visibility === 'visible','sidebar keyboard menu');
    await capture('sidebar_menu'); await choose('删除歌单'); await wait(() => one('.playlist-mutation-dialog')?.getAttribute('aria-busy') === 'false','sidebar confirm');
    await capture('sidebar_confirm'); await closeDialog(side);
    const point = side.getBoundingClientRect(); side.dispatchEvent(new MouseEvent('contextmenu',{ bubbles:true, cancelable:true, button:2, clientX:point.right-4, clientY:point.bottom-4 }));
    await wait(() => menu()?.style.visibility === 'visible','sidebar pointer menu'); await choose('分享…'); await wait(() => one('.track-share-dialog'),'sidebar share'); await closeDialog(side,'关闭分享');
    step = 'copy failure';
    let trigger = await open(2); failCopy = true; await choose('复制链接'); await wait(() => one('.playlist-action-feedback.failed'), 'copy failed'); await capture('copy_failed');
    failCopy = false; holdCopy = true; const copies = copyCalls; const retry = button('重试', one('.playlist-action-feedback')); click(retry); click(retry);
    await capture('copy_retry'); assert(copyCalls === copies + 1, 'one retry');
    await wait(() => one('.playlist-action-feedback')?.textContent.includes('已复制'), 'copy recovery'); await clipboard(); assert(document.activeElement === trigger, 'copy retry focus'); await capture('copy_recovered'); holdCopy = false;
    step = 'download full list';
    await fixture('hold_detail'); trigger = await open(7); await choose('下载全部'); await wait(() => one('.playlist-download-read'), 'read pending'); await capture('download_waiting');
    await closeDialog(trigger); await fixture('success'); await sleep(150); assert(!one('dialog[open]'), 'cancel ignores late pages');
    await fixture('fail_detail'); trigger = await open(7); await choose('下载全部'); await wait(() => one('.playlist-download-read [role="alert"]'), 'download read error'); await capture('download_failed');
    await fixture('success'); click(button('重新读取', one('.playlist-download-read'))); await wait(() => one('.download-dialog h2')?.textContent === '下载 240 首歌曲', 'all pages in download dialog');
    await capture('download_all'); await closeDialog(trigger, '关闭下载对话框');
    step = 'delete detail recovery';
    await fixture('fail_detail'); trigger = await mutation(9,'删除歌单');
    assert(button('删除歌单',one('.playlist-mutation-dialog')).getAttribute('aria-disabled') === 'true', 'cannot delete unread detail'); await capture('delete_read_failed');
    await fixture('hold_detail'); const readRetry = button('重新读取',one('.playlist-mutation-dialog')); click(readRetry); click(readRetry); await capture('delete_read_waiting');
    assert(document.activeElement === readRetry && readRetry.isConnected, 'retry control stays mounted');
    assert(Number(getComputedStyle(button('删除歌单',one('.playlist-mutation-dialog'))).opacity) < 1, 'unavailable confirmation visibly disabled');
    await fixture('success'); await wait(() => !one('.playlist-mutation-dialog [role="alert"]') && one('.playlist-mutation-dialog').getAttribute('aria-busy') === 'false', 'detail recovered');
    assert(document.activeElement === button('取消',one('.playlist-mutation-dialog')), 'retry focus recovery'); await capture('delete_read_recovered'); await closeDialog(trigger);
    step = 'rejected delete';
    await fixture('reject'); trigger = await mutation(3,'删除歌单'); click(button('删除歌单',one('.playlist-mutation-dialog')));
    await wait(() => one('.playlist-operation-message') && one('.playlist-mutation-dialog')?.getAttribute('aria-busy') === 'false', 'reject feedback'); await capture('delete_rejected');
    await closeDialog(trigger); assert(one('[data-library-open="3"]'), 'reject keeps resource');
    step = 'write wait';
    await fixture('hold_write'); trigger = await mutation(4,'删除歌单'); click(button('删除歌单',one('.playlist-mutation-dialog'))); await wait(() => button('正在处理…'), 'write pending');
    assert(button('取消',one('.playlist-mutation-dialog')).disabled, 'busy cancel disabled'); await capture('delete_waiting');
    await fixture('success'); await wait(() => !one('.playlist-mutation-dialog'), 'delete succeeded'); await sleep(200); assert(!one('[data-library-open="4"]'), 'removed resource'); assert(document.activeElement !== document.body, 'removed trigger stable fallback'); await capture('delete_complete');
    step = 'uncertain delete';
    await fixture('uncertain'); trigger = await mutation(8,'删除歌单'); click(button('删除歌单',one('.playlist-mutation-dialog'))); await wait(() => button('核对结果'), 'unknown result recoverable'); await capture('delete_uncertain');
    await closeDialog(trigger,'稍后处理'); trigger = await open(8); await choose('删除歌单'); await wait(() => button('核对结果'), 'pending restored on reopen'); assert(!button('删除歌单',one('.playlist-mutation-dialog')), 'no repeat write'); await capture('delete_reopened');
    const writes = (await fixture('success')).writes; click(button('核对结果')); await wait(() => !one('.playlist-mutation-dialog'), 'reconciled'); assert((await fixture('success')).writes === writes, 'readback does not repeat mutation'); await capture('delete_reconciled');
    step = 'unsubscribe';
    await mutation(61,'取消收藏'); click(button('取消收藏',one('.playlist-mutation-dialog'))); await wait(() => !one('.playlist-mutation-dialog'), 'unfollow'); assert(!one('[data-library-open="61"]'), 'unfollow removes local card'); await capture('unsubscribed');
    step = 'play';
    trigger = await open(2); await choose('播放'); await wait(() => one('[data-library-open="2"]').closest('article').querySelector('.library-card-play')?.getAttribute('aria-label').startsWith('暂停'), 'native playlist playback'); await capture('playing');
    click(one('[data-library-open="2"]').closest('article').querySelector('.library-card-play')); await wait(() => one('[data-library-open="2"]').closest('article').querySelector('.library-card-play')?.getAttribute('aria-label').startsWith('播放'), 'paused'); await capture('paused');
    await emit('complete',{ ok:true, playlistMenuUi:true, stageCount:count, realMusicRequests:0, realAccountWrites:0, clipboardVerified:true, mutationRecoveryVerified:true, fullDownloadVerified:true });
  } catch(error) { await emit('failed',{ step,message:String(error),stack:error.stack,stageCount:count }); }
})();
