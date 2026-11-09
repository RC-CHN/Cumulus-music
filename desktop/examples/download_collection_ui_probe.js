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
    assert(item && !item.disabled && item.getAttribute('aria-disabled') !== 'true', 'enabled control: ' + (item?.getAttribute('aria-label') || item?.textContent?.trim()));
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
    if (window.__downloadBatchCollectProof) {
      await seed('seed_collection_batch');
      click(button('下载管理'));
      await wait(ready, 'completed Downloads');
      click(await wait(() => button('更多下载操作'), 'download menu'));
      const actions = await wait(() => one('.action-menu')?.style.visibility === 'visible' &&
        one('.action-menu'), 'download menu visible');
      click(button('批量操作', actions));
      const batch = await wait(() => one('.download-batch-page:not(.page-enter-active)')?.querySelector('[data-batch-index="0"]') &&
        one('.download-batch-page'), 'batch inventory');
      const records = [...batch.querySelectorAll('[data-batch-index]')];
      const id = (row) => row.querySelector('.download-track-name').textContent.match(/(\d+) ·/)[1].replace(/^0+/, '');
      assert(records.map(id).join(',') === '1,2,3,4,5,6,7', 'exact native original order');
      // Select the known rights vector in a different order. The dispatch must
      // retain original native list order, not click order.
      for (const index of [3, 1, 2, 0]) click(records[index]);
      const favorite = await wait(() => {
        const action = button('收藏', batch);
        return action && !action.disabled &&
          one('.batch-select-label', batch)?.textContent.includes('已选 4 首歌曲') && action;
      }, 'four selected records and enabled Favorite');
      const openBatchPicker = async () => {
        click(favorite);
        const picker = await wait(dialog, 'native batch picker');
        await wait(() => destination() && destination().getAttribute('aria-disabled') !== 'true',
          'authoritative ordinary destination');
        const summary = one('.collect-selection-summary', picker).textContent.replace(/\s+/g, ' ').trim();
        assert(summary === '已选 4 项下载，将收藏 2 首歌曲。2 项歌曲无法收藏，已略过。', 'filtered native summary: ' + summary);
        assert(!one('[data-playlist-id="1"]', picker) && !button('创建新歌单', picker) &&
          !button('我喜欢的音乐', picker), 'only ordinary owned destination lane');
        return { picker, summary };
      };
      const first = await openBatchPicker();
      await capture('batch_picker', { summary: first.summary });
      click(button('关闭歌单操作', first.picker));
      await wait(() => !one('dialog[data-download-collect]') && document.activeElement === favorite,
        'cancel returns to exact Favorite button');
      await capture('batch_cancel', { exactReturnFocus: true });
      await server('uncertain');
      await openBatchPicker();
      click(destination());
      const retry = await wait(() => dialog()?.getAttribute('aria-busy') === 'false' &&
        button('核对结果', dialog()), 'uncertain batch offers read-only recovery');
      assert(destination().getAttribute('aria-disabled') === 'true' &&
        one('.mutation-result', dialog()).dataset.phase === 'uncertain', 'pending destination cannot write again');
      await capture('batch_uncertain');
      await server('success');
      click(retry);
      await wait(() => !one('dialog[data-download-collect]') && document.activeElement === favorite,
        'explicit verified recovery closes and returns focus');
      await capture('batch_reconciled');
      await emit('complete', {
        ok: true, batchCollect: true, stageCount: count,
        realMusicRequests: 0, realAccountWrites: 0,
      });
      return;
    }
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
