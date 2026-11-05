// Actual settings component and native IPC; the fixture never contacts a service.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (pass, message) => { if (!pass) throw Error(message); };
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, extra = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...extra },
  });
  async function wait(check, description) {
    for (let attempt = 0; attempt < 170; attempt++) {
      const value = await check();
      if (value) return value;
      await sleep(65);
    }
    throw Error(description);
  }
  const cache = () => one('[data-playback-cache-settings]');
  const range = () => cache()?.querySelector('[data-playback-cache-capacity]');
  const clear = () => cache()?.querySelector('[data-playback-cache-clear]');
  const note = () => cache()?.querySelector('[data-playback-cache-status]')?.textContent.trim();
  const state = () => invoke('playback_cache_state');
  const userAction = (node) => {
    assert(node && !node.disabled, 'enabled UI control');
    node.focus({ preventScroll: true });
    node.click();
  };
  const GIB = 1024 ** 3;
  const INITIAL = GIB;
  const SELECTED = 1.5 * GIB;
  let stageCount = 0;
  async function capture(name, extra = {}) {
    cache().scrollIntoView({ block: 'center', behavior: 'instant' });
    await sleep(350);
    await emit(`ux_${String(++stageCount).padStart(2, '0')}_${name}`, {
      captureHandshake: true, ...extra,
    });
  }
  function assertView(view, capacity) {
    assert(view?.capacityBytes === capacity && view.minCapacityBytes === GIB / 2 &&
      view.maxCapacityBytes === 10 * GIB && view.capacityStepBytes === GIB / 2,
      'native range and selected capacity');
    assert(view.occupiedBytes === 0 && view.reclaimableBytes === 0 &&
      view.reservedBytes === 0 && !view.overCapacity, 'new session has no audio byte cache');
    assert(cache().querySelector('[data-playback-cache-directory]')?.textContent.trim() === view.directory &&
      ['occupied', 'reclaimable', 'reserved'].every((name) =>
        cache().querySelector(`[data-playback-cache-${name}]`)?.dataset.bytes === '0'),
    'directory and counts are shown from the native snapshot');
  }
  async function reject(command, fields, code) {
    try {
      await invoke(command, fields);
    } catch (error) {
      assert(error.code === code, `unexpected public error: ${JSON.stringify(error)}`);
      return;
    }
    throw Error('invalid cache mutation was accepted');
  }
  try {
    await wait(() => one('.track-row[data-track-id="1"]') &&
      one('.playerbar button[aria-label="列表循环"]')?.disabled === false, 'desktop shell is loaded');
    let stockPlayed = false;
    if (window.__cacheBlocked) {
      const account = await invoke('account_snapshot');
      const row = one('.track-row[data-track-id="1"]');
      row.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
      stockPlayed = !!(await wait(async () => {
        const player = await invoke('player_snapshot', { epoch: account.epoch });
        return player.state === 'playing' && player.track?.id === '1' && player.position > 0.15;
      }, 'stock libmpv playback continues despite unavailable cache'));
    }
    userAction(one('button[title="设置"]'));
    await wait(cache, 'settings section is mounted');
    if (window.__cacheBlocked) {
      await wait(() => cache()?.textContent.includes('当前设备的在线播放缓存不可用'), 'unavailable status');
      assert(await state() === null && !range() && !clear(), 'no cache mutation affordances');
      await capture('unavailable', { stockPlayed });
    } else {
      await wait(() => clear() && !clear().disabled && range(), 'available native settings');
      const original = await state();
      assertView(original, window.__cacheReopen ? SELECTED : INITIAL);
      assert(Number(range().value) === original.capacityBytes, 'slider reflects saved native capacity');
      if (window.__cacheReopen) {
        await reject('playback_cache_clear', { expectedRevision: window.__oldCacheRevision },
          'playback_cache_changed');
        assert((await state()).revision === original.revision, 'previous process revision could not mutate');
        await capture('reopened', { view: original });
      } else {
        await capture('initial', { view: original });
        range().focus();
        range().value = SELECTED;
        range().dispatchEvent(new Event('input', { bubbles: true }));
        await wait(() => range()?.getAttribute('aria-valuetext')?.includes('1.5'), 'range preview is announced');
        const preview = await state();
        assert(preview.capacityBytes === INITIAL && preview.revision === original.revision,
          'drag preview does not mutate native preference');
        range().dispatchEvent(new Event('change', { bubbles: true }));
        await wait(() => note() === '设置已更新', 'capacity commit acknowledged');
        const updated = await state();
        assertView(updated, SELECTED);
        assert(updated.revision !== original.revision, 'commit rotates opaque revision');
        await reject('playback_cache_set_capacity', { expectedRevision: updated.revision, capacityBytes: 100 },
          'playback_cache_capacity');
        await reject('playback_cache_clear', { expectedRevision: original.revision }, 'playback_cache_changed');
        assert((await state()).revision === updated.revision, 'rejected commands do not mutate settings');
        await capture('capacity', { view: updated });
        userAction(clear());
        await wait(() => note() === '缓存已清除！', 'explicit clear acknowledged');
        const cleared = await state();
        assertView(cleared, SELECTED);
        assert(cleared.revision !== updated.revision, 'clear retires the prior cache revision');
        await capture('cleared', { view: cleared });
      }
    }
    await emit('complete', { ok: true, stageCount, stockPlayed, realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', { error: error?.message ?? String(error), stack: error?.stack,
      stageCount, body: document.body.innerText.slice(0, 1800) });
  }
})();
