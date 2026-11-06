// An empty guest queue opens the regular login flow; no credential is submitted.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const invoke = window.__CUMULUS_PROOF_INVOKE__ ?? window.__TAURI_INTERNALS__.invoke;
  const emit = (stage, value) => invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...value } });
  async function wait(condition, label) {
    for (let attempt = 0; attempt < 260; attempt++) {
      const result = condition();
      if (result) return result;
      await new Promise((resolve) => setTimeout(resolve, 55));
    }
    throw Error(label);
  }
  const assert = (test, message) => { if (!test) throw Error(message); };
  const login = () => one('.login-dialog[open]:not(.modal-leave-active)');
  try {
    const toggle = await wait(() => one('.queue-toggle:not(:disabled)'), 'guest native player ready');
    if (login()) {
      login().querySelector('[aria-label="关闭登录"]').click();
      await wait(() => !one('.login-dialog'), 'initial login dismissed');
    }
    const provides = one('#app').__vue_app__._context.provides;
    const client = provides[Reflect.ownKeys(provides).find((key) => key.description === 'core-client')];
    const before = await client.accountSnapshot();
    assert(before.status !== 'authenticated', 'synthetic guest');
    const route = () => one('.page-view')?.querySelector('h1')?.textContent.trim();
    const start = route();
    toggle.click();
    const action = await wait(() => one('.queue-empty button'), 'confirmed guest queue empty');
    const drawer = one('.queue-panel');
    const queue = await client.playerQueue(before.epoch);
    action.focus();
    action.click();
    await wait(login, 'normal login opened from empty queue');
    assert(one('.queue-panel') === drawer && route() === start && !one('.discovery-home'),
      'guest keeps queue and current page');
    await wait(() => !one('.login-dialog.modal-enter-active'), 'login animation settled');
    await emit('ux_guest_login', { width: innerWidth, height: innerHeight, ratio: devicePixelRatio, screenX, screenY });
    login().querySelector('[aria-label="关闭登录"]').click();
    await wait(() => !one('.login-dialog'), 'login cancellation');
    const after = await client.accountSnapshot();
    assert(after.epoch === before.epoch && after.status === before.status && action === one('.queue-empty button') &&
      JSON.stringify(await client.playerQueue(after.epoch)) === JSON.stringify(queue),
    'cancellation preserves account and empty queue');
    await emit('complete', { ok: true, guestLoginOpened: true, routePreserved: true, realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', { ok: false, error: String(error), queueReady: !!one('.queue-toggle:not(:disabled)'),
      queueOpen: !!one('.queue-panel'), loginOpen: !!login() });
  }
})();
