// Real system-liked collection from a completed download, with an ambiguous response.
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
  const liked = () => dialog() && button('我喜欢的音乐', dialog());
  const openPicker = async (task) => {
    const actions = await openMenu(task);
    click(button('收藏', actions));
    const picker = await wait(dialog, 'scoped download picker');
    await wait(() => liked()?.getAttribute('aria-disabled') === 'false', 'verified system destination');
    assert(!liked().hasAttribute('data-playlist-id'), 'system choice exposes no raw target ID');
    return picker;
  };
  let count = 0, current = 'startup';
  const capture = async (label, data = {}) => {
    current = label;
    await wait(() => !one('.page-enter-active,.page-leave-active'), 'settled route');
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
    const unlike = () => [...document.querySelectorAll('.track-row button')].find(
      (b) => b.getAttribute('aria-label')?.startsWith('取消喜欢 合成歌曲 1 ·'),
    );
    assert(!unlike(), 'ordinary song initially not liked');
    await seed('seed_collection');
    click(button('下载管理'));
    await wait(ready, 'completed Downloads');
    const provides = one('#app').__vue_app__._context.provides;
    const client = provides[Reflect.ownKeys(provides).find(
      (key) => typeof key === 'symbol' && key.description === 'core-client',
    )];
    const operations = [];
    const collect = client.downloadCollectLiked;
    client.downloadCollectLiked = async (...args) => {
      const result = await collect(...args);
      operations.push({ reconcile: args[3], operation: result.value.operationId, phase: result.value.phase });
      return result;
    };
    const epoch = (await client.accountSnapshot()).epoch;
    const inventory = await client.downloads(epoch, {
      tab: 'completed', category: 'song', keyword: '', sort: 'none',
      descending: false, offset: 0, limit: 20,
    });
    const track = inventory.tasks.find((task) => task.track.id === '1');
    const picker = await openPicker(track);
    await capture('verified_choice');
    click(button('关闭歌单操作', picker));
    await wait(() => !dialog(), 'cancel does not choose a target');
    await capture('canceled');
    await openPicker(track);
    click(liked());
    const pending = () => dialog() &&
      dialog().getAttribute('aria-busy') === 'false' &&
      one('[data-phase="uncertain"]', dialog()) &&
      button('核对结果', dialog());
    await wait(() => operations[0]?.phase === 'uncertain' && pending(), 'lost response still pending');
    assert(liked().getAttribute('aria-disabled') === 'true', 'pending locks all destinations');
    await capture('one_sided');
    click(pending());
    await wait(() => operations[1]?.phase === 'uncertain' && pending(),
      'readback with only one agreeing source remains pending');
    await capture('still_pending');
    await emit('download-liked-fixture');
    await wait(() => window.__downloadLikedFixtureAck, 'synthetic second read source settles');
    click(pending());
    await wait(() => operations[2]?.phase === 'confirmed' && !dialog(), 'confirmed readback closes picker');
    assert(operations.map((value) => value.reconcile).join(',') === 'false,true,true' &&
      operations.every((value) => value.operation === operations[0].operation),
      'only one explicit add and two readbacks of the same operation');
    await capture('reconciled');
    click(button('返回上一页'));
    await wait(unlike, 'current account heart reflects only dual confirmed song');
    await capture('liked_heart');
    await emit('complete', { ok: true, stageCount: count, realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', {
      ok: false, stageCount: count, lastStage: current,
      error: error?.message ?? String(error), stack: error?.stack ?? '',
    });
  }
})();
