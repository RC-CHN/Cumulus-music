// Passive WebView witness only. Never emits a player notice or changes CSS/IPC.
window.__observeOrdinarySkipNotice = () => {
  const selector = '.playerbar .ordinary-skip-notice';
  const expectedText = '已为您自动跳过不能播放的歌曲';
  const records = new Map();
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (condition, message) => { if (!condition) throw new Error(message); };
  const now = () => performance.now();
  let frame;
  function record(node) {
    if (!records.has(node)) records.set(node, {
      text: node.textContent.trim(), role: node.getAttribute('role'),
      mountedAt: now(), visibleAt: null, removedAt: null,
    });
  }
  function fullyVisible(node) {
    const style = getComputedStyle(node);
    const box = node.getBoundingClientRect();
    const span = node.querySelector('span');
    return document.visibilityState === 'visible' && style.opacity === '1' &&
      style.visibility === 'visible' && style.display !== 'none' &&
      !node.closest('[inert]') && box.width > 0 && box.height > 0 &&
      box.left >= 0 && box.top >= 0 && box.right <= innerWidth && box.bottom <= innerHeight &&
      span && span.scrollWidth <= span.clientWidth + 1 &&
      node.contains(document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2));
  }
  function scan() {
    const active = [...document.querySelectorAll(selector)];
    for (const node of active) {
      record(node);
      const row = records.get(node);
      if (row.visibleAt === null && fullyVisible(node)) row.visibleAt = now();
    }
    for (const [node, row] of records)
      if (!node.isConnected && row.removedAt === null) row.removedAt = now();
    return active;
  }
  const mutation = new MutationObserver((changes) => {
    for (const change of changes) {
      for (const node of change.addedNodes) {
        if (!(node instanceof Element)) continue;
        // Catch a transient addition even if it was removed in the same turn.
        if (node.matches('.ordinary-skip-notice')) record(node);
        for (const child of node.querySelectorAll('.ordinary-skip-notice')) record(child);
      }
    }
    scan();
  });
  mutation.observe(document.documentElement, { childList: true, subtree: true });
  function watchFrame() {
    if (records.size) scan();
    frame = requestAnimationFrame(watchFrame);
  }
  watchFrame();
  function visible() {
    return scan().some(fullyVisible);
  }
  function capture(expect) {
    const active = scan();
    if (!expect) {
      assert(active.length === 0 && records.size === 0, 'unexpected ordinary copyright notice');
      return { visible: [], paint: [] };
    }
    assert(active.length === 1 && records.size === 1 && fullyVisible(active[0]),
      'exactly one fully visible ordinary copyright notice');
    const node = active[0], row = records.get(node), box = node.getBoundingClientRect();
    assert(row.text === expectedText && row.role === 'status', 'exact localized accessible notice');
    const style = getComputedStyle(node);
    const pad = Number.parseFloat(style.paddingLeft);
    const x = box.left + Math.min(4, pad / 2), y = box.top + box.height / 2;
    const rgb = /^rgb\((\d+),\s*(\d+),\s*(\d+)\)$/.exec(style.backgroundColor);
    assert(pad >= 4 && document.elementFromPoint(x, y) === node && rgb &&
      style.backgroundImage === 'none', 'notice has an uncovered solid compositor sample');
    return {
      visible: [{ text: row.text, role: row.role }],
      paint: [{ x, y, color: 'rgb(' + rgb.slice(1).join(', ') + ')', source: selector }],
    };
  }
  async function finish(expect) {
    const start = now();
    if (expect) {
      while (now() - start < 5000 && (scan().length || [...records.values()].some((row) => row.removedAt === null)))
        await sleep(45);
      assert(scan().length === 0 && records.size === 1, 'ordinary notice removed once with no overlap or replay');
      const [row] = records.values();
      const elapsed = row.removedAt - row.mountedAt;
      assert(row.visibleAt !== null && elapsed >= 2500 && elapsed <= 4400, 'ordinary notice expired near three seconds');
      await sleep(175);
    } else {
      assert(scan().length === 0 && records.size === 0, 'no ordinary notice before observation window');
      await sleep(3500);
    }
    assert(scan().length === 0 && records.size === Number(expect), 'no late or repeated ordinary notice');
    return {
      observedAfterCaptureMs: now() - start,
      mounts: [...records.values()].map((row) => ({ ...row, elapsedMs: row.removedAt - row.mountedAt })),
      remaining: 0,
    };
  }
  function stop() {
    cancelAnimationFrame(frame);
    mutation.disconnect();
  }
  return { capture, finish, visible, stop };
};
