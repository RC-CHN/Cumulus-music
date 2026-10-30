// Test scripts explicitly call this adapter; Tauri's own invoke property is immutable.
(() => {
  if (window.__CUMULUS_PROOF_INVOKE__) return;
  const invoke = window.__TAURI_INTERNALS__.invoke.bind(window.__TAURI_INTERNALS__);
  window.__CUMULUS_PROOF_INVOKE__ = async function (command, args, ...rest) {
    const stage = command === 'plugin:event|emit' && args?.event === 'ux-proof'
      && /^ux_[A-Za-z0-9_]+$/.test(args.payload?.stage ?? '') ? args.payload.stage : null;
    if (stage) {
      window.__CUMULUS_PROOF_ACK__ = '';
      args = { ...args, payload: { ...args.payload, captureHandshake: true } };
    }
    const result = await invoke(command, args, ...rest);
    if (stage) {
      const end = performance.now() + 12000;
      while (window.__CUMULUS_PROOF_ACK__ !== stage) {
        if (performance.now() >= end) throw Error('screenshot completion acknowledgement timed out');
        await new Promise(resolve => setTimeout(resolve, 40));
      }
    }
    return result;
  };
})();
