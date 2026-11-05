(async () => {
  if (window.__legacyRenewalProofStarted) return;
  window.__legacyRenewalProofStarted = true;
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const report = (payload) =>
    window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
      event: 'legacy-renewal-proof',
      payload,
    });
  const account = () => document.querySelector('.account-button');
  const dialog = () => document.querySelector('.account-dialog[open]');
  const menu = () => document.querySelector('[role="menu"][aria-label="账号菜单"]');
  const renewal = () => document.querySelector('.session-renewal')?.dataset.state;
  const restored = () => account()?.textContent.includes('合成续期账号');
  async function waitFor(predicate, step) {
    const deadline = performance.now() + 20000;
    while (performance.now() < deadline) {
      if (predicate()) return;
      await sleep(50);
    }
    throw new Error(step);
  }
  try {
    await waitFor(() => restored() && !account().disabled, 'restored_account_ready');
    account().click();
    await waitFor(() => dialog() || menu(), 'account_entry');
    if (!dialog()) {
      const item = [...menu().querySelectorAll('[role="menuitem"]')].find(
        (node) => node.textContent.trim() === '切换账号',
      );
      if (!item) throw new Error('account_switch_item');
      item.click();
    }
    await waitFor(() => renewal() === 'missing_credentials', 'account_dialog');
    // Real elapsed time, shared by both cohorts. No fake native or JS clock.
    await sleep(4400);
    await report({ ok: !!restored(), renewalState: renewal() });
  } catch (error) {
    await report({ ok: false, step: error.message, renewalState: renewal() });
  }
})();
