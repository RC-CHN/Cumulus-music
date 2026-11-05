// A closed standard-vinyl WebView fixture; no real request can be made.
window.__vinylLikeRun = async function ({ epoch, first, comment, capture, tab, scroller, wait, click, invoke, assert, one }) {
  const selected = (group, id = '1') => one(`[data-vinyl-comment-group="${group}"] [data-vinyl-comment-id="${id}"]`);
  const button = (group, id) => selected(group, id)?.querySelector('.vinyl-comment-like');
  const reconcile = (group) => selected(group)?.querySelector('.vinyl-comment-like-reconcile');
  const operations = () => invoke('player_vinyl_comment_like_operations', { epoch, playId: first.playId });
  const buttons = () => [button('hot'), button('latest')];
  const correct = (liked, label) => buttons().every((node) =>
    node?.getAttribute('aria-pressed') === String(liked) && node.textContent.trim() === label);

  let anchor;
  const stage = async (name, fields) => {
    const scroll = scroller().scrollTop;
    assert(scroll >= 35 && Math.abs(scroll - anchor) < 1, 'inline action kept the comment scroll anchor');
    await capture(name, first, { scroll, ...fields });
    assert(Math.abs(scroller().scrollTop - anchor) < 1, 'settled capture retained the same scroll anchor');
  };

  await wait(() => comment()?.getAttribute('aria-pressed') === 'false', 'signed t1 enters standard comments');
  click(comment());
  await wait(() => tab() === 'comments' && correct(false, '') && buttons().every((node) => !node.disabled),
    'both zero-count inline controls are enabled');
  assert(button('latest', '2')?.disabled, 'unknown liked state must not send');
  assert((await operations()).value.length === 0, 'no prior intent');
  // Put a nonzero anchor between the hot and latest rows, away from the append sentinel.
  scroller().scrollTop = 72;
  anchor = scroller().scrollTop;
  assert(anchor >= 35, 'the comments have a nonzero scroll anchor');
  await stage('like_initial', { pressed: false, shownCount: '' });

  click(button('hot'));
  await wait(() => correct(true, '1') && buttons().every((node) => !node.disabled),
    'confirmed like projects the same state to both lists');
  await stage('like_confirmed', { pressed: true, shownCount: '1' });

  click(button('latest'));
  await wait(() => correct(true, '1') && buttons().every((node) => node.disabled) &&
    reconcile('hot') && reconcile('latest'), 'lost response preserves confirmed value and only offers readback');
  const pending = (await operations()).value;
  assert(pending.length === 1 && pending[0].id === '1' && pending[0].phase === 'uncertain' && !pending[0].desired,
    'durable unlike remains uncertain');
  // Dispatch two actual DOM clicks on disabled controls; neither can send.
  button('hot').click();
  button('latest').click();
  await stage('unlike_uncertain', { pressed: true, shownCount: '1', phase: pending[0].phase });

  click(reconcile('hot'));
  await wait(() => correct(false, '') && !reconcile('latest') && buttons().every((node) => !node.disabled),
    'readback clears both recovery entries without replay');
  assert((await operations()).value.length === 0, 'readback settled the shared pending record');
  await stage('unlike_reconciled', { pressed: false, shownCount: '' });
};
