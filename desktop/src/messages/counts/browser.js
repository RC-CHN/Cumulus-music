(() => {
  'use strict';
  if (window.top !== window || typeof navigator.onLine !== 'boolean') return;
  const nonce = '__CUMULUS_COUNT_NETWORK_NONCE__';
  const invoke = window.__TAURI_INTERNALS__.invoke.bind(window.__TAURI_INTERNALS__);
  let sequence = 0;
  let alive = true;
  let queue = Promise.resolve();
  function submit(initial, online) {
    queue = queue.then(async () => {
      if (!alive) return;
      try {
        await invoke('observe_message_connectivity', {
          report: { nonce, sequence: ++sequence, initial, online },
        });
      } catch {
        // A replaced document may lose its authority before delivery.
        cleanup();
      }
    });
  }
  function changed(event) {
    if (!event.isTrusted) return;
    const online = navigator.onLine;
    if (online !== (event.type === 'online')) return;
    submit(false, online);
  }
  function cleanup() {
    alive = false;
    window.removeEventListener('online', changed);
    window.removeEventListener('offline', changed);
    window.removeEventListener('pagehide', cleanup);
  }
  window.addEventListener('online', changed);
  window.addEventListener('offline', changed);
  window.addEventListener('pagehide', cleanup, { once: true });
  submit(true, navigator.onLine);
})();
