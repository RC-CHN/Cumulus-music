// Apply this local visual preference before the first document paint.
(() => {
  let preference = 'system';
  try {
    preference = localStorage.getItem('cumulus.appearance.v1');
  } catch {}
  // Match the runtime's bounded parser; a CSS value can only be a six-digit hex color.
  let custom;
  if (typeof preference === 'string' && preference.startsWith('{') && preference.length <= 96) {
    try {
      const entry = JSON.parse(preference);
      if (
        entry?.theme === 'custom' &&
        typeof entry.color === 'string' &&
        entry.color.length === 7 &&
        /^#[0-9a-f]{6}$/i.test(entry.color)
      )
        custom = entry.color.toLowerCase();
    } catch {}
  }
  if (custom) document.documentElement.style.setProperty('--custom-theme-color', custom);
  document.documentElement.dataset.theme = custom
    ? 'custom'
    : preference === 'light' || preference === 'dark' || preference === 'red'
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
