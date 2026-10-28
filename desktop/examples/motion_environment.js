// Isolated probe only. Inject an OS preference event and a storage failure;
// leave real CSS, animations, scrolling, account IPC and libmpv in place.
(() => {
  const originalMedia = window.matchMedia.bind(window);
  const queries = [];
  let reduced = false, failWrite = false;
  window.matchMedia = query => {
    if (query !== '(prefers-reduced-motion: reduce)') return originalMedia(query);
    const result = new EventTarget();
    Object.defineProperties(result, { matches: { get: () => reduced }, media: { value: query } });
    queries.push(result);
    return result;
  };
  const setItem = Storage.prototype.setItem;
  Storage.prototype.setItem = function(key, value) {
    if (key === 'cumulus.motion.v1' && failWrite) throw new DOMException('Synthetic write failure', 'QuotaExceededError');
    return setItem.call(this, key, value);
  };
  const scrollIntoView = Element.prototype.scrollIntoView;
  const animate = Element.prototype.animate;
  const proof = window.__motionProof = {
    scrolls: [], animations: [],
    system(value) { reduced = value; queries.forEach(q => q.dispatchEvent(new MediaQueryListEvent('change', { media: q.media, matches: value }))); },
    failWrite(value) { failWrite = value; },
  };
  Element.prototype.scrollIntoView = function(options) {
    proof.scrolls.push({ className: this.className, behavior: options?.behavior });
    return scrollIntoView.call(this, options);
  };
  Element.prototype.animate = function(...args) {
    const result = animate.apply(this, args);
    proof.animations.push(result);
    return result;
  };
})();
