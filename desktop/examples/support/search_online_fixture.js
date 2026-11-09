// Only example hosts load this: their sealed local transport has no default route.
if (!navigator.onLine) {
  Object.defineProperty(navigator, 'onLine', { configurable: true, value: true });
  window.dispatchEvent(new Event('online'));
}
