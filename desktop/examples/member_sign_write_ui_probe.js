// Closed check-in server; every click uses the production Vue and Rust paths.
(async () => {
  const one = selector => document.querySelector(selector);
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const emit = (stage, payload = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...payload },
  });
  async function wait(test, label) {
    for (let attempt = 0; attempt < 200; attempt++) {
      const result = await test();
      if (result) return result;
      await sleep(40);
    }
    throw Error(label);
  }
  async function capture(stage) {
    await wait(async () => {
      await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      return !document.getAnimations().some(animation =>
        Number.isFinite(animation.effect?.getComputedTiming().endTime) &&
        (animation.pending || animation.playState === 'running'));
    }, 'settled paint ' + stage);
    await emit(stage);
  }
  const action = kind => one(`.sign-card button[data-action="${kind}"]:not([aria-disabled="true"])`);
  const dialog = () => one('.sign-dialog[open]');
  const signedResult = () => dialog()?.querySelector('h3')?.textContent === '合成今日乐签歌曲';
  const member = () => [...document.querySelectorAll('.sidebar button:not(:disabled)')]
    .find(button => button.textContent.trim() === '会员中心');
  let step = 'enter';
  try {
    (await wait(member, 'member sidebar ready')).click();
    const first = await wait(() => action('check-in') || action('reconcile'), 'write or pending state');
    let outcome;
    if (first.dataset.action === 'check-in') {
      step = 'ready';
      if (first.dataset.dayKey !== '2026-11-01' || first.textContent.trim() !== '立即打卡')
        throw Error('the action must match the native official day');
      await capture('ux_member_checkin_ready');
      first.focus();
      first.click();
      first.click();
      await wait(() => one('.sign-card [aria-disabled="true"][data-action="check-in"]'), 'pending button retains place');
      step = 'write-result';
      outcome = await wait(() => signedResult() ? 'confirmed' : action('reconcile') && 'lost', 'write receipt or reconcile action');
      if (outcome === 'lost') {
        if (action('check-in') || !one('.sign-action-notice')?.textContent.includes('待确认'))
          throw Error('uncertain write cannot show a new check-in');
        await capture('ux_member_checkin_unknown');
      } else {
        await capture('ux_member_checkin_confirmed');
        one('.sign-dialog [aria-label="关闭乐签"]').click();
        await wait(() => !dialog() && document.activeElement === action('view'), 'focus returns to today action');
        action('view').click();
        await wait(signedResult, 'confirmed detail reopens without a write');
      }
    } else {
      step = 'reopen-unknown';
      if (action('check-in') || !one('.sign-action-notice')?.textContent.includes('待确认'))
        throw Error('persisted uncertainty must survive process restart');
      await capture('ux_member_checkin_reopened');
      first.click();
      await sleep(80);
      await wait(() => action('reconcile') && !dialog(), 'stale server readback remains unknown');
      await capture('ux_member_checkin_still_unknown');
      step = 'reconcile';
      await emit('member-sign-reveal');
      await wait(() => window.__memberSignRevealed === true, 'fixture revealed previously accepted write');
      action('reconcile').click();
      await wait(signedResult, 'later history resolves same-day result');
      if (action('check-in')) throw Error('confirmed check-in cannot be replayed');
      await capture('ux_member_checkin_reconciled');
      outcome = 'reconciled';
    }
    await emit('complete', { ok: true, outcome });
  } catch (error) {
    await emit('failed', { step, reason: error.message, card: one('.sign-card')?.textContent });
  }
})();
