// A fresh desktop process starts with an empty synthetic catalogue and finds
// its previous interrupted stop only through the local registration list.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (condition, message) => { if (!condition) throw new Error(message); };
  const wait = async (read, message) => {
    for (let index = 0; index < 150; index++) {
      const value = await read();
      if (value) return value;
      await sleep(65);
    }
    throw new Error(message);
  };
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const enabled = (node) => node && !node.disabled && node.getAttribute('aria-disabled') !== 'true' && node;
  const click = (node) => {
    assert(enabled(node), 'enabled control expected');
    node.focus({ preventScroll: true });
    node.click();
  };
  const pending = () => one('.effect-directory [data-effect-registration="300"]');
  const controls = () => one('.effect-parametric');
  const action = (name) => controls()?.querySelector(`button[data-peq-action="${name}"]`);
  const phase = (name) => controls()?.getAttribute('data-peq-phase') === name;
  const capture = async (label) => {
    await emit(`ux_${label}`, {captureHandshake: true});
    await sleep(450);
  };
  let stage = 'restore';
  try {
    const account = await wait(async () => {
      const value = await invoke('account_snapshot');
      return value.status === 'authenticated' && value;
    }, 'synthetic account did not restore in the new process');
    click(await wait(() => enabled(one('.equalizer-toggle')), 'EQ launcher'));
    const eq = await wait(() => one('.equalizer-dialog[open]'), 'EQ dialog');
    click([...eq.querySelectorAll('button')].find((node) => node.textContent.trim() === '音效目录'));
    stage = 'discover';
    await wait(() => enabled(pending()) && pending().textContent.includes('停止待核对') &&
      one('.effect-directory .effect-directory-message')?.textContent.includes('此分类暂无音效'),
      'pending registration missing from the newly opened directory');
    assert(!one('.effect-directory-card') && !controls(), 'discovery must precede any card or details');
    const other = {epoch: account.epoch, id: 301};
    const otherStatus = await invoke('effect_peq_status', other);
    assert(otherStatus.phase === 'idle' && otherStatus.activeId === 300,
      'another effect sees the pending registration after restart');
    let refused;
    try { await invoke('effect_peq_prepare', other); } catch (issue) { refused = issue; }
    assert(refused?.code === 'busy', 'pending cleanup prevents preparation of another effect');
    await capture('01_discovered');
    click(pending());
    await wait(() => phase('recovery_required') && enabled(action('status')) && enabled(action('stop')),
      'local recovery controls did not open');
    assert(!enabled(action('prepare')) && !enabled(action('activate')), 'unknown stop cannot register again');

    stage = 'reconcile';
    click(action('status'));
    await sleep(0);
    await wait(() => phase('recovery_required') && enabled(action('status')) && enabled(action('stop')),
      'read-only reconciliation must preserve unknown stop');
    click(action('stop'));
    click(await wait(() => enabled(action('cancel-stop')), 'stop retry requires confirmation'));
    await wait(() => !action('confirm-stop') && enabled(action('stop')), 'cancel leaves pending stop');
    await capture('02_cancelled_retry');

    stage = 'explicit-stop';
    click(action('stop'));
    click(await wait(() => enabled(action('confirm-stop')), 'reopen explicit stop confirmation'));
    await wait(() => phase('idle') && !pending() && !enabled(action('stop')), 'confirmed release clears pending list');
    await capture('03_released');
    await emit('complete', { ok: true, stageCount: 3, reopenedFromLocalList: true, otherEffectBlocked: true });
  } catch (error) {
    await emit('failed', { step: stage, message: String(error), stack: error?.stack,
      phase: controls()?.getAttribute('data-peq-phase') });
  }
})();
