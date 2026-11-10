// Linux XTest clicks through the ordinary song menu and real Tauri Start.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const assert = (condition, label) => { if (!condition) throw Error(label); };
  const wait = async (read, label) => {
    for (let i = 0; i < 180; i++) {
      const result = await read();
      if (result) return result;
      await new Promise((done) => setTimeout(done, 60));
    }
    throw Error(label);
  };
  const invoke = (...args) =>
    (window.__CUMULUS_PROOF_INVOKE__ ?? window.__TAURI_INTERNALS__.invoke)(...args);
  const emit = (stage, info = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...info } });
  const button = (label, root = document) => [...root.querySelectorAll('button')].find(
    (node) => node.getAttribute('aria-label') === label || node.textContent.trim() === label);
  const row = (id) => one(`.search-song-panel [data-track-id="${id}"] .track-row-title-text`);
  const dialog = () => one('dialog.download-dialog[open]:not([inert]):not(.modal-enter-active)');
  const menuItem = () => [...document.querySelectorAll('[data-menu-key="download"]')].find(
    (node) => node.getClientRects().length && !node.closest('[inert]'));
  const back = () => one('button[aria-label="返回上一页"]');
  let epoch, old, fresh, original, core, step = 0, trustedInputs = 0, stage = 'setup';
  const actions = [];
  const downloads = (tab) => invoke('downloads', { epoch, query: {
    tab, category:'song', keyword:'', sort:'completed', descending:false, offset:0, limit:10,
  }});
  async function inventory() {
    const [active, complete] = await Promise.all([downloads('active'), downloads('completed')]);
    const project = (task) => ({
      id:task.id, song:task.track.id, status:task.status, bytes:task.bytes, quality:task.requestedQuality,
    });
    return {
      active:active.tasks.map(project), complete:complete.tasks.map(project),
      counts:[active.activeCount, complete.completedCount],
    };
  }
  function samples(element) {
    assert(element && getComputedStyle(element).opacity === '1', 'settled live dialog');
    const color = getComputedStyle(element).backgroundColor, r = element.getBoundingClientRect();
    assert(/^rgb\(\d+, \d+, \d+\)$/.test(color), 'opaque dialog padding');
    return [[r.left+10,r.top+46],[r.right-10,r.bottom-32]].map(([x,y]) => {
      assert(element.contains(document.elementFromPoint(x,y)), 'sample reaches the live dialog');
      return {x,y,color};
    });
  }
  async function capture(label, extra = {}) {
    stage = label;
    const records = await inventory();
    const selectedTab = one('.downloads-page [role="tab"][aria-selected="true"]')?.id ?? null;
    const notice = dialog()?.querySelector('[role="status"]')?.textContent.trim() ?? null;
    const heading = dialog()?.querySelector('h2')?.textContent.trim() ?? null;
    await emit(`ux_${String(++step).padStart(2,'0')}_${label}`, {
      records, actions, old, fresh, selectedTab, notice, heading, trustedInputs, screenX, screenY,
      ratio:devicePixelRatio, ...extra,
    });
  }
  async function input(label, node, mouseButton = 1, paintSamples = []) {
    assert(node?.isConnected && !node.disabled, 'enabled actual control');
    const r = node.getBoundingClientRect(), x = r.left + r.width/2, y = r.top + r.height/2;
    assert(node.contains(document.elementFromPoint(x,y)), 'pointer hits intended control');
    let observed = false;
    const watch = (event) => {
      if (event.isTrusted && event.button === mouseButton - 1 && node.contains(event.target))
        observed = true;
    };
    document.addEventListener('pointerdown', watch, true);
    try {
      await capture(label, {paintSamples, pointer:{
        x:Math.floor((screenX+x)*devicePixelRatio), y:Math.floor((screenY+y)*devicePixelRatio),
        button:mouseButton, ctrl:false,
      }});
      await wait(() => observed, 'trusted X11 pointer');
      trustedInputs++;
    } finally {
      document.removeEventListener('pointerdown', watch, true);
    }
  }
  function click(node) {
    assert(node?.isConnected && !node.disabled, 'setup control available');
    node.click();
  }
  async function searchReady(id) {
    await wait(() => one('.search-results-page h1')?.textContent === '合成连续单曲' &&
      !one('.page-enter-active,.page-leave-active'), 'search route settled');
    const songs = one('[data-category="songs"]');
    if (songs?.getAttribute('aria-selected') !== 'true') click(songs);
    await wait(() => row(id) && one('#search-result-panel')?.getAttribute('aria-busy') === 'false' &&
      !one('.search-results-enter-active,.search-results-leave-active'), 'song rows settled');
  }
  async function ordinary(id, name) {
    await searchReady(id);
    await input(name+'_row', row(id), 3);
    const entry = await wait(() => {
      const item = menuItem(), menu = item?.closest('[role="menu"]');
      return menu && !item.disabled && getComputedStyle(menu).opacity === '1' && item;
    }, 'single ordinary download menu');
    await input(name+'_menu', entry);
    const start = await wait(() => {
      const modal = dialog(), submit = modal && button('开始下载', modal);
      return submit && !submit.disabled && modal.querySelector('h2')?.textContent === '下载 1 首歌曲' &&
        modal.querySelector('.download-selection')?.textContent.includes(`合成连续单曲 ${id}`) &&
        modal.querySelector('[aria-label="下载音质"]')?.value === 'exhigh' && submit;
    }, 'ordinary one-song Start with actual configured preferences');
    await input(name+'_start', start);
  }
  const activeReady = () => one('#download-tab-active[aria-selected="true"]') &&
    one(`[data-download-task="${old?.id}"]`) && !one('dialog[open],.page-enter-active,.page-leave-active');

  try {
    await wait(() => one('.track-row') && one('.sidebar'), 'signed-in fixture shell');
    await emit('resize', {width:1120,height:760});
    await wait(() => innerWidth === 1120 && innerHeight === 760, 'wide viewport');
    ({epoch} = await invoke('account_snapshot'));
    await emit('search-group-download-configure');
    await wait(() => window.__groupDownloadConfigured?.ok, 'sealed native file and preferences');
    ({old} = window.__groupDownloadConfigured);
    assert(old?.bytes === 2048, 'real paused prefix');
    click(button('下载管理', one('.sidebar')));
    await wait(() => one('#download-tab-active') && button('打开目录')?.disabled === false,
      'actual downloads model has read configured directory');
    click(one('#download-tab-active'));
    await wait(activeReady, 'persisted paused row is visible');
    window.__searchFixtureAck = false;
    await emit('search-fixture', {mode:'song_waterfall_group_next'});
    await wait(() => window.__searchFixtureAck, 'synthetic signed search data');
    const search = one('[aria-label="搜索音乐"]');
    search.value = '合成连续单曲';
    search.dispatchEvent(new Event('input',{bubbles:true}));
    search.form.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));

    const provides = one('#app').__vue_app__._context.provides;
    core = provides[Reflect.ownKeys(provides).find(
      (key) => typeof key === 'symbol' && key.description === 'core-client')];
    original = core.downloadCommand;
    core.downloadCommand = async (owner, command) => {
      const reply = await original.call(core, owner, command);
      actions.push({command, reply});
      return reply;
    };

    await ordinary('104','paused');
    const paused = await wait(() => {
      const modal = dialog(), manage = modal && button('打开下载管理', modal);
      return actions.length === 1 && manage && !manage.disabled &&
        modal.querySelector('[role="status"]')?.textContent.includes('可按原设置继续') && manage;
    }, 'paused existing task explains the options conflict');
    await input('paused_notice', paused, 1, samples(dialog()));
    await wait(activeReady, 'Manage opens active tab with exact paused task');
    await input('active_manager', back());

    await ordinary('101','fresh');
    const completed = await wait(async () => {
      const view = await inventory();
      return actions.length === 2 && activeReady() && view.complete[0]?.song === '101' && view.complete[0];
    }, 'fresh ordinary song publishes and opens active downloads');
    fresh = completed.id;
    await input('fresh_completed', back());

    await ordinary('101','complete');
    const done = await wait(() => {
      const modal = dialog(), manage = modal && button('打开下载管理', modal);
      return actions.length === 3 && manage && !manage.disabled &&
        modal.querySelector('[role="status"]')?.textContent.trim() === '资源已下载' && manage;
    }, 'completed song is acknowledged without new Start');
    await input('completed_notice', done, 1, samples(dialog()));
    await wait(() => one('#download-tab-song[aria-selected="true"]') &&
      one(`[data-download-task="${fresh}"]`) && !one('dialog[open],.page-enter-active,.page-leave-active'),
    'Manage opens completed song tab with exact file');
    await capture('completed_manager');
    core.downloadCommand = original;
    await emit('complete', {ok:true,stageCount:step,realAccountWrites:0,realMusicRequests:0});
  } catch (error) {
    if (core && original) core.downloadCommand = original;
    await emit('failed', {stageCount:step,step:stage,error:String(error),actions,
      text:document.body.innerText.slice(0,2500)});
  }
})();
