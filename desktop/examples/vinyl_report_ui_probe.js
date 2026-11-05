// A closed standard-vinyl report fixture; the production renderer/IPC handles every write.
window.__vinylReportRun = async function ({
  epoch, first, comment, capture, tab, scroller, wait, click, invoke, assert, one, all,
}) {
  const thread = { kind: 'track', id: '1' };
  const row = (group, id) => one(`[data-vinyl-comment-group="${group}"] [data-vinyl-comment-id="${id}"]`);
  const report = (group, id) => row(group, id)?.querySelector('.vinyl-comment-report');
  const dialog = () => one('[data-moderation-report-dialog][open]');
  const dialogButton = (text) => [...(dialog()?.querySelectorAll('button') ?? [])]
    .find((b) => b.textContent.trim() === text);
  const reasons = () => one('[aria-label="举报原因"]');
  const outcome = () => dialog()?.querySelector('[data-moderation-report-outcome]')?.textContent.trim();
  const inspect = (id) => invoke('comment_report', { epoch, resource: thread, id, command: { kind: 'inspect' } });
  const other = '其他原因';
  const lost = '  合成原因原样发送  ';
  const choose = async (label) => {
    const button = await wait(() => [...(reasons()?.querySelectorAll('button') ?? [])]
      .find((b) => b.textContent.trim().startsWith(label)), 'official report reason: ' + label);
    click(button);
    await wait(dialog, 'native report dialog');
  };
  function setDescription(value) {
    const field = dialog()?.querySelector('textarea');
    assert(field, 'custom reason editor');
    field.value = value;
    field.dispatchEvent(new Event('input', { bubbles: true }));
  }
  async function close(label) {
    await wait(() => dialogButton(label) && !dialogButton(label).disabled, 'report dialog can close');
    click(dialogButton(label));
    await wait(() => !dialog(), 'report dialog closed');
  }
  await wait(() => comment()?.getAttribute('aria-pressed') === 'false', 'signed t1 enters standard comments');
  click(comment());
  await wait(() => tab() === 'comments' && report('hot', '1') && report('latest', '3'), 'both embedded groups can report');
  assert(!report('hot', '2') && !report('latest', '2'), 'own report control is absent');
  assert((await inspect('1')).value.status === 'ready', 'no existing report');
  scroller().scrollTop = 35;
  // The popover dismisses on scroll. Let the browser deliver the deliberate
  // scroll event before exercising the report action at this position.
  await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const anchor = scroller().scrollTop;
  const stage = async (name, extra = {}) => {
    assert(tab() === 'comments' && Math.abs(scroller().scrollTop - anchor) < 1,
      'report overlays preserve the embedded page and scroll');
    await capture(name, first, { scroll: anchor, ...extra });
  };

  click(report('hot', '1'));
  await choose(other);
  await wait(() => dialogButton('确认')?.disabled, 'blank other reason is disabled');
  setDescription('不发送的合成草稿');
  await wait(() => dialogButton('确认') && !dialogButton('确认').disabled,
    'valid custom text can be confirmed');
  await close('取消');
  assert((await inspect('1')).value.status === 'ready', 'cancelling custom reason did not send');
  await stage('report_cancelled');

  row('hot', '1').dispatchEvent(new MouseEvent('contextmenu', {
    bubbles: true, cancelable: true, button: 2,
  }));
  const context = await wait(() => one('[aria-label="评论操作"] button[role="menuitem"]'),
    'comment context menu has Report');
  click(context);
  await choose('段子或者无意义的评论');
  await wait(() => outcome() === '举报成功', 'preset sends once without another confirmation');
  assert((await inspect('1')).value.status === 'accepted', 'independent route reads the same receipt');
  await stage('report_preset_accepted', { outcome: outcome() });
  await close('关闭');

  click(report('latest', '3'));
  await choose(other);
  setDescription(lost);
  await wait(() => dialogButton('确认') && !dialogButton('确认').disabled, 'custom reason ready to confirm');
  click(dialogButton('确认'));
  await wait(() => outcome()?.includes('请勿重复提交') && dialogButton('核对本机回执') &&
    !dialogButton('核对本机回执').disabled, 'lost response has only read-only recovery');
  assert(!dialogButton('确认') && (await inspect('3')).value.status === 'uncertain',
    'unknown response stays uncertain in shared native journal');
  await stage('report_other_uncertain', { outcome: outcome() });
  click(dialogButton('核对本机回执'));
  await wait(() => !dialogButton('核对本机回执')?.disabled &&
    outcome()?.includes('请勿重复提交'), 'local readback cannot confirm an absent acknowledgement');
  await close('关闭');
  click(report('hot', '3'));
  await choose('段子或者无意义的评论');
  await wait(() => outcome()?.includes('请勿重复提交'), 'reopening same target reads previous responsibility');
  assert(!dialogButton('确认'), 'new reason cannot replay the old uncertain write');
  await stage('report_reopened', { outcome: outcome() });
  await close('关闭');
};
