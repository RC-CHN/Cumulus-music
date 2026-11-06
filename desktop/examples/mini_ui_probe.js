(async () => {
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const sleep = ms => new Promise(r => setTimeout(r, ms));
  const assert = (ok, text) => { if (!ok) throw Error(text); };
  const wait = async (fn, text) => { for (let i = 0; i < 140; i++) { if (await fn()) return; await sleep(100); } throw Error(text); };
  const emit = payload => invoke('plugin:event|emit', { event: 'mini-proof', payload });
  window.__miniAck = {};
  let serial = 0, stageCount = 0;
  const fixture = async (kind, payload = {}) => { const id = ++serial; await emit({ kind, id, ...payload }); await wait(() => id in window.__miniAck, `fixture ${kind}`); const result = window.__miniAck[id]; delete window.__miniAck[id]; if (result.error) throw Error(result.error); return result.value ?? result; };
  const mini = source => fixture('eval', { source });
  const facts = `const r=n=>{const v=n.getBoundingClientRect();return {x:v.x,y:v.y,right:v.right,bottom:v.bottom,width:v.width,height:v.height}};
    return { width:innerWidth,height:innerHeight,screenX,screenY,ratio:devicePixelRatio,visible:document.visibilityState==='visible',focused:document.hasFocus(),paintSamples:[{x:3,y:3,color:getComputedStyle(document.querySelector('.mini-shell')).backgroundColor}],theme:document.documentElement.dataset.theme,body:document.body.textContent,
      overflow:document.documentElement.scrollWidth>innerWidth,buttons:[...document.querySelectorAll('button')].map(n=>({label:n.getAttribute('aria-label')||n.textContent.trim(),...r(n)})),
      mode:document.querySelector('[data-mini-mode]')?.dataset.mode,modeFamily:document.querySelector('[data-mini-mode]')?.dataset.family,modeHit:(()=>{const n=document.querySelector('[data-mini-mode]'),b=n.getBoundingClientRect(),hit=document.elementFromPoint(b.left+b.width/2,b.top+b.height/2);return n===hit||n.contains(hit)})(),modeError:document.querySelector('.mini-message[role="alert"]')?.textContent??null,rows:document.querySelectorAll('.mini-queue-rows li').length,background:getComputedStyle(document.querySelector('.mini-shell')).backgroundColor };`;
  const capture = async name => { await sleep(550); const data=await mini(facts); assert(!data.overflow, 'mini horizontal overflow');assert(data.modeHit,'mini mode hit'); assert(data.buttons.every(b=>b.x>=0 && b.right<=data.width+1 && b.width>0), 'mini horizontal controls'); await emit({kind:'snapshot',stage:`ux_${String(++stageCount).padStart(2,'0')}_${name}`,...data}); await sleep(1300); };
  // A return/close destroys this WebView; acknowledge the fixture before
  // scheduling the click, then let callers verify the actual native transition.
  const clickMini = label => mini(`const b=[...document.querySelectorAll('button')].find(b=>b.getAttribute('aria-label')===${JSON.stringify(label)});if(!b||b.disabled)throw Error('mini button unavailable: '+${JSON.stringify(label)});b.focus({preventScroll:true});${['返回主窗口','关闭窗口'].includes(label) ? 'setTimeout(()=>b.click(),100)' : 'b.click()'};return true;`);
  const read = () => mini(`return await window.__TAURI_INTERNALS__.invoke('mini_read',{generation:location.hash.slice(6)});`);
  const labels={order_loop:'列表循环',single_loop:'单曲循环',random:'随机播放',sequential:'顺序播放'};
  const modeSettled=async(mode,family)=>{await wait(async()=>{const current=await read();return current.media.mode===mode&&current.media.modeFamily===family&&await mini(`const b=document.querySelector('[data-mini-mode]');return b.dataset.mode===${JSON.stringify(mode)}&&b.dataset.family===${JSON.stringify(family)}&&b.closest('.mini-shell').dataset.controlTicket===${JSON.stringify(current.ticket)}&&!b.disabled;`);},'mini mode '+mode+'/'+family);};
  const cycle=async(mode,family,label)=>{const before=await read();await clickMini(labels[before.media.mode]);await modeSettled(mode,family);const after=await read();assert(!after.media.playing&&Math.abs(after.media.position-10)<.2,'mode retains paused progress');assert(!(await mini(facts)).modeError,'mode has no hidden error');await capture(label);};
  const open = async () => { await invoke('mini_enter'); await wait(async()=>!(await fixture('windows')).main,'main hidden after mini ready'); await wait(async()=>(await read()).media.item,'mini populated'); };
  try {
    await wait(()=>document.querySelector('.track-row'),'main startup');
    // The real system portal may report dark. Establish the tested baseline
    // explicitly before comparing the subsequent theme transition.
    localStorage.setItem('cumulus.appearance.v1','light');
    window.dispatchEvent(new StorageEvent('storage',{key:'cumulus.appearance.v1',newValue:'light'}));
    await wait(()=>document.documentElement.dataset.theme==='light','light baseline');
    let account = await invoke('account_snapshot');
    await invoke('play_selection',{epoch:account.epoch,selection:{kind:'songs',ids:Array.from({length:40},(_,i)=>String(i+1)),index:0,quality:'standard'}});
    await sleep(500);
    await invoke('player_action',{epoch:account.epoch,action:{kind:'pause',paused:true}});
    const mainPlayer = () => invoke('player_snapshot',{epoch:account.epoch});
    const normal = await mainPlayer();
    const mainVolume = () => document.querySelector('.playerbar .volume-trigger');
    await wait(()=>mainVolume()?.getAttribute('aria-label')==='静音','ordinary volume control');
    document.querySelector('.playerbar .volume-control').dispatchEvent(new PointerEvent('pointerenter',{pointerType:'mouse'}));
    await wait(()=>!!document.querySelector('.playerbar .volume-popover'),'ordinary hover volume popup');
    mainVolume().click();
    await wait(async()=>mainVolume()?.getAttribute('aria-label')==='取消静音' && (await mainPlayer()).volume===0,'ordinary click still mutes');
    mainVolume().click();
    await wait(async()=>mainVolume()?.getAttribute('aria-label')==='静音' && (await mainPlayer()).volume===normal.volume,'ordinary click still restores volume');
    const afterNormal = await mainPlayer();
    assert(afterNormal.playId===normal.playId && afterNormal.state==='paused','ordinary mute retains transport');
    document.querySelector('.playerbar').dispatchEvent(new PointerEvent('pointerdown',{bubbles:true}));
    await wait(()=>!document.querySelector('.playerbar .volume-popover'),'ordinary popup closed');
    // A module that never mounts cannot hide the main window; real 8-second timeout.
    await fixture('block',{enabled:true});
    const failedOpen = invoke('mini_enter').then(()=>false,()=>true);
    await sleep(500);
    assert((await fixture('windows')).main,'main hidden before ready');
    await wait(async()=>(await fixture('windows')).mini.length===0,'mini timeout did not close failed window');
    assert((await fixture('windows')).main,'main not restored on timeout');
    assert(await failedOpen,'failed handover did not reach the main-window error state');
    await fixture('block',{enabled:false});
    await open();
    await capture('light_paused');
    const before=await read();
    let rejected=await mini(`try{await window.__TAURI_INTERNALS__.invoke('account_snapshot');return false}catch{return true}`);
    assert(rejected,'mini gained main-only account IPC');
    rejected=await invoke('mini_read',{generation:before.generation}).then(()=>false,()=>true);
    assert(rejected,'main gained mini-only IPC');
    await clickMini('播放'); await wait(async()=>(await read()).media.playing,'mini play');
    await clickMini('暂停'); await wait(async()=>!(await read()).media.playing,'mini pause');
    await capture('paused_after_toggle');
    await invoke('player_action',{epoch:account.epoch,action:{kind:'seek',seconds:10}});
    await modeSettled('order_loop','ordinary');
    for(const [mode,name] of [['single_loop','single'],['random','random'],['sequential','sequential'],['order_loop','loop']])await cycle(mode,'ordinary','ordinary_'+name);

    const volumePanel = () => mini(`return document.querySelector('.volume-trigger').getAttribute('aria-expanded')==='true'
      && !!document.querySelector('[aria-label="音量"][aria-orientation="vertical"]');`);
    const adjustVolume = value => mini(`const input=document.querySelector('[aria-label="音量"]');input.value=${JSON.stringify(String(value))};input.dispatchEvent(new Event('input',{bubbles:true}));return true;`);
    const showVolume = async () => { await clickMini('音量调节'); await wait(volumePanel, 'explicit mini volume popup'); };
    const hideVolume = async () => {
      await clickMini('音量调节');
      await wait(()=>mini(`return !document.querySelector('.volume-popover')&&document.activeElement===document.querySelector('.volume-trigger');`),'repeated volume click closes');
    };
    const beforeVolume = await mainPlayer();
    const checkVolume = async value => {
      const native=await mainPlayer();
      assert(native.volume===value && native.playId===beforeVolume.playId &&
        native.queueRevision===beforeVolume.queueRevision && native.state==='paused' &&
        Math.abs(native.position-beforeVolume.position)<.2,'mini volume popup preserves current transport');
    };
    await mini(`document.querySelector('.volume-control').dispatchEvent(new PointerEvent('pointerenter',{pointerType:'mouse'}));return true;`);
    assert(!(await volumePanel()),'hover alone does not open the mini popup');
    await mini(`document.querySelector('.volume-trigger').focus();return true;`);
    assert(!(await volumePanel()),'focus alone does not open the mini popup');
    await showVolume();
    await checkVolume(beforeVolume.volume);
    await capture('volume_opened');
    await adjustVolume(28);
    await wait(async()=>(await read()).media.volume===28,'volume');
    await mini(`const trigger=document.querySelector('.volume-trigger');trigger.dispatchEvent(new WheelEvent('wheel',{bubbles:true,cancelable:true,deltaY:-1}));return true;`);
    await wait(async()=>(await read()).media.volume===31,'wheel volume');
    await hideVolume();
    await checkVolume(31);
    await showVolume();
    await adjustVolume(0);
    await wait(async()=>{const current=await read();return current.media.volume===0&&current.media.unmuteVolume===31;},'slider zero preserves last volume');
    await capture('volume_muted');
    await hideVolume();
    await checkVolume(0);
    await showVolume();
    await checkVolume(0);
    await adjustVolume(31);
    await wait(async()=>(await read()).media.volume===31,'slider restores requested volume');
    assert(await mini(`const n=document.querySelector('.volume-popover'),r=n.getBoundingClientRect();return r.left>=0&&r.top>=0&&r.right<=innerWidth&&r.bottom<=innerHeight;`),'volume popover clipped');
    await mini(`const n=document.querySelector('[aria-label="音量"]');n.focus();n.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true}));return true;`);
    await wait(()=>mini(`return !document.querySelector('.volume-popover')&&document.activeElement===document.querySelector('.volume-trigger');`),'volume escape restores focus');
    await checkVolume(31);
    await clickMini('喜欢'); await wait(async()=>(await read()).media.like?.observed===true,'like');
    await clickMini('播放队列'); await wait(async()=>(await mini(facts)).rows===30,'bounded queue');
    await capture('queue_first_page');
    await mini(`const b=[...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='下一页');b.click();return true;`);
    await wait(async()=>(await mini(facts)).rows===10,'second queue page');
    await capture('queue_second_page');
    await mini(`const b=document.querySelector('.mini-queue-track');b.click();return true;`);
    await wait(async()=>(await read()).media.item?.id!==before.media.item.id,'queue selection');
    // Main still exists, and direct native actions update the mini projection.
    await invoke('player_action',{epoch:account.epoch,action:{kind:'pause',paused:true}});
    await wait(async()=>!(await read()).media.playing,'main-to-mini pause');
    const stale=await read();
    await invoke('player_action',{epoch:account.epoch,action:{kind:'next'}});
    assert(await mini(`try { await window.__TAURI_INTERNALS__.invoke('mini_action',{generation:${JSON.stringify(stale.generation)},ticket:${JSON.stringify(stale.ticket)},action:{kind:'stop'}});return false;}catch{return true;}`),'old ticket stopped new track');
    assert(await mini(`try { await window.__TAURI_INTERNALS__.invoke('mini_action',{generation:${JSON.stringify(stale.generation)},ticket:${JSON.stringify(stale.ticket)},action:{kind:'volume',value:77}});return false;}catch{return true;}`),'old ticket changed volume');
    await wait(async()=>(await read()).media.item?.id!==stale.media.item.id,'new track after stale action');
    assert((await read()).media.volume===31,'stale ticket changed the next track volume');
    await invoke('player_action',{epoch:account.epoch,action:{kind:'pause',paused:true}});
    await mini(`localStorage.setItem('cumulus.appearance.v1','dark');window.dispatchEvent(new StorageEvent('storage',{key:'cumulus.appearance.v1',newValue:'dark'}));return true;`);
    await capture('dark_queue');
    await clickMini('始终置顶'); await wait(async()=>(await read()).preferences.alwaysOnTop===false,'pin persistence');
    await clickMini('播放队列'); await capture('dark_compact');
    const beforeRoom=await read();
    await invoke('together_command',{epoch:account.epoch,command:{kind:'create'}});
    await wait(async()=>{const room=await invoke('together_view',{epoch:account.epoch,refresh:false});return room.value.attached&&room.value.pending.length===0;},'synthetic room attached');
    await modeSettled('order_loop','together');
    assert(await mini(`try{await window.__TAURI_INTERNALS__.invoke('mini_action',{generation:${JSON.stringify(beforeRoom.generation)},ticket:${JSON.stringify(beforeRoom.ticket)},action:{kind:'mode',mode:'sequential'}});return false;}catch{return true;}`),'pre-room mode ticket rejected');
    await invoke('player_action',{epoch:account.epoch,action:{kind:'pause',paused:true}});
    await invoke('player_action',{epoch:account.epoch,action:{kind:'seek',seconds:10}});
    await wait(async()=>{const room=await invoke('together_view',{epoch:account.epoch,refresh:false});return !room.value.playing&&room.value.progressMs===10000&&room.value.pending.length===0;},'room pause and progress settled');
    for(const theme of ['light','dark']){
      await mini(`localStorage.setItem('cumulus.appearance.v1',${JSON.stringify(theme)});window.dispatchEvent(new StorageEvent('storage',{key:'cumulus.appearance.v1',newValue:${JSON.stringify(theme)}}));return true;`);
      await wait(async()=>(await mini(facts)).theme===theme,'room theme');
      for(const [mode,name] of [['single_loop','single'],['random','random'],['order_loop','loop']])await cycle(mode,'together','room_'+theme+'_'+name);
    }
    const beforeExit=await read();
    await invoke('together_command',{epoch:account.epoch,command:{kind:'end',room_id:'synthetic-mini-room'}});
    await modeSettled('order_loop','ordinary');
    assert(await mini(`try{await window.__TAURI_INTERNALS__.invoke('mini_action',{generation:${JSON.stringify(beforeExit.generation)},ticket:${JSON.stringify(beforeExit.ticket)},action:{kind:'mode',mode:'single_loop'}});return false;}catch{return true;}`),'departed room mode ticket rejected');
    await capture('ordinary_after_room');

    await clickMini('返回主窗口'); await wait(async()=>(await fixture('windows')).main && (await fixture('windows')).mini.length===0,'return main');
    await open(); assert(!(await read()).preferences.alwaysOnTop,'pin not saved'); await capture('reopened');
    // Native tray show uses the same return path and cannot leave a hidden orphan.
    await fixture('show'); assert((await fixture('windows')).main && !(await fixture('windows')).mini.length,'tray return');
    await open();
    await mini(`setTimeout(()=>location.reload(),50);return true;`);
    await wait(async()=>(await fixture('windows')).main && !(await fixture('windows')).mini.length,'reload fallback');
    await open(); await clickMini('关闭窗口');
    await wait(()=>document.querySelector('dialog[open]'),'mini close did not use main close prompt');
    const cancel=document.querySelector('dialog[open] [aria-label="取消关闭"]');
    assert(cancel,'close prompt cancel'); cancel.click(); await wait(()=>!document.querySelector('dialog[open]'),'close prompt dismissed');
    await open(); const previousAccount=await read(); await fixture('account');
    await wait(async()=>!(await read()).media.item,'account change clears mini');
    assert(await mini(`try { await window.__TAURI_INTERNALS__.invoke('mini_action',{generation:${JSON.stringify(previousAccount.generation)},ticket:${JSON.stringify(previousAccount.ticket)},action:{kind:'toggle'}});return false;}catch{return true;}`),'old account controls were accepted');
    await capture('account_isolated');
    await clickMini('返回主窗口');
    await emit({kind:'complete',stage:'complete',ok:true,stageCount,realMusicRequests:0,timeoutVerified:true,staleControlRejected:true,ipcIsolation:true,reloadFallback:true,sharedAudio:true,closePolicy:true,accountIsolation:true,ordinaryModeCycle:true,togetherModeCycle:true,roomModeTicketsRejected:true,twoAccountRealtimeVerified:false,volumeControlsVerified:true,volumePopupToggleVerified:true,ordinaryVolumeMuteVerified:true});
  } catch(e) { await emit({kind:'failed',stage:'failed',message:String(e)+'\n'+String(e?.stack||''),body:document.body.textContent,stageCount}); }
})();
