// Real My Podcasts DOM, product model/IPC, and a sealed native badge adapter.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const button = (label, root = document) => [...(root?.querySelectorAll('button') ?? [])]
    .find((node) => node.textContent.trim() === label || node.getAttribute('aria-label') === label);
  const pane = () => one('.podcast-page');
  const card = (id) => one(`.podcast-card[data-radio-id="${id}"]`, pane());
  const badge = (id) => one('[data-podcast-new-badge]', card(id));
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, payload = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...payload },
  });
  const assert = (value, message) => { if (!value) throw Error(message); };
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  async function wait(test, label) {
    for (let n = 0; n < 200; n++) {
      const result = await test();
      if (result) return result;
      await sleep(50);
    }
    throw Error(label);
  }
  function shown(node) {
    if (!node?.isConnected || node.disabled || node.closest('[inert]')) return false;
    const box = node.getBoundingClientRect();
    return box.width > 0 && box.height > 0 &&
      node.contains(document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2));
  }
  function opaque(node) {
    for (let current = node; current; current = current.parentElement) {
      const { opacity, visibility } = getComputedStyle(current);
      if (opacity !== '1' || visibility !== 'visible') return false;
    }
    return true;
  }
  function click(node) {
    assert(shown(node), 'ready visible control');
    node.focus({ preventScroll: true });
    node.click();
  }
  function solidPadding(selector) {
    const node = one(selector);
    const rect = node.getBoundingClientRect();
    const padding = Number.parseFloat(getComputedStyle(node).paddingLeft);
    const x = rect.left + Math.min(5, padding / 2), y = rect.top + rect.height / 2;
    assert(padding >= 4 && document.elementFromPoint(x, y) === node && opaque(node),
      'visible solid padding: ' + selector);
    for (let current = node; current; current = current.parentElement) {
      const style = getComputedStyle(current);
      assert(style.backgroundImage === 'none', 'solid sample only');
      const rgb = /^rgb\((\d+),\s*(\d+),\s*(\d+)\)$/.exec(style.backgroundColor);
      if (rgb) return { x, y, color: 'rgb(' + rgb.slice(1).join(', ') + ')' };
      assert(style.backgroundColor === 'transparent' ||
        /^rgba\(\d+,\s*\d+,\s*\d+,\s*0\)$/.test(style.backgroundColor), 'transparent ancestor only');
    }
    throw Error('no opaque padding color');
  }
  function badgeLayout(id) {
    const item = badge(id);
    if (!item) return null;
    const cover = one('.podcast-card-cover', card(id)).getBoundingClientRect();
    const badgeBox = item.getBoundingClientRect();
    assert(shown(item) && opaque(item) &&
      badgeBox.left >= cover.left && badgeBox.right <= cover.right &&
      badgeBox.top >= cover.top && badgeBox.bottom < cover.bottom &&
      cover.right - badgeBox.right <= 6 && badgeBox.top - cover.top <= 6,
      'badge stays in top-right of the actual cover: ' + id);
    return { id, text: item.textContent.trim(), label: item.getAttribute('aria-label') };
  }
  const requests = [], detailReads = [];
  let index = 0, last = 'startup';
  async function capture(label, target) {
    last = label;
    await wait(() => shown(target()) && opaque(target()), 'settled visible stage: ' + label);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const badges = ['81', '82', '83'].map(badgeLayout).filter(Boolean);
    assert(pane().scrollWidth <= pane().clientWidth + 1, 'podcast layout fits viewport');
    await emit(`ux_${String(++index).padStart(2, '0')}_${label}`, {
      actual: document.visibilityState === 'visible', badges,
      detail: one('.podcast-summary h2', pane())?.textContent.trim() ?? null,
      calls: requests.slice(), readCount: detailReads.length,
      screenX, screenY, ratio: devicePixelRatio,
      paintSamples: window.__CUMULUS_PROOF_INVOKE__
        ? ['.sidebar-caption', '.playerbar'].map(solidPadding) : [],
    });
    await wait(() => window.__podcastProgressStage === `ux_${String(index).padStart(2, '0')}_${label}`,
      'durable stage acknowledgement');
  }
  function detailReady(id) {
    return one('h1', pane())?.textContent.trim() === '播客详情' &&
      one(`[data-voice-id="${9000 + Number(id)}"]`, pane());
  }
  async function back() {
    click(await wait(() => {
      const node = button('返回播客列表', pane());
      return shown(node) && node;
    }, 'real Back button'));
    await wait(() => shown(card('81')) && shown(card('82')) && shown(card('83')),
      'returned to current subscription snapshot');
  }
  function refresh() {
    return button('刷新', pane());
  }
  let core, clearOriginal, pageOriginal;
  try {
    const account = await wait(async () => {
      const value = await invoke('account_snapshot');
      return value.status === 'authenticated' && value;
    }, 'sealed authenticated account');
    const provides = one('#app').__vue_app__._context.provides;
    core = provides[Reflect.ownKeys(provides).find((key) =>
      typeof key === 'symbol' && key.description === 'core-client')];
    clearOriginal = core.podcastClearSubscribedBadge;
    pageOriginal = core.podcastPage;
    core.podcastClearSubscribedBadge = async (owner, receipt, id) => {
      assert(owner === account.epoch && /^[0-9a-f]{32}$/.test(receipt), 'real account-bound receipt');
      try {
        const result = await clearOriginal.call(core, owner, receipt, id);
        requests.push([id, 'acknowledged']);
        return result;
      } catch (error) {
        requests.push([id, error?.code ?? 'unexpected']);
        throw error;
      }
    };
    core.podcastPage = async (owner, query, fresh) => {
      const result = await pageOriginal.call(core, owner, query, fresh);
      if (query.scope.kind === 'radio') detailReads.push(query.scope.id);
      return result;
    };

    click(await wait(() => {
      const node = button('我的播客', one('.sidebar-navigation'));
      return shown(node) && node;
    }, 'real sidebar entry'));
    await wait(() => shown(card('81')) && shown(card('82')) && shown(card('83')),
      'signed subscription cards');
    assert(badge('81')?.textContent.trim() === '99+' &&
      badge('81').getAttribute('aria-label') === '108 集新节目' &&
      badge('82')?.textContent.trim() === '4' && !badge('83') &&
      !one('[data-podcast-recent-tab] [data-podcast-new-badge]', pane()) &&
      one('[data-podcast-recent-tab]', pane()).textContent.trim() === '最近更新单集',
      'positive per-card count, overflow, zero, and no recent-tab badge');
    await capture('initial_badges', () => badge('81'));

    const input = one('[aria-label="播客编号或链接"]', pane());
    input.value = '81';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    click(await wait(() => {
      const node = button('打开播客', pane());
      return shown(node) && node;
    }, 'ordinary detail input action'));
    await wait(() => detailReady('81') && detailReads.length === 1, 'ordinary detail rendered');
    assert(requests.length === 0, 'ordinary typed detail did not clear');
    await capture('ordinary_detail', () => one('.podcast-summary h2', pane()));
    click(await wait(() => shown(refresh()) && refresh(), 'enabled detail refresh'));
    await wait(() => detailReady('81') && detailReads.length === 2 && shown(refresh()),
      'ordinary detail refresh completed');
    assert(requests.length === 0, 'ordinary detail refresh did not clear');
    await capture('ordinary_refresh', refresh);
    await back();
    assert(badge('81')?.textContent.trim() === '99+', 'ordinary detail and refresh left badge');

    click(card('81'));
    await wait(() => detailReady('81') && requests.length === 1, 'explicit acknowledged card detail');
    assert(JSON.stringify(requests) === JSON.stringify([['81', 'acknowledged']]), 'real clear ACK');
    await capture('confirmed_detail', () => one('.podcast-summary h2', pane()));
    await back();
    await wait(() => !badge('81') && badge('82'), 'confirmed badge gone on Back');
    await capture('confirmed_back', () => badge('82'));

    click(card('82'));
    await wait(() => detailReady('82') && requests.length === 2, 'explicit lost reply still opened detail');
    assert(JSON.stringify(requests[1]) === JSON.stringify(['82', 'network']), 'actual lost clear reply');
    await capture('lost_detail', () => one('.podcast-summary h2', pane()));
    await back();
    assert(badge('82')?.textContent.trim() === '4' && !badge('81'), 'lost reply did not pretend to clear');
    await capture('lost_back', () => badge('82'));

    click(card('82'));
    await wait(() => detailReady('82') && requests.length === 3, 'second explicit detail still opens');
    assert(JSON.stringify(requests[2]) === JSON.stringify(['82', 'stale']), 'same ambiguous fact rejected natively');
    await back();
    assert(badge('82')?.textContent.trim() === '4' && !badge('81'), 'lost badge persists without resend');
    await capture('no_replay', () => badge('82'));
    await emit('complete', { ok: true, stageCount: index, remoteWrites: 0,
      realMusicRequests: 0, calls: requests, detailReads });
  } catch (error) {
    await emit('failed', { ok: false, last, error: String(error), requests, detailReads });
  } finally {
    if (core && clearOriginal && pageOriginal) {
      core.podcastClearSubscribedBadge = clearOriginal;
      core.podcastPage = pageOriginal;
    }
  }
})();
