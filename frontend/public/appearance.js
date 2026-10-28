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
})();
