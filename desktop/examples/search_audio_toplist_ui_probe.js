// Real search popup and IPC against the assigned/control synthetic PC experiment.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const frame = () => new Promise((resolve) => requestAnimationFrame(resolve));
  const assert = (condition, message) => { if (!condition) throw Error(message); };
  async function wait(condition, message) {
    for (let i = 0; i < 200; i++) {
      if (condition()) return;
      await sleep(60);
    }
    throw Error(message);
  }
  const emit = (stage, details = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...details },
  });
  const input = () => one('[aria-label="搜索音乐"]');
  const panel = () => one('.search-panel');
  const chartNames = () => all('.search-chart h3').map((n) => n.textContent);
  const expected = window.__toplistAssigned ? ['热搜榜', '播客榜', '摇滚榜', '听书榜'] : ['热搜榜', '摇滚榜'];
  const ordinary = () => one('[title="普通热搜条目"]');
  const more = (name) => one(`[aria-label="${name}：更多"]`);
  let count = 0, last = 'startup';
  async function visibleCharts() {
    await wait(() => chartNames().join() === expected.join() && ordinary(), 'assigned chart headings and ordinary row');
    if (window.__toplistAssigned) {
      await wait(() => one('[title="合成听书榜条目"]') && more('播客榜') && more('听书榜'),
        'assigned audio card/detail and destinations');
    } else {
      assert(!more('播客榜') && !more('听书榜') && !one('[title="合成播客榜条目"]'), 'control has no audio card');
    }
  }
  async function capture(label) {
    last = label;
    await frame();
    await wait(() => panel() && panel().getAnimations().every((a) => a.playState !== 'running' && !a.pending),
      'search panel transition finished');
    await frame();
    const element = panel();
    const rect = element.getBoundingClientRect();
    assert(rect.left >= 11 && rect.right <= innerWidth - 11 && rect.bottom <= innerHeight - 79 &&
      panel().scrollWidth <= panel().clientWidth, 'chart panel stays within viewport');
    await emit(`ux_${String(++count).padStart(2, '0')}_${label}`, {
      width: innerWidth, height: innerHeight, ratio: devicePixelRatio, screenX, screenY,
      names: chartNames(), scrollTop: panel().scrollTop,
      scrollable: panel().scrollHeight > panel().clientHeight, noHorizontalOverflow: true,
      inputFocused: document.activeElement === input(), suggestions: all('[data-search-suggestion]').length,
      more: ['播客榜', '听书榜'].filter((name) => !!more(name)),
      paintSamples: [{ x: rect.left + 4, y: rect.top + rect.height / 2,
        color: getComputedStyle(element).backgroundColor }],
    });
  }
  function type(value) {
    input().value = value;
    input().dispatchEvent(new Event('input', { bubbles: true }));
  }
  try {
    await wait(() => input() && !input().disabled && one('.track-row'), 'authenticated fixture booted');
    input().focus();
    await visibleCharts();
    await capture('opened');

    input().dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }));
    type('合成词');
    await sleep(380);
    assert(!all('[data-search-suggestion]').length && !chartNames().length &&
      one('.search-exact')?.disabled && buttonDisabled('开始搜索') &&
      panel()?.textContent.includes('输入完成后显示搜索建议'),
      'IME does not submit or show hot charts while typing');
    input().dispatchEvent(new CompositionEvent('compositionend', { bubbles: true }));
    await wait(() => all('[data-search-suggestion]').length > 0, 'ordinary suggestions resume');
    await capture('typed');
    input().dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    await wait(() => !panel() && document.activeElement === input(), 'Escape closes and restores input');
    type('');
    await emit('resize', { width: 760, height: 540 });
    await wait(() => innerWidth === 760 && innerHeight === 540, 'narrow window');
    input().click();
    await visibleCharts();
    if (panel().scrollHeight > panel().clientHeight) {
      panel().scrollTop = panel().scrollHeight;
      await wait(() => panel().scrollTop > 0, 'chart section scrolls inside narrow popup');
    } else {
      assert(!window.__toplistAssigned, 'assigned charts should require scroll in the narrow popup');
    }
    await capture('reopened');
    await emit('complete', { ok: true, count, assigned: window.__toplistAssigned, realMusicRequests: 0, accountWrites: 0 });
  } catch (error) {
    await emit('failed', { ok: false, last, count, error: String(error), names: chartNames(),
      text: panel()?.textContent.slice(0, 900) });
  }
  function buttonDisabled(label) {
    return one(`button[aria-label="${label}"]`)?.disabled;
  }
})();
