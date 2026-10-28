// Apply this local visual preference before the first document paint.
(() => {
  let preference = 'system';
  try {
    preference = localStorage.getItem('cumulus.appearance.v1');
  } catch {}
  document.documentElement.dataset.theme =
    preference === 'light' || preference === 'dark'
      ? preference
      : matchMedia('(prefers-color-scheme: dark)').matches
        ? 'dark'
        : 'light';
  let motion = 'system';
  try {
    motion = localStorage.getItem('cumulus.motion.v1');
  } catch {}
  document.documentElement.dataset.motion =
    motion === 'disabled' || matchMedia('(prefers-reduced-motion: reduce)').matches
      ? 'reduced'
      : 'full';
})();
