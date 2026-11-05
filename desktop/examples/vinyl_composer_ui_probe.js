// Opt-in write scenario for the existing vinyl WebView proof.
window.__vinylComposerRun = async function ({
  epoch, first, player, pause, comment, capture, tab, scroller,
  rows, footer, pane, wait, click, invoke, assert, one, all,
}) {
  const textButton = (text, root = document) =>
    [...root.querySelectorAll('button')].find((node) => node.textContent.trim() === text);
  const dialog = () => one('.vinyl-comment-dialog[open]');
  const field = () => dialog()?.querySelector('[aria-label="评论内容"]');
  const submit = () => dialog()?.querySelector('.comment-compose-footer button.primary');
  const opener = () => textButton('发布评论', pane() ?? document);
  const currentCount = () => Number(one('.vinyl-comments-heading small')?.textContent);
  const latest = () => rows('latest');
  const recent = (id) => latest().find((row) => row.querySelector('p')?.textContent === '合成评论 ' + id);
  const operations = (playId) => invoke('player_vinyl_write_operations', { epoch, playId });
  const raw = '  零评论发布 🎵\n保留空白  ';
  const reply = '  回复原评论 🎧 ';
  const lost = '合成回复已送达，回包丢失';
  const entry = () => one('.vinyl-comment-publish button');
  const recovery = () => textButton('在独立评论页核对');
  function fill(text) {
    field().value = text;
    field().dispatchEvent(new Event('input', { bubbles: true }));
  }
  async function open(node, label) {
    click(node);
    return wait(() => dialog() && field() && submit() && submit().textContent.trim() === label,
      'compose ' + label);
  }
  async function playSecond() {
    click(footer('下一首'));
    await wait(async () => {
      const value = await player();
      return value.state === 'playing' && value.track?.id === '2' && value.playId !== first.playId;
    }, 'ordinary second song');
    return pause();
  }
  const second = await playSecond();
  await wait(() => comment()?.getAttribute('aria-pressed') === 'false', 'verified t1 footer available');
  click(comment());
  await wait(() => tab() === 'comments' && scroller()?.textContent.includes('暂无评论') &&
    currentCount() === 0 && entry() && opener(), 'known-zero comment thread with a publish entry');
  await capture('composer_zero', second, { count: currentCount() });

  await open(opener(), '发布');
  assert(dialog().querySelector('h2')?.textContent.trim().startsWith('歌曲：'), 'song title in composer');
  fill('合成草稿，取消时不能发送');
  await wait(() => !submit().disabled, 'cancelled draft is otherwise valid');
  click(dialog().querySelector('[aria-label="关闭评论编辑"]'));
  await wait(() => !dialog() && document.activeElement === opener(),
    'cancel dismisses composer and restores the exact opener');
  assert((await operations(second.playId)).value.length === 0, 'cancel creates no native write intent');
  await capture('cancelled', second, { count: currentCount() });

  await open(opener(), '发布');
  fill('  \n\t');
  await wait(() => submit().disabled, 'whitespace-only draft cannot publish');
  fill('🎵'.repeat(500));
  await wait(() => !submit().disabled, 'exactly 1000 UTF-16 code units are allowed');
  fill('🎵'.repeat(500) + 'a');
  await wait(() => submit().disabled, '1001 UTF-16 code units are rejected');
  await capture('utf16_rejected', second, { units: field().value.length, disabled: submit().disabled });
  fill(raw);
  await wait(() => !submit().disabled && field().value === raw, 'raw draft ready for explicit publish');
  click(submit());
  await wait(() => !dialog() && currentCount() === 1 && latest()[0]?.textContent.includes('零评论发布'),
    'confirmed zero-count publish prepends and increments');
  const publish = (await operations(second.playId)).value;
  assert(publish.length === 1 && publish[0].phase === 'confirmed' &&
    publish[0].observed?.id === '9001' && publish[0].observed?.content === raw,
    'native confirmed receipt matches exact raw content');
  await capture('published', second, { count: currentCount(), confirmed: publish[0].observed.id });

  await open(opener(), '发布');
  fill('旧歌曲草稿不得提交到新的歌曲');
  await wait(() => !submit().disabled, 'stale dialog has a valid draft before the native transport changes');
  const oldDialog = dialog();
  const oldSubmit = submit();
  await capture('stale_open', second, { count: currentCount() });
  // The footer is inert during a modal. A native media transport can still
  // change tracks, so use the existing typed player action at that boundary.
  await invoke('player_action', { epoch, action: { kind: 'previous' } });
  await wait(async () => {
    const value = await player();
    return value.state === 'playing' && value.track?.id === '1' && value.playId !== first.playId;
  }, 'native previous while a dialog is open');
  await wait(() => !dialog() && !oldDialog.isConnected && !oldSubmit.isConnected,
    'old modal is invalidated when the native song changes');
  const current = await pause();
  assert(await operations(second.playId).then(() => false, () => true), 'old native play ID rejected');
  await wait(() => tab() === 'comments' && currentCount() === 63 && recent('1'),
    'active first song comments replace prior thread');
  assert((await operations(current.playId)).value.length === 0, 'the stale draft cannot become a new thread intent');
  await capture('stale_closed', current, { count: currentCount() });

  const replyTrigger = () => recent('1')?.querySelector('.vinyl-comment-reply');
  await open(replyTrigger(), '回复');
  assert(field().placeholder === '回复 合成作者: ', 'reply targets the selected displayed author');
  fill(reply);
  await wait(() => !submit().disabled, 'valid reply draft');
  await capture('reply_target', current, { placeholder: field().placeholder });
  click(submit());
  await wait(() => !dialog() && currentCount() === 64 && latest()[0]?.textContent.includes('回复原评论'),
    'verified reply prepends and increments the same song count');
  const acknowledged = (await operations(current.playId)).value;
  assert(acknowledged.length === 1 && acknowledged[0].phase === 'confirmed' &&
    acknowledged[0].observed?.id === '9002' && acknowledged[0].observed?.replies?.[0]?.id === '1',
    'confirmed reply receipt records the exact original comment');
  await capture('reply_confirmed', current, { count: currentCount(), confirmed: acknowledged[0].observed.id });

  await open(replyTrigger(), '回复');
  fill(lost);
  await wait(() => !submit().disabled, 'valid reply to simulate response loss');
  click(submit());
  // Prepared/Sending can render the recovery entry before the write reply.
  // Wait for the actual enabled action, after the pending record is refreshed.
  await wait(() => recovery() && !recovery().disabled && (!submit() || submit().disabled),
    'settled uncertain result offers recovery instead of resend');
  const pending = (await operations(current.playId)).value.find((item) => item.phase === 'uncertain');
  assert(pending && !pending.acknowledged && !pending.observed,
    'lost reply preserves the pending record without acknowledgment or observation');
  assert(currentCount() === 64, 'lost reply must not alter the known count (was ' + currentCount() + ')');
  assert(!latest()[0]?.textContent.includes(lost), 'lost reply must not prepend an unconfirmed row');
  await capture('reply_uncertain', current, { count: currentCount(), pending: pending.id });
  click(recovery());
  await wait(() => !pane() && one('.comments-page .comment-write-pending [data-write-phase="uncertain"]'),
    'independent page loads the same outstanding write');
  const panel = one('.comments-page .comment-write-pending');
  const stored = (await invoke('comments_write_operations', {
    epoch, resource: { kind: 'track', id: '1' },
  })).value;
  assert(stored.some((row) => row.id === pending.id && row.phase === 'uncertain') &&
    panel.querySelector('[data-write-phase="uncertain"]')?.textContent.includes(lost),
  'independent page displays the same saved responsibility');
  await wait(() => textButton('核对提交结果', panel) && !textButton('核对提交结果', panel).disabled,
    'independent page offers explicit read-only reconciliation');
  await capture('independent_recovery', current, { pending: pending.id });
  click(textButton('核对提交结果', panel));
  await wait(() => {
    const button = textButton('核对提交结果', panel);
    const error = one('.comments-page .comment-write-panel [role="alert"]');
    return button && !button.disabled && error?.textContent.includes('不要重复提交');
  }, 'reconciliation returns without a resend');
  assert(all('.comments-page .comment-write-pending [data-write-phase="uncertain"]').length === 1,
    'no receipt ID means independent reconciliation remains uncertain');
  await capture('independent_unresolved', current, { pending: pending.id });
};
