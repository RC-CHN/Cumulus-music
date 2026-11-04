// Production Vue/IPC and clipboard, isolated library mutations and null audio.
(async () => {
  const one = (s) => document.querySelector(s), all = (s) => [...document.querySelectorAll(s)];
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const assert = (v, m) => { if (!v) throw Error(m); };
  const wait = async (fn, m) => { for (let i = 0; i < 240; i++) { if (fn()) return; await sleep(50); } throw Error(m); };
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const emit = (stage, data = {}) => invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...data } });
  const button = (label, root = document) => [...root.querySelectorAll('button')].find((n) => n.getAttribute('aria-label') === label || n.textContent.trim() === label);
  const click = (node) => { assert(node && !node.disabled, 'enabled control'); node.scrollIntoView({ block: 'nearest' }); node.focus({ preventScroll: true }); node.click(); };
  const key = (node, value, extra = {}) => node.dispatchEvent(new KeyboardEvent('keydown', { key: value, bubbles: true, cancelable: true, ...extra }));
  const menu = () => one('.action-menu');
  let step = 'startup', count = 0;
  const fixture = async (mode) => {
    window.__libraryFixtureAck = null; await emit('library-fixture', { mode });
    await wait(() => window.__libraryFixtureAck, 'fixture ack'); return window.__libraryFixtureAck;
  };
  async function capture(label, extra = {}) {
    step = label; await sleep(200);
    assert(document.documentElement.scrollWidth <= innerWidth, 'document containment');
    for (const n of all('.action-menu, dialog[open]')) {
      const r = n.getBoundingClientRect();
      assert(r.left >= 0 && r.top >= 0 && r.right <= innerWidth + 1 && r.bottom <= innerHeight + 1, 'overlay containment');
    }
    for (const n of all('.action-menu button, dialog[open] footer button, dialog[open] header button')) {
      const r = n.getBoundingClientRect();
      const hit = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
      assert(hit && (n.contains(hit) || n === hit), 'dialog controls reachable');
    }
    await emit(`ux_${String(++count).padStart(2,'0')}_${label}`, { width: innerWidth, height: innerHeight, theme: document.documentElement.dataset.theme, items: menu() ? [...menu().querySelectorAll('button')].map(n => ({ text: n.textContent.trim(), disabled: n.disabled })) : [], ...extra });
    await sleep(420);
  }
  async function library() {
    click(button('我的歌单')); await wait(() => one('.playlist-library-page') && !one('.page-enter-active,.page-leave-active'), 'library');
  }
  async function open(id, context = false) {
    const card = one(`[data-library-open="${id}"]`), trigger = context ? card : card.closest('article').querySelector('.menu-trigger');
    trigger.scrollIntoView({ block: 'nearest' });
    // The library restores scroll/focus after mount. Dispatch only once the
    // trigger is stable and hit-testable; an outside scroll dismisses menus.
    let previous, stable = 0;
    await wait(() => {
      const r = trigger.getBoundingClientRect(), at = [r.x, r.y, r.width, r.height].join(':');
      stable = previous === at ? stable + 1 : 0; previous = at;
      const hit = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
      return stable >= 2 && trigger.contains(hit) && !one('.page-enter-active,.page-leave-active');
    }, 'settled card target ' + id);
    trigger.focus({ preventScroll: true });
    if (context) key(trigger, 'F10', { shiftKey: true }); else trigger.click();
    await wait(() => menu()?.style.visibility === 'visible' && menu().contains(document.activeElement), 'focused resource menu');
    return trigger;
  }
  async function choose(label) { click(button(label, menu())); await wait(() => !menu(), 'menu selection'); }
  async function escape(trigger) { key(menu(), 'Escape'); await wait(() => !menu(), 'escape'); assert(document.activeElement === trigger, 'menu focus restored'); }
  const feedback = () => one('.playlist-action-feedback');
  const done = () => feedback()?.textContent.includes('已添加至播放列表');
  const pause = async () => { const b=button('暂停',one('[aria-label="播放器"]')); if(b) click(b); await wait(()=>button('播放',one('[aria-label="播放器"]')),'paused'); };
  async function detail(id) { click(one(`[data-library-open="${id}"]`)); await wait(()=>one('.playlist-hero')&&!one('.page-enter-active,.page-leave-active'),'detail'); }
  try {
    await wait(()=>one('.track-more'),'daily'); await library();
    await open(8); await choose('下一首播放'); await wait(()=>done()&&button('暂停',one('[aria-label="播放器"]')),'empty queue starts');
    await capture('empty_queue_started'); await pause(); await capture('initial_paused');
    for(const [w,h,t] of [[1120,760,'light'],[1120,760,'dark'],[760,540,'light'],[760,540,'dark']]) {
      await emit('resize',{width:w,height:h}); await wait(()=>innerWidth===w&&innerHeight===h,'resize');
      click(button('外观主题')); await wait(()=>menu(),'theme'); click(button(t==='dark'?'深色':'浅色')); await wait(()=>!menu(),'theme closes');
      const suffix=`${w}_${t}`;
      let trigger=await open(7,true); assert(button('下一首播放',menu())&&!button('下一首播放',menu()).disabled,'collection next');
      await capture(`next_menu_${suffix}`); await escape(trigger);
      trigger=await open(5); assert(button('下一首播放',menu()).disabled,'empty next disabled'); await capture(`empty_menu_${suffix}`); await escape(trigger);
      await fixture('hold_detail'); trigger=await open(7); await choose('下一首播放');
      await wait(()=>feedback()?.getAttribute('aria-busy')==='true','pending feedback');
      assert(document.activeElement===trigger,'waiting keeps menu trigger focus'); assert(!button('播放',one('[aria-label="播放器"]')).disabled,'adding does not disable transport'); assert(!one('.playback-message'),'only queue feedback while adding'); await capture(`next_wait_${suffix}`);
      await fixture('success'); await wait(done,'playlist queued'); assert(button('播放',one('[aria-label="播放器"]')),'paused remains paused');
      await capture(`next_done_${suffix}`);
      await detail(8); trigger=button('更多歌单操作'); click(trigger); await wait(()=>menu(),'detail menu');
      assert(all('.action-menu button').map(n=>n.textContent.trim()).join('|')==='分享…|批量操作|添加全部至播放列表','detail action order');
      await capture(`detail_menu_${suffix}`); await fixture('hold_detail'); await choose('添加全部至播放列表');
      await wait(()=>feedback()?.getAttribute('aria-busy')==='true','detail pending'); await capture(`detail_wait_${suffix}`);
      await fixture('success'); await wait(done,'detail added'); assert(document.activeElement===trigger,'detail focus preserved'); await capture(`detail_done_${suffix}`);
      await library();
    }
    step='read failure'; await fixture('fail_detail'); let trigger=await open(7); await choose('下一首播放');
    await wait(()=>feedback()?.classList.contains('failed'),'visible failure'); assert(!one('.playback-message'),'queue failure is not an audio error'); await capture('failed');
    await fixture('hold_detail'); click(button('重试',feedback())); await wait(()=>feedback()?.getAttribute('aria-busy')==='true','retry waiting');
    const reads=(await fixture('hold_detail')).detailReads; click(button('正在重试…',feedback()));
    assert((await fixture('hold_detail')).detailReads===reads,'no double submission'); await capture('retry_wait');
    await fixture('success'); await wait(done,'retry done'); assert(document.activeElement===trigger,'retry restores original trigger'); await capture('retry_done');
    step='changed to empty'; await fixture('empty_detail'); trigger=await open(7); await choose('下一首播放');
    await wait(()=>feedback()?.textContent.includes('没有可添加'),'authoritative empty'); await capture('became_empty');
    click(button('关闭',feedback())); await wait(()=>!feedback(),'dismiss empty'); assert(document.activeElement===trigger,'dismiss focus');
    step='superseded'; await fixture('hold_detail'); await open(7); await choose('下一首播放'); await wait(()=>feedback()?.getAttribute('aria-busy')==='true','held request');
    click(button('播放',one('[aria-label="播放器"]'))); await wait(()=>button('暂停',one('[aria-label="播放器"]')),'new transport');
    await fixture('success'); await wait(()=>feedback()?.textContent.includes('本次添加已取消'),'late response not success'); await pause(); await capture('superseded');
    click(button('下一首',one('[aria-label="播放器"]'))); await wait(()=>button('暂停',one('[aria-label="播放器"]')) && !button('暂停',one('[aria-label="播放器"]')).disabled && one('[aria-label="播放器"]').textContent.includes('合成歌曲 1 ·'),'explicit next');
    await pause(); await capture('explicit_next');
    click(button('播放队列')); await wait(()=>one('.queue-panel'),'queue drawer'); await capture('queue_drawer');
    click(button('关闭播放队列')); await wait(()=>!one('.queue-panel'),'queue close');
    step='sidebar queue';
    one('[data-sidebar-group="created"] header').scrollIntoView({block:'start'});
    await wait(()=>one('[data-sidebar-id="2"]'),'sidebar row');
    const side=one('[data-sidebar-id="2"]'); side.focus(); key(side,'F10',{shiftKey:true});
    await wait(()=>menu()?.style.visibility==='visible','sidebar menu'); await capture('sidebar_menu');
    await fixture('hold_detail'); await choose('下一首播放'); await wait(()=>feedback()?.getAttribute('aria-busy')==='true','sidebar wait');
    assert(document.activeElement===side,'sidebar waiting focus'); await capture('sidebar_wait');
    await fixture('success'); await wait(done,'sidebar added'); assert(document.activeElement===side,'sidebar success focus'); await capture('sidebar_done');
    window.__motionProof.system(true); await wait(()=>document.documentElement.dataset.motion==='reduced','reduced motion');
    side.focus(); key(side,'F10',{shiftKey:true}); await wait(()=>menu()?.style.visibility==='visible','reduced menu');
    assert(menu().getAnimations().every(a=>Number(a.effect.getTiming().duration)<=1),'reduced menu animation');
    await capture('reduced_menu'); await escape(side); window.__motionProof.system(false);
    step='removed return target';
    await fixture('fail_detail'); trigger=await open(7); await choose('下一首播放'); await wait(()=>feedback()?.classList.contains('failed'),'failure before navigating');
    click(button('每日推荐')); await wait(()=>!one('.playlist-library-page')&&!one('.page-leave-active'),'leave source page');
    assert(!trigger.isConnected,'old trigger unmounted'); await capture('detached_failure');
    await fixture('success'); click(button('重试',feedback())); await wait(done,'retry after navigation');
    assert(document.activeElement===one('.sidebar-navigation [aria-current="page"]'),'disconnected trigger uses current page navigation focus'); await capture('detached_recovered');
    await library();
    step='online row drop';
    await emit('resize',{width:1120,height:760}); await wait(()=>innerWidth===1120,'wide queue');
    await detail(9);
    const field=one('[aria-label="搜索歌单歌曲"]');
    field.value='合成歌曲 507'; field.dispatchEvent(new Event('input',{bubbles:true}));
    await wait(()=>all('.track-scroll [data-track-id]').length===1 &&
      one('.track-scroll [data-track-id]')?.dataset.trackId==='507','filtered source with original ordinal two');
    const {epoch}=await invoke('account_snapshot');
    const before=await invoke('player_snapshot',{epoch});
    const original=await invoke('player_queue',{epoch});
    const queueIds=(q)=>q.tracks.map(r=>r.track.id);
    assert(before.state==='paused' && !queueIds(original).includes('507'),'paused queue lacks the target song');
    click(button('播放队列'));
    await wait(()=>one('.queue-scroll')?.querySelectorAll('.queue-row').length>=2,'ordinary drop destination');
    await sleep(350);
    function startDrop() {
      const source=one('.track-scroll [data-track-id="507"] .track-row-title-text');
      const viewport=one('.queue-scroll');
      viewport.scrollTop=0;
      const a=source.getBoundingClientRect(), b=viewport.getBoundingClientRect();
      const x=a.left+4, y=a.top+a.height/2, X=b.left+b.width/2, Y=b.top+72;
      assert(source.contains(document.elementFromPoint(x,y)) &&
        viewport.contains(document.elementFromPoint(X,Y)),'source and destination are both pointer reachable');
      const pointer=(node,type,clientX,clientY)=>node.dispatchEvent(new PointerEvent(type,{
        bubbles:true,cancelable:true,pointerId:57,pointerType:'mouse',button:0,buttons:type==='pointerup'?0:1,clientX,clientY,
      }));
      pointer(source,'pointerdown',x,y); pointer(viewport,'pointermove',X,Y);
      return ()=>pointer(viewport,'pointerup',X,Y);
    }
    await fixture('empty_detail');
    startDrop()();
    await wait(()=>one('.queue-notice[role="alert"]')?.textContent.includes('列表已更新'),'stale playlist rejection in queue');
    assert((await invoke('player_queue',{epoch})).revision===original.revision,'stale playlist leaves queue intact');
    await capture('playlist_drop_stale');
    await fixture('success');
    const drop=startDrop();
    await wait(()=>one('.queue-drop-marker')?.style.top==='64px' &&
      one('.queue-drag-hint')?.textContent.includes('正在添加歌曲'),'visible insertion marker between rows');
    const line=one('.queue-drop-marker'), rect=line.getBoundingClientRect();
    await capture('playlist_drop_boundary',{screenX,screenY,ratio:devicePixelRatio,
      paintSamples:[{x:rect.left+8,y:rect.top+1,color:getComputedStyle(line).backgroundColor}]});
    drop();
    const expected=queueIds(original); expected.splice(1,0,'507');
    let inserted;
    for(let i=0;i<200;i++) {
      const observed=await invoke('player_queue',{epoch});
      if(queueIds(observed).join(',')===expected.join(',')){inserted=observed;break;}
      await sleep(30);
    }
    assert(inserted && inserted.revision!==original.revision,'exact original boundary insertion');
    const after=await invoke('player_snapshot',{epoch});
    assert(after.state==='paused' && after.playId===before.playId && after.entryId===before.entryId &&
      Math.abs(after.position-before.position)<.05 && !one('.playback-message'),'retains native playback and avoids an audio error');
    await capture('playlist_drop_done',{inserted:'507',at:1});
    click(button('关闭播放队列')); await wait(()=>!one('.queue-panel'),'close drop destination');
    await library();
    await fixture('expired_detail'); await open(7); await choose('下一首播放'); await wait(()=>button('重新登录') || button('点击登录'),'expired account'); await capture('expired');
    await emit('complete',{ok:true,playlistQueueUi:true,onlinePlaylistDrop:true,stageCount:count,realMusicRequests:0,realAccountWrites:0});
  } catch(error) { await emit('failed',{step,message:String(error),stack:error.stack,stageCount:count}); }
})();
