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
    await waitFor(restored, 'restored_account');
    account().click();
    await waitFor(() => renewal() === 'missing_credentials', 'account_dialog');
    // Real elapsed time, shared by both cohorts. No fake native or JS clock.
    await sleep(4400);
    await report({ ok: !!restored(), renewalState: renewal() });
  } catch (error) {
    await report({ ok: false, step: error.message, renewalState: renewal() });
  }
})();
