// Real Downloads menu, scoped native picker and durable mutation recovery.
(async () => {
  const one = (selector, root = document) => root.querySelector(selector);
  const dialog = () => one('dialog[data-download-collect][open]');
  const menu = () => one('.action-menu[aria-label="下载资源操作"]');
  const button = (text, root = document) =>
    [...root.querySelectorAll('button')].find(
      (item) => item.getAttribute('aria-label') === text || item.textContent.trim() === text,
    );
  const sleep = (ms) => new Promise((done) => setTimeout(done, ms));
  const assert = (ok, why) => { if (!ok) throw Error(why); };
  const wait = async (check, why, attempts = 260) => {
    for (let i = 0; i < attempts; i++) {
      const value = await check();
      if (value) return value;
      await sleep(50);
    }
    throw Error(why);
  };
  const emit = (stage, data = {}) =>
    window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
      event: 'ux-proof', payload: { stage, ...data },
    });
  const click = (item) => {
    assert(item && !item.disabled && item.getAttribute('aria-disabled') !== 'true', 'enabled control');
    item.focus({ preventScroll: true });
    item.click();
  };
  const control = async (stage, ack, mode) => {
    window[ack] = null;
    await emit(stage, { mode });
    return wait(() => window[ack], 'fixture ' + mode, 1200);
  };
  const seed = async (mode) => {
    const result = await control('download-page-fixture', '__downloadPageAck', mode);
    assert(result.ok, result.message);
  };
  const server = (mode) => control('collect-fixture', '__collectFixtureAck', mode);
  const ready = () => one('.downloads-page') && !one(
    '#download-content[aria-busy="true"],.download-section-enter-active,.download-section-leave-active,.page-enter-active,.page-leave-active',
  );
  const row = (task) => one('[data-download-task="' + CSS.escape(task.id) + '"]');
  const openMenu = async (task) => {
    const r = await wait(() => row(task), 'download row ' + task.track.id);
    r.scrollIntoView({ block: 'nearest' });
    await sleep(140);
    click(one('[aria-label^="管理下载"]', r));
    await wait(() => menu()?.style.visibility === 'visible' &&
      menu().contains(document.activeElement), 'focused download menu');
    return menu();
  };
  const closeMenu = async () => {
    menu().dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await wait(() => !menu(), 'menu closed');
  };
  const destination = () => one('[data-playlist-id="2"]', dialog() ?? document);
  const openPicker = async (task) => {
    const actions = await openMenu(task);
    click(button('收藏', actions));
    const picker = await wait(dialog, 'scoped download picker');
    await wait(() => destination() && destination().getAttribute("aria-disabled") !== "true", 'authoritative owned destination');
    assert(!one('[data-playlist-id="1"]', picker) &&
      !button('创建新歌单', picker), 'system liked and create are not authorized in this picker');
    return picker;
  };
  let count = 0, current = 'startup';
  const capture = async (label, data = {}) => {
    current = label;
    await wait(() => {
      const picker = one('dialog[data-download-collect]');
      return !picker || (
        picker.open && !picker.inert && getComputedStyle(picker).opacity === '1' &&
        picker.getAnimations().every((animation) => animation.playState !== 'running') &&
        !one('.collect-step-enter-active,.collect-step-leave-active', picker)
      );
    }, 'settled picker');
    await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));
    const stage = 'ux_' + String(++count).padStart(2, '0') + '_' + label;
    window.__downloadCollectionRecorded = '';
    await emit(stage, data);
    await wait(() => window.__downloadCollectionRecorded === stage, 'native capture ' + label);
  };
  try {
    await wait(() => one('.track-row'), 'authenticated initial inventory');
    await seed('seed_collection');
    await seed('cloud_comment_targets');
    click(button('下载管理'));
    await wait(ready, 'completed Downloads');
    const provides = one('#app').__vue_app__._context.provides;
    const client = provides[Reflect.ownKeys(provides).find(
      (key) => typeof key === 'symbol' && key.description === 'core-client',
    )];
    const epoch = (await client.accountSnapshot()).epoch;
    const inventory = await client.downloads(epoch, {
      tab: 'completed', category: 'song', keyword: '', sort: 'none',
      descending: false, offset: 0, limit: 20,
    });
    const nativeInventory = inventory.tasks.map((task) => ({
      id: task.track.id, source: task.source,
    })).sort((left, right) => Number(left.id) - Number(right.id));
    assert(inventory.total === 7 && nativeInventory.length === 7 &&
      nativeInventory.every(({ id, source }, index) =>
        id === String(index + 1) && source === (index < 4 ? 'song' : 'cloud')),
      'only the seven exact completed native fixture rows are present');
    const [ordinary, matched, privateCloud, legacyCloud] = ['1', '5', '6', '7']
      .map((id) => inventory.tasks.find((task) => task.track.id === id));
    assert(ordinary.collectionAvailable && matched.collectionAvailable &&
      !privateCloud.collectionAvailable && !legacyCloud.collectionAvailable &&
      matched.track.id === '5' && matched.commentTrackId === '9010',
      'native scope distinguishes regular and matched from private/legacy');
    const first = await openPicker(ordinary);
    await capture('ordinary_picker', { song: ordinary.track.id });
    click(button('关闭歌单操作', first));
    await wait(() => !dialog(), 'cancel before selecting a destination');
    await wait(() => document.activeElement === row(ordinary)?.querySelector('[aria-label^="管理下载"]'),
      'cancel restores focus to the exact download row');
    await capture('canceled_before_write', { exactReturnFocus: true });
    await openPicker(ordinary);
    click(destination());
    await wait(() => !dialog(), 'confirmed ordinary collection closes');
    await capture('ordinary_confirmed');
    for (const [task, label] of [[privateCloud, 'private_hidden'], [legacyCloud, 'legacy_hidden']]) {
      const actions = await openMenu(task);
      assert(!button('收藏', actions), 'no unsupported collection action ' + label);
      await capture(label, { song: task.track.id });
      await closeMenu();
    }
    await server('uncertain');
    await openPicker(matched);
    click(destination());
    const retry = await wait(() => {
      const picker = dialog();
      if (!picker || picker.getAttribute('aria-busy') === 'true') return;
      return button('核对结果', picker);
    }, 'lost reply retains explicit readback action');
    assert(destination().getAttribute("aria-disabled") === "true" && one('.mutation-result', dialog()).dataset.phase === 'uncertain',
      'pending result does not allow a second add');
    assert(!one('.collect-working', dialog()), 'pending result is not shown as an active write');
    await capture('matched_uncertain', { song: matched.track.id });
    await server('success');
    click(retry);
    await wait(() => !dialog(), 'explicit read-only confirmation closes');
    await capture('matched_reconciled');
    await emit('complete', {
      ok: true, stageCount: count, nativeInventory, realMusicRequests: 0, realAccountWrites: 0,
    });
  } catch (error) {
    await emit('failed', {
      ok: false, stageCount: count, lastStage: current,
      error: error?.message ?? String(error), stack: error?.stack ?? '',
    });
  }
})();
