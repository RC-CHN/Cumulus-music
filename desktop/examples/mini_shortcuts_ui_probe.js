// Synthetic fixture only. Local keys cross the real DOM/IPC; global keys come
// from a separate XTest/SendInput process with its own focused native window.
(async () => {
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const sleep = ms => new Promise(r => setTimeout(r, ms));
  const assert = (ok, text) => { if (!ok) throw Error(text); };
  const wait = async (fn, text) => { for (let i = 0; i < 140; i++) { if (await fn()) return; await sleep(100); } throw Error(text); };
  const emit = payload => invoke('plugin:event|emit', { event: 'mini-proof', payload });
  window.__miniAck = {};
  let serial = 0, stageCount = 0;
  const checks = [];
  const fixture = async (kind, payload = {}) => { const id = ++serial; await emit({ kind, id, ...payload }); await wait(() => id in window.__miniAck, `fixture ${kind}`); const result = window.__miniAck[id]; delete window.__miniAck[id]; if (result.error) throw Error(result.error); return result.value ?? result; };
  const mini = source => fixture('eval', { source });
  const read = () => mini(`return await window.__TAURI_INTERNALS__.invoke('mini_read',{generation:location.hash.slice(6)});`);
  const keySource = (code, ctrl = true, selector = 'body') => `const target=document.querySelector(${JSON.stringify(selector)});if(!target)throw Error('key target missing');target.focus();const event=new KeyboardEvent('keydown',{code:${JSON.stringify(code)},key:${JSON.stringify(code)},ctrlKey:${ctrl},bubbles:true,cancelable:true});target.dispatchEvent(event);return event.defaultPrevented;`;
  const mainKey = (code, ctrl = true, selector = 'body') => { const target=document.querySelector(selector); assert(target,'main key target'); target.focus(); const event=new KeyboardEvent('keydown',{code,key:code,ctrlKey:ctrl,bubbles:true,cancelable:true}); target.dispatchEvent(event); return event.defaultPrevented; };
  const miniKey = (code, ctrl = true, selector = 'body') => mini(keySource(code, ctrl, selector));
  // A closing window cannot acknowledge a later fixture message. Acknowledge
  // scheduling first, then assert the real native visibility transition.
  const returnKey = (code, selector = 'body') => mini(`setTimeout(()=>{${keySource(code,true,selector)}},100);return true;`);
  const globalKey = key => fixture('key', { key });
  const windows = () => fixture('windows');
  const ready = () => wait(async () => { const s=await invoke('shortcut_status');return s.phase==='ready'&&s.rows.some(r=>r.scope==='local'&&r.status.kind==='active'); }, 'shortcut listeners');
  const opened = async () => { await wait(async()=>!(await windows()).main && (await windows()).mini.length===1,'mini opened by shortcut'); await sleep(650); };
  const returned = async () => { await wait(async()=>(await windows()).main && !(await windows()).mini.length,'main returned by shortcut'); await ready(); await sleep(250); };
  const capture = async name => {
    await sleep(450);
    const data=await mini(`return {width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,body:document.body.textContent,overflow:document.documentElement.scrollWidth>innerWidth,buttons:[...document.querySelectorAll('button')].map(b=>{const r=b.getBoundingClientRect();return {label:b.getAttribute('aria-label')||b.textContent.trim(),x:r.x,right:r.right,width:r.width}})};`);
    assert(!data.overflow && data.buttons.every(b=>b.x>=0&&b.right<=data.width+1&&b.width>0),'mini controls clipped');
    await emit({kind:'snapshot',stage:`ux_${String(++stageCount).padStart(2,'0')}_${name}`,...data});
    await sleep(1100);
  };
  try {
    await wait(()=>document.querySelector('.track-row'),'main startup'); await ready();
    localStorage.setItem('cumulus.appearance.v1','light');
    window.dispatchEvent(new StorageEvent('storage',{key:'cumulus.appearance.v1',newValue:'light'}));
    await wait(()=>document.documentElement.dataset.theme==='light','light baseline');
    const account=await invoke('account_snapshot'), epoch=account.epoch;
    await invoke('play_selection',{epoch,selection:{kind:'songs',ids:['1','2','3'],index:0,quality:'standard'}});
    await sleep(500); await invoke('player_action',{epoch,action:{kind:'pause',paused:true}});
    assert(mainKey('KeyM'),'default main Ctrl+M consumed'); await opened();
    await capture('default_paused');
    assert(await miniKey('KeyP',true,'button'),'Ctrl+P on focused button consumed');
    await wait(async()=>(await read()).media.playing,'mini configured play');
    assert(!(await miniKey('Space',false,'button')),'button Space must remain native button activation');
    assert(await miniKey('KeyP'),'configured mini pause'); await wait(async()=>!(await read()).media.playing,'mini pause');
    const previousVolume=(await read()).media.volume;
    assert(await miniKey('ArrowDown',true,'[aria-label="音量"]'),'modified range key consumed');
    await wait(async()=>(await read()).media.volume<previousVolume,'mini volume');
    assert(!(await miniKey('ArrowDown',false,'[aria-label="音量"]')),'plain range key intercepted');
    await mini(`const input=document.createElement('input');input.id='proof-input';document.body.append(input);return true;`);
    assert(!(await miniKey('KeyM',true,'#proof-input')),'editing key intercepted');
    await mini(`document.querySelector('#proof-input').remove();const d=document.createElement('dialog');d.id='proof-modal';document.body.append(d);d.showModal();return true;`);
    assert(!(await miniKey('KeyM')),'modal key intercepted');
    await mini(`document.querySelector('#proof-modal').remove();return true;`);
    assert(await miniKey('KeyL'),'like key consumed'); await wait(async()=>(await read()).media.like?.observed===true,'like readback');
    await capture('local_controls'); checks.push('local_transport_volume_like_and_focus_guards');
    await returnKey('KeyM','button'); await returned();
    // Closing the mini must preserve the acknowledged main-window listener.
    assert(mainKey('KeyP'),'main listener survived mini close'); await sleep(450);
    assert(mainKey('KeyP'),'main pause after mini close'); await sleep(450);
    checks.push('main_listener_survives_mini_close');
    let settings=(await invoke('shortcut_settings',{epoch})).value.current;
    const bind=async(scope,keys)=>{ settings=(await invoke('shortcut_settings_command',{epoch,command:{kind:'change',expected_revision:settings.revision,edit:{kind:'bind',action:'mini_max',scope,keys}}})).value.current; await ready(); await sleep(200); };
    await bind('local',[17,81]); await bind('global',[17,18,81]);
    assert(!mainKey('KeyM'),'old main chord still active'); await sleep(250); assert((await windows()).main,'old chord opened mini');
    assert(mainKey('KeyQ'),'remapped main chord'); await opened();
    assert(!(await miniKey('KeyM')),'old mini chord still active');
    await mini(`localStorage.setItem('cumulus.appearance.v1','dark');window.dispatchEvent(new StorageEvent('storage',{key:'cumulus.appearance.v1',newValue:'dark'}));return true;`);
    await capture('dark_remapped'); checks.push('remap_shared_between_windows');
    const recording=await invoke('shortcut_begin_recording',{epoch});
    await wait(async()=>(await invoke('shortcut_status')).phase==='recording','recording pause');
    assert(!(await miniKey('KeyQ')),'mini local key active during recording');
    await globalKey('q'); await sleep(300); assert(!(await windows()).main,'global key active during recording');
    await capture('recording');
    await invoke('shortcut_end_recording',{id:recording}); await ready(); await sleep(500);
    await returnKey('KeyQ'); await returned();
    checks.push('recording_suspends_both_scopes_and_restores_listeners');
    await globalKey('m'); await sleep(350); assert((await windows()).main,'old OS chord active');
    await globalKey('q'); await opened(); await capture('global_open');
    await globalKey('p'); await wait(async()=>(await read()).media.playing,'background global play');
    await globalKey('p'); await wait(async()=>!(await read()).media.playing,'background global pause');
    await globalKey('q'); await returned();
    checks.push('independent_native_global_remap_and_transport');
    assert(mainKey('KeyQ'),'main listener after global return'); await opened();
    const prior=await read(), status=await invoke('shortcut_status');
    const lease=await mini(`return await window.__TAURI_INTERNALS__.invoke('shortcut_attach',{epoch:${JSON.stringify(epoch)},ticket:${JSON.stringify(status.ticket)}});`);
    const press={epoch,lease:lease.id,ticket:lease.ticket,keys:[17,81],selection:prior.media.selection};
    assert(await invoke('shortcut_press',{press}).then(()=>false,()=>true),'main impersonated mini lease');
    assert(await mini(`try{await window.__TAURI_INTERNALS__.invoke('shortcut_begin_recording',{epoch:${JSON.stringify(epoch)}});return false}catch{return true}`),'mini gained settings privilege');
    await fixture('account'); await wait(async()=>(await read()).media.epoch!==epoch,'account switch');
    assert(await mini(`try{await window.__TAURI_INTERNALS__.invoke('shortcut_press',{press:${JSON.stringify(press)}});return false}catch{return true}`),'old account mini-mode key accepted');
    await capture('account_empty'); checks.push('window_lease_and_account_isolation');
    await mini(`setTimeout(()=>document.querySelector('[aria-label="返回主窗口"]').click(),100);return true;`); await returned();
    await emit({kind:'complete',stage:'complete',ok:true,stageCount,checks,realMusicRequests:0,localDomKeys:true,independentGlobalKeys:true});
  } catch(e) { await emit({kind:'failed',stage:'failed',message:String(e)+'\n'+String(e?.stack||''),body:document.body.textContent,stageCount,checks}); }
})();
