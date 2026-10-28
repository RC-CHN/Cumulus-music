(async()=>{
  const one=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)],sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const invoke=(name,args={})=>window.__TAURI_INTERNALS__.invoke(name,args);
  const emit=(stage,extra={})=>invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...extra}});
  const assert=(ok,message)=>{if(!ok)throw Error(message);};
  const wait=async(fn,label)=>{for(let i=0;i<200;i++){if(await fn())return;await sleep(100);}throw Error(label);};
  const click=n=>{assert(n&&!n.disabled,'unavailable control');n.scrollIntoView({block:'nearest'});n.focus({preventScroll:true});n.click();};
  const section=()=>one('.window-behavior'),refresh=()=>one('[aria-label="重新读取关闭设置"]'),dialog=()=>one('.window-close-dialog[open]');
  const ready=()=>section()?.getAttribute('aria-busy')==='false';
  const state=()=>invoke('window_lifecycle');
  let count=0;
  async function capture(name){
    if(!dialog())section().scrollIntoView({block:'center'});await sleep(250);
    assert(document.documentElement.scrollWidth<=innerWidth,'page overflow');
    const panel=dialog()||section(),box=panel.getBoundingClientRect();
    assert(box.left>=0&&box.right<=innerWidth+1,'panel overflow');
    if(dialog())for(const n of dialog().querySelectorAll('.close-choices input')){
      const input=n.getBoundingClientRect(),label=n.nextElementSibling.getBoundingClientRect();
      assert(input.width<=18&&input.height<=18&&input.right<=label.left&&Math.abs((input.top+input.bottom-label.top-label.bottom)/2)<2,'radio label alignment');
    }
    await emit(`ux_${String(++count).padStart(2,'0')}_${name}`,{theme:document.documentElement.dataset.theme,width:innerWidth,height:innerHeight,focus:document.activeElement?.getAttribute('aria-label')||document.activeElement?.textContent,dialog:!!dialog(),error:!!panel.querySelector('[role="alert"]')});await sleep(500);
  }
  async function read(){click(refresh());await sleep(100);await wait(ready,'settings read');}
  async function close(){click(one('.window-close'));await wait(()=>dialog(),'close prompt');}
  async function cancel(){click(dialog().querySelector('[aria-label="取消关闭"]'));await wait(()=>!dialog(),'close cancelled');await sleep(300);assert(document.activeElement===one('.window-close'),'cancel focus not restored');}
  async function setting(value){const n=one(`input[name="settings-close-behavior"][value="${value}"]`);click(n);await sleep(120);await wait(ready,'choice saved');assert(n.checked,'native setting not reflected');assert(document.activeElement===n,'setting focus lost');}
  try {
    await wait(()=>one('[aria-label="打开设置"]')&&!one('[aria-label="打开设置"]').disabled,'app ready');click(one('[aria-label="打开设置"]'));
    await wait(()=>ready()&&refresh()&&!refresh().disabled,'window settings');
    click(one('input[name="settings-theme"][value="light"]'));await capture('default_light');
    await close();assert(dialog().querySelector('[value="tray"]').disabled,'missing host accepted');assert(dialog().querySelector('.primary').disabled,'missing host confirm enabled');await capture('no_host_prompt');await cancel();
    await setting('exit');await setting('tray');await capture('setting_saved');
    await emit('host_enable');await wait(async()=> (await state()).trayAvailable,'host registration accepted');
    await emit('resize');await wait(()=>innerWidth===760,'small window');click(one('input[name="settings-theme"][value="dark"]'));
    await wait(()=>one('.playerbar')?.dataset.state==='paused','synthetic audio restored');click(one('.playerbar [aria-label="播放"]'));await wait(()=>one('.playerbar')?.dataset.state==='playing','native playing');
    await close();assert(!dialog().querySelector('[value="tray"]').disabled,'host not usable');click(dialog().querySelector('[name="close-remember"]'));await capture('dark_prompt');click(dialog().querySelector('.primary'));
    await wait(async()=>{const v=await state();return v.hidden&&!v.busy;},'native hide acknowledged');await emit('hidden_first');
    for(const [stateName,stage] of [['paused','media_paused'],['playing','media_resumed'],['idle','media_stopped'],['playing','media_restarted']]) {
      await wait(()=>one('.playerbar')?.dataset.state===stateName,'native tray '+stage);
      assert((await state()).hidden,'tray transport unexpectedly revealed window');
      await emit(stage,{ok:true,playerState:stateName});
    }
    await wait(async()=>!(await state()).hidden,'native tray Show');click(one('.playerbar [aria-label="暂停"]'));await wait(()=>one('.playerbar')?.dataset.state==='paused','pause after native Show');await read();assert(one('[name="settings-close-remember"]').checked,'remember not persisted');await capture('shown_from_menu');
    click(one('.window-close'));await wait(async()=>{const v=await state();return v.hidden&&!v.busy&&!v.prompt;},'remembered tray close');await emit('hidden_second');
    await wait(async()=>{const v=await state();return !v.hidden&&!v.trayAvailable;},'host loss restored window');await capture('host_lost_restored');
    await close();assert(dialog().querySelector('[value="tray"]').disabled,'remembered hide ignored unavailable host');await capture('unavailable_remembered');await cancel();
    await emit('host_restart');await wait(async()=> (await state()).trayAvailable,'watcher restart');await capture('host_restarted');
    await emit('corrupt');await read();assert(section().querySelector('.settings-recovery'),'damage hidden');await capture('damaged_settings');
    await close();assert(dialog().querySelector('[name="close-remember"]').disabled,'damage allows remember');await capture('damaged_prompt');await cancel();
    click(all('.window-behavior button').find(n=>n.textContent.trim()==='恢复默认关闭设置'));await wait(()=>ready()&&one('[name="settings-close-remember"]'),'repair');assert(!one('[name="settings-close-remember"]').checked,'repair did not restore reminder');await capture('repaired');
    await emit('reject-write');click(one('[name="settings-close-behavior"][value="exit"]'));await wait(()=>ready()&&section().querySelector('.catalog-error'),'write failure');assert(one('[name="settings-close-behavior"][value="tray"]').checked,'failed write changed selection');await capture('save_failed');
    await emit('allow-write');await read();await capture('save_recovered');
    await close();click(dialog().querySelector('[value="exit"]'));click(dialog().querySelector('[name="close-remember"]'));await capture('exit_prompt');
    await emit('complete',{ok:true,states:count,realMusicRequests:0,physicalPanelVerified:false});click(dialog().querySelector('.primary'));
  }catch(error){await emit('failed',{ok:false,error:String(error),states:count,body:document.body.innerText.slice(-1800)});}
})();
