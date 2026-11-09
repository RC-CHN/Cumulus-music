// Actual radio-detail row menu over the production adapter and sealed native API.
(async () => {
  const one = (s, root = document) => root?.querySelector(s);
  const row = (id) => one(`.podcast-page .episode-row[data-voice-id="${id}"]`);
  const trigger = (id) => one('[data-podcast-program-more]', row(id));
  const menu = () => one('[role="menu"][aria-label^="单集菜单"]:not([inert])');
  const item = (key) => one(`[data-menu-key="${key}"]`, menu());
  const dialog = (kind) => one(`dialog.${kind}[open]:not([inert])`);
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, facts = {}) => invoke('plugin:event|emit', {event:'ux-proof',payload:{stage,...facts}});
  const enabled = (el) => !!el?.getClientRects().length && !el.disabled && !el.closest('[inert]');
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const frame = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const assert = (test, label) => { if (!test) throw Error(label); };
  const button = (text, root = document) => [...(root?.querySelectorAll('button') ?? [])]
    .find((el) => el.textContent.trim() === text || el.getAttribute('aria-label') === text);
  let stageCount = 0, last = 'bootstrap', epoch, hold, release, parked;
  const countReceipts = [], copied = [], queueCalls = [];
  async function wait(fn, label) {
    last = label;
    const until = performance.now() + 15000;
    while (performance.now() < until) { const value = await fn(); if (value) return value; await sleep(50); }
    throw Error(label);
  }
  const transitions = () => !one('.page-enter-active,.page-leave-active,.modal-enter-active,.modal-leave-active,.menu-enter-active,.menu-leave-active');
  async function click(find, label) {
    const el = await wait(() => { const el = find(); return transitions() && enabled(el) && el; }, label);
    el.scrollIntoView({block:'nearest',behavior:'instant'});
    await frame();
    await wait(() => {
      if (find()!==el || !enabled(el) || !transitions()) return false;
      const r=el.getBoundingClientRect();
      return el.contains(document.elementFromPoint(r.left+r.width/2,r.top+r.height/2));
    }, label+' hit target after scroll');
    el.focus({preventScroll:true});
    el.click();
    return el;
  }
  function key(node, value, shiftKey = false) {
    node.dispatchEvent(new KeyboardEvent('keydown', { key:value, shiftKey, bubbles:true, cancelable:true }));
  }
  function state() { return invoke('player_snapshot',{epoch}); }
  function queue() { return invoke('player_queue',{epoch}); }
  async function menuReady(id, count) {
    return wait(() => trigger(id)?.getAttribute('aria-expanded') === 'true' &&
      menu()?.getAttribute('aria-label') === `单集菜单：合成单集 ${id}` &&
      (count === undefined || item('comments')?.textContent.trim() === `查看评论(${count})`), `program menu ${id}`);
  }
  async function capture(label, extra = {}) {
    await wait(transitions,'settled presentation '+label);
    await frame();
    const open = menu(), rect = open?.getBoundingClientRect();
    assert(!rect || (rect.left >= 7 && rect.top >= 7 && rect.right <= innerWidth-7 && rect.bottom <= innerHeight-7), 'menu fits viewport');
    const active = await state(), rows = await queue();
    const stage = `ux_${String(++stageCount).padStart(2,'0')}_${label}`;
    window.__podcastProgressStage = '';
    await emit(stage,{...extra, actual:document.visibilityState==='visible',
      focus:document.activeElement?.getAttribute('aria-label') ?? null,
      menu:open ? [...open.querySelectorAll('[data-menu-key]')].map((el) => [el.dataset.menuKey,el.textContent.trim()]) : null,
      countReceipts:countReceipts.slice(), copied:copied.slice(), queueCalls:queueCalls.slice(),
      native:{state:active.state, voice:active.track?.voiceId ?? null, song:active.track?.id ?? null,
        playId:active.playId, entryId:active.entryId, position:active.position, revision:rows.revision,
        origin:active.origin, rows:rows.tracks.map((entry) => entry.track.voiceId)},
    });
    await wait(() => window.__podcastProgressStage===stage, 'native capture '+label);
  }
  try {
    epoch = (await wait(async () => {
      const value = await invoke('account_snapshot');
      return value.status === 'authenticated' && value;
    }, 'sealed account')).epoch;
    const provides = one('#app').__vue_app__._context.provides;
    const core = provides[Reflect.ownKeys(provides).find((key) => typeof key==='symbol' && key.description==='core-client')];
    const originalCount=core.podcastProgramCommentCount, originalCopy=core.copyText, originalAction=core.playerAction;
    core.podcastProgramCommentCount = async (owner,target) => {
      const result = await originalCount.call(core,owner,target);
      countReceipts.push({ owner, target, result });
      if (hold && target.voiceId==='501') await new Promise((done) => { release=done; });
      return result;
    };
    core.copyText = async (text) => { const result=await originalCopy.call(core,text); copied.push(text); return result; };
    core.playerAction = async (owner,action) => {
      const result = await originalAction.call(core,owner,action);
      if (action.kind==='queue_voice') queueCalls.push({owner,action});
      return result;
    };
    await click(() => button('我的播客',one('.sidebar-navigation[aria-label="我的"]')), 'my podcasts');
    await click(() => one('.podcast-card[data-radio-id="8"]'), 'radio card');
    await wait(() => enabled(trigger('501')) && enabled(trigger('503')) && enabled(trigger('504')), 'radio programs');
    assert(countReceipts.length===0,'count not prefetched by the radio page');
    await click(() => trigger('501'),'More for first program');
    await menuReady('501',51);
    await capture('more');
    key(menu(),'Escape');
    await wait(() => document.activeElement===trigger('501') && trigger('501').getAttribute('aria-expanded')==='false','Escape returns focus');

    hold=true;
    key(trigger('501'),'F10',true);
    await wait(() => release && menu()?.getAttribute('aria-label')?.endsWith('501'),'old menu count awaiting delivery');
    const next = row('503');
    next.scrollIntoView({block:'center',behavior:'instant'});
    await frame();
    const rect = next.getBoundingClientRect();
    const x = rect.left + Math.min(130,rect.width/2), y = rect.top + rect.height/2;
    const hit = document.elementFromPoint(x,y);
    assert(hit?.closest('.episode-row')===next,'context point hits the visible second program');
    hit.dispatchEvent(new MouseEvent('contextmenu',{button:2,bubbles:true,cancelable:true,clientX:x,clientY:y}));
    await menuReady('503',53);
    hold=false;
    release();
    await frame();
    assert(item('comments')?.textContent.trim()==='查看评论(53)','late other count did not replace this menu');
    await capture('right_click',{pointerHitVoice:next.dataset.voiceId});
    await click(() => item('share'),'open program share');
    const sharing = await wait(() => dialog('track-share-dialog'),'program share dialog');
    const link='https://music.163.com/dj?id=503';
    assert(one('input',sharing)?.value===link && sharing.textContent.includes('合成单集 503'),'program link and title');
    await click(() => button('复制链接',sharing),'copy program link in dialog');
    await wait(() => one('[role="status"]',sharing)?.textContent.trim()==='链接已复制','clipboard acknowledgement');
    await capture('share',{link});
    await click(() => button('关闭分享',sharing),'close share');
    await wait(() => !dialog('track-share-dialog') && document.activeElement===trigger('503'),'return from sharing');
    await click(() => trigger('503'),'More for direct copy');
    await menuReady('503',53);
    await click(() => item('copy'),'direct program copy');
    await wait(() => copied.length===2,'direct clipboard acknowledgement');

    await invoke('play_selection',{epoch,selection:{kind:'voice',id:'501'}});
    await wait(async () => {const v=await state();return v.state==='playing' && v.position>.2 && v.track?.voiceId==='501';},'native baseline playing');
    await invoke('player_action',{epoch,action:{kind:'pause',paused:true}});
    parked=await wait(async()=>{const v=await state();return v.state==='paused' && v.position>.2 && v;},'native baseline paused');
    await click(() => trigger('503'),'More for next');
    await menuReady('503',53);
    await click(() => item('next'),'next program');
    await wait(async()=>{const q=await queue();return q.tracks.length===2 && q.tracks[1].track.voiceId==='503';},'next appears in native queue');
    const retained=await state();
    assert(retained.playId===parked.playId && retained.entryId===parked.entryId && retained.position===parked.position && retained.state==='paused','next keeps parked audio');
    await capture('next',{previous:{playId:parked.playId,entryId:parked.entryId,position:parked.position,revision:parked.queueRevision}});

    await click(() => trigger('503'),'More for download');
    await menuReady('503',53);
    await click(() => item('download'),'open program download');
    await wait(() => dialog('download-dialog'),'ordinary program download dialog');
    await capture('download',{dialog:dialog('download-dialog').querySelector('h2')?.textContent.trim()});
    await click(() => button('关闭下载对话框',dialog('download-dialog')),'cancel program download');
    await wait(() => !dialog('download-dialog'),'download closed');

    await click(() => trigger('503'),'More for comments');
    await menuReady('503',53);
    await click(() => item('comments'),'open program comments');
    await wait(() => one('.comments-page')?.textContent.includes('合成单集 503'),'program comment route');
    await capture('comments');
    await click(() => button('返回上一页'),'return to radio detail');
    await click(() => trigger('504'),'More for play');
    await menuReady('504',54);
    await click(() => item('play'),'play in surrounding radio');
    await wait(async()=>{const v=await state();return v.state==='playing' && v.position>.2 && v.track?.voiceId==='504' && v.queueLen===3;},'full radio queue plays selected program');
    await capture('play');
    await emit('complete',{ok:true,stageCount,realMusicRequests:0,realAccountWrites:0,physicalAudioVerified:false});
  } catch(error) {
    await emit('failed',{ok:false,last,error:String(error?.message ?? JSON.stringify(error)),countReceipts,copied,queueCalls,
      menu:menu()?.outerHTML, trigger504:trigger('504')?.outerHTML, motion:transitions(),
      route:one('.main-content h1')?.textContent.trim()});
  }
})();
