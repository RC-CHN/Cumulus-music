(async () => {
  if (window.__editStarted) return;
  window.__editStarted = true;
  const sleep = ms => new Promise(r => setTimeout(r, ms));
  const el = s => document.querySelector(s);
  const all = s => [...document.querySelectorAll(s)];
  const button = (value, scope = document) => [...scope.querySelectorAll('button')].find(n => n.textContent.trim() === value);
  const report = (stage, detail = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {event: 'comment-edit-proof', payload: {stage, ...detail}});
  const wait = async (test, label, ms = 30000) => {
    const until = Date.now() + ms;
    while (Date.now() < until) {const v = await test(); if (v) return v; await sleep(80);}
    throw Error(label);
  };
  const click = async n => {
    if (!n) throw Error('missing_control');
    await wait(() => n.isConnected && !n.disabled, 'control_disabled');
    n.scrollIntoView({block: 'nearest'}); n.click(); await sleep(100);
  };
  const fill = value => {
    const n = el('[aria-label="评论内容"]');
    n.value = value; n.dispatchEvent(new Event('input', {bubbles: true}));
  };
  const draft = () => el('[aria-label="评论内容"]')?.value;
  const card = id => el(`[data-group="page"] [data-comment-id="${id}"]`);
  const current = id => el(`[data-group="current"] [data-comment-id="${id}"]`);
  const pending = () => el('.comment-write-pending');
  const ready = () => el('.comment-compose-footer button') && !el('[aria-label="评论内容"]').disabled;
  const stage = async (name, detail = {}) => {
    await sleep(350);
    const dialog = el('dialog'), rect = dialog?.getBoundingClientRect();
    const overflow = document.documentElement.scrollWidth > innerWidth || !!dialog && (dialog.scrollWidth > dialog.clientWidth + 1 || rect.top < 0 || rect.bottom > innerHeight + 1);
    if (overflow) throw Error('layout_overflow');
    window.__editAck = '';
    await report(name, { ...detail, overflow: false, width: innerWidth, height: innerHeight,
      draft: draft(), pendingPhases: all('.comment-write-pending [data-write-phase]').map(n => n.dataset.writePhase),
      currentIds: all('[data-group="current"] .comment-card').map(n => n.dataset.commentId)});
    await wait(() => window.__editAck === name, 'native_ack'); await sleep(850);
  };
  const publish = async value => {fill(value); await sleep(80); await click(el('.comment-compose-footer button'));};
  const confirmed = async id => wait(() => ready() && !pending() && current(id), 'confirmed_' + id);
  let step = 'boot';
  try {
    await wait(() => el('.track-row .track-more'), 'daily_bootstrap');
    await click(el('.track-row .track-more'));
    await click(await wait(() => button('查看评论'), 'track_menu'));
    await wait(() => card('102') && (ready() || pending()), 'comments_ready');
    if (button('删除', card('102')) || !button('删除', card('101'))) throw Error('ownership_controls');
    if (window.__editProof.reopen) {
      step = 'reopen_pending';
      await wait(() => pending()?.textContent.includes('丢失回包后保留责任'), 'pending_restored');
      if (!el('.comment-compose-footer button').disabled) throw Error('pending_allows_new_publish');
      await stage('edit_reopen_pending');
      await click(button('核对提交结果', pending()));
      await wait(() => button('核对提交结果', pending()) && !button('核对提交结果', pending()).disabled, 'read_only_reconcile');
      await stage('edit_reopen_unresolved');
      await click(button('停止核对', pending()));
      await wait(() => el('dialog[open]'), 'abandon_confirmation');
      await stage('edit_reopen_abandon_confirm');
      await click(button('取消', el('dialog')));
      if (!pending()) throw Error('cancel_abandon_lost_responsibility');
      await click(button('停止核对', pending()));
      await wait(() => el('dialog[open]'), 'abandon_confirmation_again');
      await click(button('停止核对', el('dialog')));
      await wait(() => !pending() && ready(), 'abandoned');
      await stage('edit_reopen_abandoned');
      step = 'new_intent';
      await publish('重开后明确提交的新内容'); await confirmed('9003');
      await stage('edit_reopen_published');
      await click(button('删除', current('9003')));
      await wait(() => el('dialog[open]'), 'delete_reopen');
      await click(button('确认删除', el('dialog')));
      await wait(() => ready() && !pending() && !current('9003'), 'reopen_deleted');
      await stage('edit_reopen_deleted');
    } else {
      step = 'text_validation';
      if (!el('.comment-compose-footer button').disabled) throw Error('empty_submission_enabled');
      await stage('edit_composer');
      fill('🎵'.repeat(500) + 'a'); await sleep(80);
      if (!el('.comment-compose-footer button').disabled || !el('.overlength')?.textContent.includes('1001')) throw Error('utf16_bound');
      await stage('edit_unicode_limit', {utf16Length: draft().length});
      fill('  \n\t'); await sleep(80);
      if (!el('.comment-compose-footer button').disabled) throw Error('whitespace_submission_enabled');
      fill('');
      step = 'floor';
      await click(button('查看回复', card('102')));
      await wait(() => all('.comment-floor-panel .comment-card').length === 21, 'floor_first');
      await stage('edit_floor_first', {floorIds: all('.comment-floor-panel .comment-card').map(n => n.dataset.commentId)});
      await click(button('下一页回复'));
      await wait(() => all('.comment-floor-panel .comment-card').length === 2 && button('下一页回复').disabled, 'floor_second');
      await stage('edit_floor_second', {floorIds: all('.comment-floor-panel .comment-card').map(n => n.dataset.commentId)});
      await click(button('上一页回复'));
      await wait(() => all('.comment-floor-panel .comment-card').length === 21, 'floor_back');
      await click(button('收起回复'));
      step = 'publish';
      await publish(' 第一条评论 🎵\n保留原始空白 '); await confirmed('9000');
      if (draft() !== '') throw Error('confirmed_draft_not_cleared');
      await stage('edit_published');
      step = 'reply';
      await click(button('回复', card('102')));
      await wait(() => el('.comment-reply-draft'), 'reply_draft');
      await publish('回复：也想去看看海。'); await confirmed('9001');
      if (!current('9001').querySelector('.comment-quote')?.textContent.includes('夏天的海风')) throw Error('reply_quote_missing');
      await stage('edit_replied');
      step = 'delete';
      await click(button('删除', current('9001')));
      await wait(() => el('dialog[open]'), 'delete_confirm');
      await stage('edit_delete_confirm');
      await click(button('取消', el('dialog')));
      if (!current('9001')) throw Error('cancel_deleted_comment');
      await click(button('删除', current('9001')));
      await wait(() => el('dialog[open]'), 'delete_confirm_again');
      await click(button('确认删除', el('dialog')));
      await wait(() => ready() && !pending() && !current('9001'), 'delete_verified');
      await stage('edit_deleted');
      step = 'rejections';
      await publish('需要绑定手机号');
      await wait(() => ready() && el('.comment-write-panel [role="alert"]')?.textContent.includes('绑定手机号'), 'phone_rejection');
      if (draft() !== '需要绑定手机号') throw Error('rejected_draft_lost');
      await stage('edit_rejected_phone');
      await click(button('回复', card('102')));
      await publish('目标已消失');
      await wait(() => ready() && el('.comment-write-panel [role="alert"]'), 'reply_rejection');
      if (draft() !== '目标已消失' || !el('.comment-reply-draft')) throw Error('rejected_reply_lost');
      await stage('edit_rejected_reply');
      await click(button('取消回复'));
      step = 'response_loss';
      await publish('丢失回包后保留责任');
      await wait(() => pending()?.querySelector('[data-write-phase="uncertain"]') && !button('核对提交结果', pending()).disabled, 'uncertain_write');
      if (!el('.comment-compose-footer button').disabled) throw Error('uncertain_duplicate_allowed');
      await click(button('刷新操作记录'));
      await stage('edit_uncertain');
    }
    await report('complete', {ok: true, reopened: window.__editProof.reopen, publicWrites: 0});
  } catch (error) {
    await report('failed', {ok: false, step, error: error instanceof Error ? error.message : 'unknown',
      visibleError: all('[role="alert"]').map(n => n.textContent), message: el('.comment-write-message')?.textContent});
  }
})();
