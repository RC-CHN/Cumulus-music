// Actual three-window lifecycle; only the account, API and PCM are synthetic.
(async () => {
  const invoke=(command,args={})=>window.__TAURI_INTERNALS__.invoke(command,args);
  const sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const assert=(value,message)=>{if(!value)throw Error(message);};
  const wait=async(check,label)=>{for(let n=0;n<220;n++){if(await check())return;await sleep(70);}throw Error(label);};
  const emit=value=>invoke('plugin:event|emit',{event:'mini-proof',payload:value});
  window.__miniAck={};let serial=0,count=0,last='startup',epoch,playId;
  async function fixture(kind,payload={}){
    const id=++serial;await emit({kind,id,...payload});await wait(()=>id in window.__miniAck,'fixture '+kind);
    const reply=window.__miniAck[id];delete window.__miniAck[id];if(reply.error)throw Error(reply.error);return reply.value??reply;
  }
  const mini=source=>fixture('eval',{source});
  const read=()=>mini("return await window.__TAURI_INTERNALS__.invoke('mini_read',{generation:location.hash.slice(6)});");
  const status=()=>invoke('desktop_lyric_settings',{epoch});
  const root="document.querySelector('[role=\"menu\"][aria-label=\"小窗菜单\"]:not(.menu-leave-active)')";
  const child="document.querySelector('.action-submenu:not(.menu-leave-active)')";
  async function theme(value,main=false){
    if(main){localStorage.setItem('cumulus.appearance.v1',value);window.dispatchEvent(new StorageEvent('storage',{key:'cumulus.appearance.v1',newValue:value}));}
    else await mini(`localStorage.setItem('cumulus.appearance.v1',${JSON.stringify(value)});window.dispatchEvent(new StorageEvent('storage',{key:'cumulus.appearance.v1',newValue:${JSON.stringify(value)}}));return true;`);
  }
  async function openMini(){await invoke('mini_enter');await wait(async()=>!(await fixture('windows')).main,'mini visible');await wait(()=>mini("return !!document.querySelector('[aria-label=\"小窗菜单\"]:not(:disabled)');"),'menu trigger ready');await auditMenu();}
  async function auditMenu(){await mini(`window.__miniMenuLeaving=[];const seen=new WeakSet();new MutationObserver(()=>{for(const p of document.querySelectorAll('.menu-leave-active')){if(seen.has(p))continue;seen.add(p);const n=p.querySelector('[data-menu-key=\"pin\"]')||p.querySelector('[data-menu-key=\"mode:random\"]');if(!n)continue;n.focus();window.__miniMenuLeaving.push({inert:p.inert,focusBlocked:document.activeElement!==n});n.click();}}).observe(document.documentElement,{subtree:true,attributes:true,attributeFilter:['class']});return true;`);}
  async function openMenu(){
    if(!await mini(`return !!${root};`))await mini("const b=document.querySelector('[aria-label=\"小窗菜单\"]');if(!b||b.disabled)throw Error('menu unavailable');b.focus({preventScroll:true});b.click();return true;");
    await wait(()=>mini(`return !!${root}&&!${root}.querySelector('[role=\"status\"]');`),'menu read complete');
    await wait(()=>mini("return !!document.querySelector('.action-menu-host[data-lyric-context]');"),'native lyric context');
  }
  async function click(key){
    await mini(`const n=document.querySelector('[data-menu-key=${JSON.stringify(key)}]:not(:disabled)');if(!n)throw Error('menu choice '+${JSON.stringify(key)});n.scrollIntoView({block:'nearest'});n.focus({preventScroll:true});${key==='return'||key==='exit'?'setTimeout(()=>n.click(),80)':'n.click()'};return true;`);
  }
  async function key(value,selector='document.activeElement'){
    await mini(`const n=${selector};if(!n)throw Error('key target');n.dispatchEvent(new KeyboardEvent('keydown',{key:${JSON.stringify(value)},code:${JSON.stringify(value===' '?'Space':value)},bubbles:true,cancelable:true}));return true;`);
  }
  async function modes(){await openMenu();await click('mode');await wait(()=>mini(`return !!${child};`),'mode submenu');}
  async function closeMenu(){
    if(await mini(`return !!${child};`))await key('Escape');
    if(await mini(`return !!${root};`))await key('Escape');
    await wait(()=>mini(`return !${root}&&!${child};`),'menus closed');
  }
  async function prepare(){
    await invoke('player_action',{epoch,action:{kind:'pause',paused:true}});
    await invoke('player_action',{epoch,action:{kind:'seek',seconds:10}});
    await wait(()=>{const p=document.querySelector('.playerbar');return p?.dataset.state==='paused'&&Math.abs(Number(p.dataset.position)-10)<.2;},'paused10');
    playId=document.querySelector('.playerbar').dataset.playId;
    await wait(async()=>{const v=await read();return v.media.selection.audioPlayId===playId&&await mini("return document.querySelector('.mini-timeline span')?.textContent==='0:10';");},'mini paused projection');
  }
  async function settled(mode,family){
    await wait(async()=>{const v=await read();return v.media.mode===mode&&v.media.modeFamily===family&&await mini("return document.querySelector('[data-mini-mode]')?.getAttribute('aria-busy')==='false';");},'mode '+mode+'/'+family);
  }
  const facts=`const r=n=>{const b=n.getBoundingClientRect();return {left:b.left,top:b.top,right:b.right,bottom:b.bottom,width:b.width,height:b.height}};const root=${root},sub=${child},panels=[root,sub].filter(Boolean),active=document.activeElement;
    const inside=(a,b)=>a.left>=b.left-1&&a.right<=b.right+1&&a.top>=b.top-1&&a.bottom<=b.bottom+1;
    const rows=panels.map(p=>{const box=r(p);return {label:p.getAttribute('aria-label'),box,opacity:getComputedStyle(p).opacity,inert:p.inert,buttons:[...p.querySelectorAll('button')].map(n=>{const b=r(n),h=document.elementFromPoint(b.left+b.width/2,b.top+b.height/2);return {key:n.dataset.menuKey,label:n.textContent.trim(),disabled:n.disabled,checked:n.getAttribute('aria-checked'),visible:inside(b,box),hit:n===h||n.contains(h),box:b}})}});
    const focusFrame=active?.closest('.mini-header')&&active instanceof HTMLElement?(()=>{const b=r(active),s=getComputedStyle(active),outset=Math.max(0,parseFloat(s.outlineWidth)+parseFloat(s.outlineOffset));return {left:b.left-outset,top:b.top-outset,right:b.right+outset,bottom:b.bottom+outset}})():null;
    return {windowKind:'mini',captureTitle:'Cumulus · 迷你播放器',width:innerWidth,height:innerHeight,screenX,screenY,ratio:devicePixelRatio,theme:document.documentElement.dataset.theme,visible:document.visibilityState==='visible',focused:document.hasFocus(),overflow:document.documentElement.scrollWidth>innerWidth,
      root:!!root,submenu:!!sub,menus:rows,focusFrame,activeKey:active?.dataset?.menuKey??null,menuFocus:active===document.querySelector('[aria-label=\"小窗菜单\"]'),focusInside:panels.length?panels.some(p=>p.contains(active)):true,
      pin:document.querySelector('[data-mini-pin]')?.getAttribute('aria-pressed'),mode:document.querySelector('[data-mini-mode]')?.dataset.mode,family:document.querySelector('[data-mini-mode]')?.dataset.family,
      alert:document.querySelector('.mini-message[role=\"alert\"]')?.textContent?.trim()??null,notice:document.querySelector('.mini-message')?.textContent?.trim()??null,
      canTransport:!document.querySelector('.mini-play').disabled&&!document.querySelector('[aria-label=\"播放进度\"]').disabled,
      audit:window.__miniMenuLeaving??[],paintSamples:panels.length?panels.map(p=>({x:r(p).left+r(p).width/2,y:r(p).top+3,color:getComputedStyle(p).backgroundColor})):[{x:3,y:3,color:getComputedStyle(document.querySelector('.mini-shell')).backgroundColor}]};`;
  async function capture(name,main=false){
    last=name;let data;
    if(main){data={windowKind:'main',captureTitle:'Cumulus',width:innerWidth,height:innerHeight,screenX,screenY,ratio:devicePixelRatio,theme:document.documentElement.dataset.theme,visible:document.visibilityState==='visible',focused:document.hasFocus()};}
    else{
      await wait(()=>mini(`return [${root},${child}].filter(Boolean).every(n=>getComputedStyle(n).opacity==='1'&&!n.classList.contains('menu-enter-active'));`),'menus painted');
      data=await mini(facts);
      assert(!data.overflow&&data.focusInside,'contained menu focus');
      if(data.focusFrame)assert(data.focusFrame.left>=0&&data.focusFrame.top>=0&&data.focusFrame.right<=data.width&&data.focusFrame.bottom<=data.height,'header focus outline inside viewport');
      for(const p of data.menus){assert(p.box.left>=7&&p.box.top>=7&&p.box.right<=data.width-7&&p.box.bottom<=data.height-7,'contained menu box');assert(p.buttons.filter(b=>b.visible&&!b.disabled).every(b=>b.hit),'visible menu hits');}
      if(data.menus.length===2){const [a,b]=data.menus.map(p=>p.box);assert(a.right<=b.left||b.right<=a.left,'submenu does not cover parent');assert(data.menus[1].buttons.some(b=>b.key===data.activeKey),'submenu owns keyboard focus');}
    }
    const lyric=await status();
    const stage=`ux_${String(++count).padStart(2,'0')}_${name}`;window.__miniRecorded=null;
    await emit({kind:'snapshot',stage,miniMenuUi:true,expectedPlayId:playId,lyricMenuStatus:lyric,...data});
    await wait(()=>window.__miniRecorded===stage,'native snapshot '+name);
  }
  async function lyricVisible(value){await wait(async()=>{const v=await status();return v.visible===value&&!v.opening;},'lyric window visible '+value);await wait(()=>mini("return !document.querySelector('[aria-label=\"小窗菜单\"]').disabled;"),'lyric action completed');}
  try{
    await wait(()=>document.querySelector('.track-row'),'main startup');epoch=(await invoke('account_snapshot')).epoch;
    await invoke('play_selection',{epoch,selection:{kind:'playlist',id:'1',start_id:'1',quality:'standard'}});await openMini();await prepare();await theme('light');
    await openMenu();await capture('window_light');await theme('dark');await capture('window_dark');
    await modes();await capture('modes_dark');await key('Escape');await wait(()=>mini(`return !!${root}&&!${child};`),'Escape closes only submenu');await capture('child_escape_dark');
    await mini("const n=document.querySelector('[data-menu-key=\"mode\"]');n.dispatchEvent(new MouseEvent('mouseenter'));return true;");await wait(()=>mini(`return !!${child}&&document.activeElement?.dataset.menuKey==='mode';`),'hover child keeps parent focus');await key('End');await wait(()=>mini(`return !${child}&&document.activeElement?.dataset.menuKey==='exit';`),'parent End closes hovered child');await key('Home');
    await closeMenu();await wait(()=>mini("return document.activeElement===document.querySelector('[aria-label=\"小窗菜单\"]');"),'menu focus restored');await capture('menu_closed_dark');
    assert((await read()).preferences.alwaysOnTop,'outgoing menu replayed pin');assert((await read()).media.mode==='order_loop','outgoing submenu changed playback');
    await theme('light');await openMenu();await mini("const n=document.querySelector('[data-menu-key=\"mode\"]');n.focus();return true;");await key('ArrowRight');await wait(()=>mini(`return !!${child};`),'keyboard submenu');await capture('modes_keyboard_light');
    await click('mode:random');await settled('random','ordinary');await capture('selected_random_light');await modes();await capture('checked_random_light');await closeMenu();
    await openMenu();await click('pin');await wait(async()=>!(await read()).preferences.alwaysOnTop&&await mini("return document.querySelector('[data-mini-pin]')?.getAttribute('aria-pressed')==='false';"),'menu pin applied');await capture('pinned_off_light');
    await theme('dark');await openMenu();await capture('pinned_menu_dark');await closeMenu();
    const before=await read();assert(await invoke('mini_lyric_menu_read',{generation:before.generation,ticket:before.ticket}).then(()=>false,()=>true),'main cannot use mini lyric menu');assert(await invoke('mini_exit',{generation:before.generation}).then(()=>false,()=>true),'main cannot invoke mini exit');
    await fixture('lyric-assets',{blocked:true});await openMenu();await click('lyric:toggle');await wait(()=>mini("return document.querySelector('.mini-message')?.textContent.includes('正在更新桌面歌词');"),'lyric pending status');await wait(async()=>(await status()).opening,'native lyric opening');await capture('opening_lyrics_dark');
    assert(await mini("return !document.querySelector('.mini-play').disabled&&!document.querySelector('[aria-label=\"播放进度\"]').disabled;"),'lyric startup must preserve playback controls');
    await mini("const n=document.querySelector('[aria-label=\"播放进度\"]');n.value='10';n.dispatchEvent(new Event('change',{bubbles:true}));return true;");
    await wait(()=>mini("return !!document.querySelector('.mini-message[role=\"alert\"]');"),'real eight-second lyric timeout');await capture('startup_failed_dark');
    await openMenu();await capture('retry_menu_dark');await fixture('lyric-assets',{blocked:false});await click('lyric:retry');await lyricVisible(true);await theme('light');await openMenu();await capture('lyrics_open_light');
    const approved=await read();const context=await mini(`return await window.__TAURI_INTERNALS__.invoke('mini_lyric_menu_read',{generation:location.hash.slice(6),ticket:${JSON.stringify(approved.ticket)}});`);
    const again=await mini(`return await window.__TAURI_INTERNALS__.invoke('mini_lyric_menu_read',{generation:location.hash.slice(6),ticket:${JSON.stringify(approved.ticket)}});`);assert(context.ticket===again.ticket,'unchanged pure read invalidated displayed context');await wait(()=>mini(`return document.querySelector('.action-menu-host')?.dataset.lyricContext===${JSON.stringify(again.ticket)};`),'displayed lyric context current');
    await click('lyric:lock');await wait(async()=>(await status()).locked,'lyric locked');await openMenu();await capture('lyrics_locked_light');
    await click('lyric:unlock');await wait(async()=>!(await status()).locked,'lyric unlocked');await theme('dark');await openMenu();await capture('lyrics_unlocked_dark');
    await mini("document.querySelector('[data-menu-key=\"lyric:toggle\"]').focus();return true;");const old=await status();await invoke('desktop_lyric_change',{epoch,command:{kind:'change',expected_revision:old.preferences.current.revision,edit:{kind:'enabled',value:false}}});
    await wait(()=>mini("return document.querySelector('[data-menu-key=\"lyric:toggle\"]')?.textContent.trim()==='打开桌面歌词';"),'open menu follows external close');await capture('external_closed_dark');
    await click('lyric:toggle');await lyricVisible(true);await theme('light');await openMenu();await capture('external_reopened_light');await closeMenu();
    await fixture('lyric-storage',{fail:true});await theme('dark');await openMenu();await click('lyric:toggle');await wait(()=>mini("return !!document.querySelector('.mini-message[role=\"alert\"]');"),'save failure');await capture('save_failed_dark');
    await fixture('lyric-storage',{fail:false});await theme('light');await openMenu();await click('lyric:toggle');await lyricVisible(false);await capture('save_recovered_light');
    await fixture('lyric-storage',{corrupt:true});await theme('dark');await openMenu();await wait(()=>mini("return !!document.querySelector('[data-menu-key=\"lyric:repair\"]');"),'damaged repair');await capture('damaged_dark');
    await click('lyric:repair');await lyricVisible(false);await theme('light');await openMenu();await capture('repaired_light');await click('lyric:toggle');await lyricVisible(true);await capture('lyrics_reopened_light');
    await modes();await capture('heart_modes_light');await click('mode:intelligence');await wait(async()=>['seeded','active'].includes((await read()).media.intelligence),'intelligence started');await wait(async()=>(await read()).media.intelligence==='active','intelligence active');await prepare();await capture('heart_active_light');
    await theme('dark');await modes();await capture('heart_checked_dark');await click('mode:order_loop');await settled('order_loop','ordinary');await capture('heart_exit_dark');
    await invoke('together_command',{epoch,command:{kind:'create'}});await wait(async()=>(await read()).media.modeFamily==='together','room attached');await prepare();await theme('light');await modes();await capture('room_modes_light');await click('mode:random');await settled('random','together');await modes();await click('mode:order_loop');await settled('order_loop','together');await invoke('together_command',{epoch,command:{kind:'end',room_id:'synthetic-mini-room'}});await settled('order_loop','ordinary');
    await invoke('fm_command',{epoch,source:{kind:'personal'},command:{kind:'start',quality:'standard'}});await prepare();await theme('dark');await modes();await capture('fm_modes_dark');await click('mode:single_loop');await settled('single_loop','fm');await modes();await click('mode:sequential');await settled('sequential','fm');await modes();await invoke('player_action',{epoch,action:{kind:'next'}});await prepare();await wait(()=>mini(`return !${root}&&!${child};`),'source change closes old menu');await capture('source_changed_dark');
    await openMenu();await click('return');await wait(async()=>{const v=await fixture('windows');return v.main&&!v.mini.length;},'main returned');await theme('dark',true);await capture('returned_main_dark',true);await openMini();await theme('light');await openMenu();await capture('reentered_context_light');
    assert(await mini(`try{await window.__TAURI_INTERNALS__.invoke('mini_lyric_menu_action',{generation:${JSON.stringify(before.generation)},menuTicket:${JSON.stringify(context.ticket)},action:'toggle'});return false}catch{return true}`),'retired mini context accepted');
    assert(await mini("try{await window.__TAURI_INTERNALS__.invoke('account_snapshot');return false}catch{return true}"),'mini gained account IPC');
    if(window.__miniMenuExitProof){await key('End');await capture('exit_ready_light');await fixture('exit-armed',{miniMenuUi:true,stageCount:count,realMusicRequests:0,realAccountWrites:0});await click('exit');return;}
    const prior=await read();const priorContext=await mini(`return await window.__TAURI_INTERNALS__.invoke('mini_lyric_menu_read',{generation:location.hash.slice(6),ticket:${JSON.stringify(prior.ticket)}});`);await fixture('account');await wait(async()=>(await read()).media.epoch!==epoch,'new account');await wait(()=>mini(`return !${root}&&!${child};`),'old account menu closed');
    assert(await mini(`try{await window.__TAURI_INTERNALS__.invoke('mini_lyric_menu_action',{generation:${JSON.stringify(prior.generation)},menuTicket:${JSON.stringify(priorContext.ticket)},action:'toggle'});return false}catch{return true}`),'old account lyric action accepted');
    epoch=(await read()).media.epoch;playId=null;await capture('account_closed_light');
    assert(await mini("return window.__miniMenuLeaving.length>0&&window.__miniMenuLeaving.every(v=>v.inert&&v.focusBlocked);"),'outgoing menu inert and unfocusable');
    await emit({kind:'complete',stage:'complete',ok:true,miniMenuUi:true,stageCount:count,staleContextRejected:true,ipcIsolationVerified:true,twoAccountRealtimeVerified:false,realMusicRequests:0,realAccountWrites:0});
  }catch(error){await emit({kind:'failed',stage:'failed',message:String(error),stack:error?.stack,last,stageCount:count,miniFailure:await mini(facts).catch(()=>null)});}
})();
