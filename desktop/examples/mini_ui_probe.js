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
    return { width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,body:document.body.textContent,
      overflow:document.documentElement.scrollWidth>innerWidth,buttons:[...document.querySelectorAll('button')].map(n=>({label:n.getAttribute('aria-label')||n.textContent.trim(),...r(n)})),
      rows:document.querySelectorAll('.mini-queue-rows li').length,background:getComputedStyle(document.querySelector('.mini-shell')).backgroundColor };`;
  const capture = async name => { await sleep(550); const data=await mini(facts); assert(!data.overflow, 'mini horizontal overflow'); assert(data.buttons.every(b=>b.x>=0 && b.right<=data.width+1 && b.width>0), 'mini horizontal controls'); await emit({kind:'snapshot',stage:`ux_${String(++stageCount).padStart(2,'0')}_${name}`,...data}); await sleep(1300); };
  // A return/close destroys this WebView; acknowledge the fixture before
  // scheduling the click, then let callers verify the actual native transition.
  const clickMini = label => mini(`const b=[...document.querySelectorAll('button')].find(b=>b.getAttribute('aria-label')===${JSON.stringify(label)});if(!b||b.disabled)throw Error('mini button unavailable: '+${JSON.stringify(label)});${['返回主窗口','关闭窗口'].includes(label) ? 'setTimeout(()=>b.click(),100)' : 'b.click()'};return true;`);
  const read = () => mini(`return await window.__TAURI_INTERNALS__.invoke('mini_read',{generation:location.hash.slice(6)});`);
  const open = async () => { await invoke('mini_enter'); await wait(async()=>!(await fixture('windows')).main,'main hidden after mini ready'); await wait(async()=>(await read()).media.item,'mini populated'); };
  try {
    await wait(()=>document.querySelector('.track-row'),'main startup');
    let account = await invoke('account_snapshot');
    await invoke('play_selection',{epoch:account.epoch,selection:{kind:'songs',ids:Array.from({length:40},(_,i)=>String(i+1)),index:0,quality:'standard'}});
    await sleep(500);
    await invoke('player_action',{epoch:account.epoch,action:{kind:'pause',paused:true}});
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
    await mini(`const input=document.querySelector('[aria-label="音量"]');input.value='28';input.dispatchEvent(new Event('change',{bubbles:true}));return true;`);
    await wait(async()=>(await read()).media.volume===28,'volume');
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
    await wait(async()=>(await read()).media.item?.id!==stale.media.item.id,'new track after stale action');
    await invoke('player_action',{epoch:account.epoch,action:{kind:'pause',paused:true}});
    await mini(`localStorage.setItem('cumulus.appearance.v1','dark');window.dispatchEvent(new StorageEvent('storage',{key:'cumulus.appearance.v1',newValue:'dark'}));return true;`);
    await capture('dark_queue');
    await clickMini('始终置顶'); await wait(async()=>(await read()).preferences.alwaysOnTop===false,'pin persistence');
    await clickMini('播放队列'); await capture('dark_compact');
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
    await emit({kind:'complete',stage:'complete',ok:true,stageCount,realMusicRequests:0,timeoutVerified:true,staleControlRejected:true,ipcIsolation:true,reloadFallback:true,sharedAudio:true,closePolicy:true,accountIsolation:true});
  } catch(e) { await emit({kind:'failed',stage:'failed',message:String(e)+'\n'+String(e?.stack||''),body:document.body.textContent,stageCount}); }
})();
