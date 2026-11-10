// Trusted Linux pointer selection, explicit purchased quality and native admission.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
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
  const bar = () => one('.playerbar');
  const button = (label, root = document) => [...root.querySelectorAll('button')].find(
    (node) => node.getAttribute('aria-label') === label || node.textContent.trim() === label);
  const row = (id) => one(`.search-song-panel .track-row[data-track-id="${id}"]`);
  const title = (id) => row(id)?.querySelector('.track-row-title-text');
  const download = () => all('[data-menu-key="download"]').find(
    (node) => node.getClientRects().length && !node.closest('[inert]'));
  const selection = () => all('.search-song-panel .track-row[aria-selected="true"]')
    .map((node) => node.dataset.trackId).join(',');
  let epoch, before, stage = 'setup', step = 0;
  const actions = [];
  const state = () => invoke('player_snapshot', { epoch });
  const downloads = (tab) => invoke('downloads', { epoch, query: {
    tab, category: 'song', keyword: '', sort: 'completed', descending: false, offset: 0, limit: 10,
  }});
  async function capture(label, extra = {}) {
    stage = label;
    const [player, queue] = await Promise.all([state(), invoke('player_queue', { epoch })]);
    assert(player.state === 'paused' && player.playId === before.playId &&
      player.entryId === before.entryId && player.position === before.position &&
      player.queueRevision === before.queueRevision && queue.current === before.entryId &&
      queue.tracks.length === 1 && queue.tracks[0].track.id === '701', 'independent paused playback');
    await emit(`ux_${String(++step).padStart(2,'0')}_${label}`,
      { player, queue, actions, ...extra });
  }
  async function input(label, node, mouseButton = 1, ctrl = false, extra = {}) {
    assert(node?.isConnected && !node.disabled, 'actual enabled target');
    const r = node.getBoundingClientRect(), x = r.left + r.width / 2, y = r.top + r.height / 2;
    assert(node.contains(document.elementFromPoint(x,y)), 'pointer reaches intended control');
    let trusted = false;
    const observed = (event) => {
      if (event.isTrusted && event.button === mouseButton - 1 &&
        event.ctrlKey === ctrl && node.contains(event.target)) trusted = true;
    };
    document.addEventListener('pointerdown', observed, true);
    try {
      await capture(label, { ...extra, pointer: { x: Math.floor((screenX+x)*devicePixelRatio),
        y: Math.floor((screenY+y)*devicePixelRatio), button:mouseButton, ctrl }});
      await wait(() => trusted, 'trusted X11 pointer and modifier');
    } finally {
      document.removeEventListener('pointerdown', observed, true);
    }
  }
  function afterPaint() {
    return new Promise((resolve, reject) => {
      let frame;
      const timeout = setTimeout(() => {
        cancelAnimationFrame(frame);
        reject(Error('quality selection did not get two paint frames'));
      }, 2500);
      frame = requestAnimationFrame(() => {
        frame = requestAnimationFrame(() => {
          clearTimeout(timeout);
          resolve();
        });
      });
    });
  }
  function click(node) {
    assert(node?.isConnected && !node.disabled,'enabled action');
    node.click();
  }

  try {
    await wait(() => one('.track-row') && button('列表循环', bar() ?? document)?.disabled === false,
      'authenticated initial player ready');
    await emit('resize', { width:1120, height:760 });
    await wait(() => innerWidth === 1120 && innerHeight === 760, 'wide viewport');
    ({epoch} = await invoke('account_snapshot'));
    await invoke('liked_ids');
    await emit('search-group-download-configure');
    await wait(() => window.__groupDownloadConfigured?.ok, 'native isolated download settings');
    await invoke('player_action', {epoch, action:{kind:'mode',mode:'single_loop'}});
    await invoke('play_selection', {epoch,selection:{kind:'songs',ids:['701'],index:0,quality:'standard'}});
    await wait(() => bar()?.dataset.state === 'playing' && Number(bar().dataset.position) > .2,
      'real paused fixture source advances before pause');
    click(button('暂停',bar()));
    before = await wait(async () => {
      const view = await state();
      return view.state === 'paused' && view.position > .2 && view;
    }, 'paused native position captured');
    assert((await downloads('active')).activeCount === 0, 'no earlier task');
    window.__searchFixtureAck = false;
    await emit('search-fixture',{mode:'song_waterfall_group_next'});
    await wait(() => window.__searchFixtureAck, 'search fixture selected');
    const search = one('[aria-label="搜索音乐"]');
    search.value = '合成连续单曲';
    search.dispatchEvent(new Event('input',{bubbles:true}));
    search.form.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));
    await wait(() => one('.search-results-page h1')?.textContent === '合成连续单曲' &&
      one('#search-result-panel')?.getAttribute('aria-busy') === 'false' &&
      !one('.page-enter-active,.page-leave-active'), 'initial search route');
    click(one('[data-category="songs"]'));
    await wait(() => title('103') && title('102') && title('101') &&
      !one('.search-results-enter-active'), 'actual single song rows');

    const provides = one('#app').__vue_app__._context.provides;
    const core = provides[Reflect.ownKeys(provides)
      .find((key) => typeof key === 'symbol' && key.description === 'core-client')];
    const original = core.searchGroupDownload.bind(core);
    core.searchGroupDownload = async (owner, command) => {
      const reply = await original(owner, command);
      actions.push({ command, reply });
      // Only simulate delivery loss after the actual native write/receipt.
      if (command.kind === 'confirm_quality') throw Error('sealed purchased confirmation reply lost');
      return reply;
    };

    const modal = () => one('[data-search-group-purchased][open]');
    const choice = (quality) => modal()?.querySelector(`[data-voice-purchased-quality="${quality}"]`);
    const selectionPaint = () => {
      const selected = choice('hires'), previous = choice('sky');
      const selectedRow = selected?.closest('label'), previousRow = previous?.closest('label');
      const marks = modal()?.querySelectorAll('.purchased-quality-row > svg');
      const mark = marks?.[0];
      if (!selected?.checked || previous?.checked || marks?.length !== 1 ||
          mark.parentElement !== selectedRow || !previousRow) return;
      const {left, top, width, height} = mark.getBoundingClientRect();
      const to = selectedRow.getBoundingClientRect(), from = previousRow.getBoundingClientRect();
      const color = getComputedStyle(mark).color;
      return {screenX, screenY, ratio: devicePixelRatio, paintRegions: [
        {name: 'hires-tick', left, top, width, height, color, present: true},
        {name: 'sky-no-tick', left: left + from.left - to.left, top: top + from.top - to.top,
          width, height, color, present: false},
      ]};
    };
    const waitForMenu = () => wait(() => {
      const node = download(), menu = node?.closest('[role="menu"]');
      return node && !node.disabled && menu && getComputedStyle(menu).opacity === '1' &&
        !menu.matches('.menu-enter-active,.menu-leave-active');
    }, 'mixed purchased download available on the blocked anchor');
    const expectChooser = async () => {
      await wait(() => modal() && getComputedStyle(modal()).opacity === '1' &&
        !modal().matches('.modal-enter-active,.modal-leave-active'), 'quality chooser fully presented');
      assert(modal().querySelector('h2').textContent === '选择下载音质', 'official title');
      assert([...modal().querySelectorAll('input[type="radio"]')].map(node => node.value).join(',') ===
        'sky,hires,exhigh,standard', 'union excludes blocked vivid and is sorted highest first');
      assert(choice('sky').checked, 'highest tier is default but not accepted automatically');
      assert(!one('[data-search-group-download-picker][open]'), 'no destination before explicit quality');
      assert((await downloads('active')).activeCount === 0, 'no tasks from opening chooser');
    };

    await input('first',title('103'));
    await wait(() => selection() === '103','first selected');
    await input('blocked',title('102'),1,true);
    await wait(() => selection() === '102,103','blocked row selected');
    await input('last',title('101'),1,true);
    await wait(() => selection() === '101,102,103','ordinary third selected');
    await input('paid2',title('104'),1,true);
    await wait(() => selection() === '101,102,103,104','second purchased row selected');
    await input('anchor',title('102'),3);
    await waitForMenu();
    await input('download',download());
    await expectChooser();
    await input('cancel',button('关闭音质选择',modal()));
    await wait(() => !modal() && actions.length === 2, 'native cancels original receipt');
    assert((await downloads('active')).activeCount === 0, 'cancellation never enqueues');

    await input('anchor_again',title('102'),3);
    await waitForMenu();
    await input('download_again',download());
    await expectChooser();
    assert(actions[2].reply.receipt !== actions[0].reply.receipt, 'reopen gets an independent receipt');
    await input('lower_tier',choice('hires').closest('label'));
    await wait(selectionPaint, 'trusted Hi-Res selection and the Vue check mark agree');
    await afterPaint();
    const paint = selectionPaint();
    assert(paint, 'Hi-Res check mark remains selected before confirmation');
    await input('confirm',button('确定',modal()),1,false,paint);
    await wait(() => button('核对下载') && !modal() && actions.length === 4 &&
      one('.track-action-feedback')?.textContent.includes('接纳结果未确认'), 'lost paid reply offers only status');
    await input('check',button('核对下载'));
    const result = await wait(async () => {
      const view = await downloads('completed');
      return actions.length === 5 && view.completedCount === 3 &&
        one('.track-action-feedback')?.textContent.includes('已添加 3 首到下载列表，跳过 1 首') && view;
    }, 'authorized audio is published once, status stays read only');
    assert(result.tasks.map((task) => task.track.id).sort().join(',') === '101,103,104',
      'only surviving requested tracks completed');
    await capture('accepted', { completed: result.tasks.map(({id, track}) => ({id, trackId:track.id})) });
    core.searchGroupDownload = original;
    await emit('complete',{ok:true,stageCount:step,realAccountWrites:0,realMusicRequests:0});
  } catch (error) {
    await emit('failed',{stageCount:step,step:stage,error:String(error),actions,
      text:document.body.innerText.slice(0,2800)});
  }
})();
