// Native quota projection and cached official membership on the real cloud page.
(async () => {
  const q = (selector, root = document) => root.querySelector(selector);
  const page = () => q('.cloud-page');
  const status = () => q('[data-cloud-capacity-status]', page());
  const sleep = (ms) => new Promise((done) => setTimeout(done, ms));
  const wait = async (check, why) => {
    for (let i = 0; i < 250; i++) {
      const value = check();
      if (value) return value;
      await sleep(50);
    }
    throw Error(why);
  };
  const assert = (value, why) => { if (!value) throw Error(why); };
  const emit = (payload) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'mini-proof', payload,
  });
  window.__miniAck = {};
  let serial = 0, count = 0, stage = 'startup';
  async function configure(fields) {
    const id = ++serial;
    await emit({ kind: 'cloud-config', id, ...fields });
    const ack = await wait(() => window.__miniAck[id], 'configure synthetic quota');
    delete window.__miniAck[id];
    assert(ack.ok, 'synthetic quota setup');
  }
  const button = (label, root = document) => [...root.querySelectorAll('button')].find(
    (b) => b.getAttribute('aria-label') === label || b.textContent.trim() === label,
  );
  function click(b) {
    assert(b && !b.disabled, 'enabled button');
    b.focus({ preventScroll: true });
    b.click();
  }
  let client, epoch, observed, remoteReads = 0;
  const ready = () => page()?.dataset.cloudEpoch === epoch &&
    page().dataset.cloudReady === 'true' && page().dataset.cloudLoading === 'false';
  async function refresh() {
    const before = remoteReads;
    click(await wait(() => {
      const b = button('刷新云盘', page());
      return b && !b.disabled && b;
    }, 'refresh action'));
    await wait(() => remoteReads > before && ready(), 'native cloud refresh and view committed');
  }
  function readStatus() {
    const node = status();
    return node ? { kind: node.dataset.cloudCapacityStatus, text: node.textContent.trim() } : null;
  }
  async function capture(name, expected) {
    stage = name;
    await wait(() => ready() && !q('.page-enter-active,.page-leave-active') &&
      JSON.stringify(readStatus()) === JSON.stringify(expected), name + ' current status');
    assert(JSON.stringify(observed?.capacityStatus ?? null) === JSON.stringify(expected), 'native status matches DOM');
    assert(button('扩容', page()), 'only existing capacity handoff');
    assert(!button('立即续费', page()) && !button('立即购买', page()), 'no commerce action');
    const label = 'ux_' + String(++count).padStart(2, '0') + '_' + name;
    await emit({ kind: 'snapshot', stage: label, status: readStatus(),
      nativeStatus: observed?.capacityStatus ?? null, fullLibraryCount: observed.uploadedCount,
      visibleRows: page().querySelectorAll('[data-cloud-id]').length });
    await wait(() => window.__miniRecorded === label, 'capture acknowledged');
  }
  try {
    await wait(() => q('.track-row'), 'authenticated initial route');
    const provides = q('#app').__vue_app__._context.provides;
    client = provides[Reflect.ownKeys(provides).find(
      (key) => typeof key === 'symbol' && key.description === 'core-client',
    )];
    epoch = (await client.accountSnapshot()).epoch;
    const original = client.cloudPage;
    client.cloudPage = async (...args) => {
      const result = await original(...args);
      observed = result;
      if (args[2]) remoteReads++;
      return result;
    };
    click(button('音乐云盘'));
    await wait(() => observed && ready(), 'ready cloud route');
    const known = (kind, text) => ({ kind, text });
    const full = known('full', '网盘已满，请清除部分文件，才能继续上传');
    const fullOptions = known('full', '网盘已满，升级SVIP或购买容量后可继续上传歌曲');
    await capture('unknown_member', null);
    await configure({ size: 1073741824 });
    await refresh();
    await capture('unknown_member_full', full);
    await configure({ size: 123456789, capacityMembership: 1 });
    await client.memberRights(epoch, true);
    await refresh();
    await capture('active', known('svip_active', '黑胶SVIP尊享6TB超大空间'));
    await configure({ size: 1073741824 });
    await refresh();
    await capture('active_full', full);
    await configure({ size: 123456789, capacityMembership: 2 });
    await client.memberRights(epoch, true);
    await refresh();
    await capture('expiring', known('svip_expiring', '黑胶SVIP6TB超大空间即将到期'));
    await configure({ capacityMembership: 3 });
    await client.memberRights(epoch, true);
    await refresh();
    await capture('positive_expiry', null);
    await configure({ size: 1073741824 });
    await refresh();
    await capture('non_plus_full', fullOptions);
    await configure({ size: 123456789, capacityMembership: 4 });
    await client.memberRights(epoch, true);
    await refresh();
    const offer = known('svip_offer', '黑胶SVIP6TB超大空间已失效');
    await capture('known_zero_expiry', offer);
    const input = q('[aria-label="搜索云盘歌曲"]', page());
    input.value = 'No matching synthetic song';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    input.form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await wait(() => ready() && observed.total === 0, 'filtered rows');
    await capture('filtered_keeps_status', offer);
    await emit({ kind: 'complete', stage: 'complete', ok: true, stageCount: count,
      realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit({ kind: 'failed', stage: 'failed', error: error?.message ?? String(error), count, last: stage });
  }
})();
