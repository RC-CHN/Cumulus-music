// Real renderer controls plus the production native poller and durable journal.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (condition, detail) => { if (!condition) throw Error(detail); };
  const wait = async (predicate, detail) => {
    for (let attempt = 0; attempt < 220; attempt++) {
      const result = await predicate();
      if (result) return result;
      await sleep(50);
    }
    throw Error(detail);
  };
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, data = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...data } });
  const drawer = () => one('.message-drawer');
  const trigger = () => one('.message-drawer-trigger');
  const dot = () => !!trigger()?.querySelector('.message-drawer-unread-dot');
  const readAll = () => one('.message-drawer [aria-label="一键全读"]');
  const refresh = () => one('.message-drawer [aria-label="刷新消息"]');
  const modal = () => one('[data-message-read-all-confirm]');
  const text = (el) => el?.textContent.trim();
  const status = () => one('[data-message-read-all-status]');
  const statusText = () => text(status()) ?? '';
  const check = () => all('[data-message-read-all-status] button').find(
    (button) => text(button) === '核对未读数',
  );
  const modalButton = (label) => all('[data-message-read-all-confirm] button').find(
    (button) => text(button) === label,
  );
  const tabs = () => ['private', 'comments', 'forwards', 'notices'].map((name) => text(
    one(`[data-message-tab="${name}"] .message-drawer-badge`),
  ) ?? null);
  const peers = () => [501, 505].map((id) => text(
    one(`[data-message-peer="${id}"] .message-drawer-badge`),
  ) ?? null);
  const equals = (left, right) => JSON.stringify(left) === JSON.stringify(right);
  const initialTabs = ['3', '2', '1', '4'];
  const initialPeers = ['1', '2'];
  const newTabs = ['6', '1', '2', '3'];
  const newPeers = ['4', '2'];
  const zeroTabs = [null, null, null, null];
  const zeroPeers = [null, null];
  let step = 'startup';
  let counter = 0;
  let sequence = 0;
  window.__messageFixtureAck = {};
  const fixture = async (mode = 'read') => {
    const id = ++sequence;
    await emit('message-fixture', { id, mode });
    return wait(() => window.__messageFixtureAck[id], 'fixture did not acknowledge ' + mode);
  };
  const requests = async () => {
    const receipt = await fixture();
    assert(!receipt.blockedWrite, 'unexpected mutation reached sealed fixture');
    const wire = receipt.requests.map(([path]) => path);
    return {
      reads: wire.filter((path) => path === '/api/pl/count').length,
      lists: wire.filter((path) => path === '/api/msg/private/users').length,
      writes: wire.filter((path) => path === '/api/msg/count/clear').length,
    };
  };
  function click(button, detail) {
    const box = button?.getBoundingClientRect();
    assert(button?.isConnected && !button.disabled && box?.width > 0 && box?.height > 0, detail);
    button.focus({ preventScroll: true });
    button.click();
  }
  async function capture(label, dialog = false) {
    step = label;
    const panel = await wait(() => {
      const panel = drawer();
      if (!panel || Math.abs(panel.getBoundingClientRect().right - innerWidth) >= 0.5 ||
          panel.getAnimations().some((a) => a.pending || a.playState === 'running')) return false;
      return panel;
    }, 'drawer animation did not settle');
    const box = panel.getBoundingClientRect();
    assert(box.width <= 387 && box.bottom <= innerHeight && document.documentElement.scrollWidth <= innerWidth,
      'read-all drawer overflowed the actual desktop viewport');
    const visibleDialog = await wait(() => {
      const element = modal();
      if (!dialog) return !element?.open && { closed: true };
      if (!element?.open || getComputedStyle(element).opacity !== '1' ||
          element.getAnimations().some((a) => a.pending || a.playState === 'running')) return false;
      return element;
    }, 'confirmation visibility or animation mismatch');
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const paint = dialog ? {
      dialogs: [{
        open: true, rect: visibleDialog.getBoundingClientRect().toJSON(), opacity: '1',
        background: getComputedStyle(visibleDialog).backgroundColor,
      }],
    } : {
      paintSamples: [{
        x: box.left + 4, y: box.top + 35, color: getComputedStyle(panel).backgroundColor,
      }],
    };
    await emit(`ux_${String(++counter).padStart(2, '0')}_${label}`, {
      label, tabs: tabs(), peers: peers(), dot: dot(),
      readAllDisabled: readAll()?.disabled === true, canCheck: !!check(), dialog,
      statusText: statusText(), counts: await requests(),
      ratio: devicePixelRatio, screenX, screenY, ...paint,
    });
  }
  async function confirmation() {
    click(readAll(), 'expected enabled one-click read-all control');
    await wait(() => modal()?.open && text(modal()).includes('确定清空全部新消息提醒？'),
      'expected official visible confirmation');
    await wait(() => document.activeElement === modalButton('取消'),
      'destructive prompt defaults to Cancel focus');
  }
  try {
    const reopened = window.__cumulusMessageReadAllReopened === true;
    await wait(() => trigger() && !trigger().disabled && one('.track-row') && dot(),
      'authenticated native message count ready');
    assert(equals(await requests(), { reads: 1, lists: 0, writes: 0 }),
      'authenticated background only reads the count once');
    click(trigger(), 'open real message drawer');
    await wait(async () => equals(tabs(), initialTabs) && equals(peers(), initialPeers) &&
      equals(await requests(), { reads: 2, lists: 1, writes: 0 }),
    'visible drawer supplies original four categories and two private badges');

    if (!reopened) {
      await wait(() => readAll() && !readAll().disabled, 'read-all ready with current positive counts');
      await capture('initial');
      await confirmation();
      assert((await requests()).writes === 0, 'opening confirmation sends no write');
      await capture('confirmation', true);
      click(modalButton('取消'), 'dismiss confirmation');
      await wait(() => !modal()?.open && readAll() && !readAll().disabled &&
        document.activeElement === readAll(), 'cancel restored the drawer control and its focus');
      assert(equals(tabs(), initialTabs) && equals(peers(), initialPeers) &&
        (await requests()).writes === 0, 'cancel preserved all unread and sent no write');
      await capture('cancelled');
      await confirmation();
      click(modalButton('确定'), 'accept the explicit confirmation');
      await wait(async () => !modal()?.open && !dot() && equals(tabs(), zeroTabs) &&
        equals(peers(), zeroPeers) && readAll()?.disabled &&
        statusText().includes('已清除全部新消息提醒。') && (await requests()).writes === 1,
      'accepted code 200 clears the four counts and existing rows in place');
      assert(equals(await requests(), { reads: 2, lists: 1, writes: 1 }),
        'confirmed clear does not reread or rewrite message rows');
      await capture('confirmed');

      await fixture('read-all-delivery');
      click(refresh(), 'manually read newly delivered synthetic messages');
      await wait(async () => dot() && equals(tabs(), newTabs) && equals(peers(), newPeers) &&
        !readAll()?.disabled && equals(await requests(), { reads: 3, lists: 2, writes: 1 }),
      'fresh server delivery can become positive again after a successful clear');
      await capture('delivery');

      await confirmation();
      click(modalButton('确定'), 'submit a second explicit user action');
      await wait(async () => !modal()?.open && check() && !check().disabled &&
        statusText().includes('一键全读的结果尚未确认') && (await requests()).writes === 2, 'lost acknowledgement exposes read-only recovery');
      assert(dot() && equals(tabs(), newTabs) && equals(peers(), newPeers) && readAll()?.disabled,
        'lost acknowledgement did not erase any unread badges or permit retry');
      await capture('lost');
    } else {
      await wait(() => check() && !check().disabled && readAll()?.disabled,
        'actual second process recovers the uncertain operation in the same account store');
      await capture('reopened_pending');
      click(check(), 'check current unread counts without replay');
      await wait(async () => check() && !check().disabled &&
        statusText().includes('当前仍无法确认清除结果') &&
        equals(await requests(), { reads: 3, lists: 1, writes: 0 }),
      'explicit positive count read finished and the operation stayed pending');
      assert(dot() && equals(tabs(), initialTabs) && equals(peers(), initialPeers),
        'positive readback preserves server unread and observed private rows');
      await capture('checked_positive');

      await fixture('read-all-zero');
      click(check(), 'verify a second explicit read-only result');
      await wait(async () => !check() && !dot() && equals(tabs(), zeroTabs) &&
        statusText().includes('未读提醒总数已归零，已结束核对。') &&
        equals(await requests(), { reads: 4, lists: 1, writes: 0 }),
      'four authoritative zeros resolve the old operation without write or list refresh');
      assert(readAll()?.disabled, 'nothing positive remains to prepare another clear');
      assert(equals(peers(), initialPeers), 'aggregate readback did not infer a per-row acknowledgement');
      await capture('checked_zero');
    }
    await emit('complete', { ok: true, reopened, count: counter, realAccountWrites: 0, realMusicRequests: 0 });
  } catch (error) {
    await emit('failed', { step, message: error?.message ?? String(error), tabs: tabs(), peers: peers(),
      statusText: statusText() });
  }
})();
