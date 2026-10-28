// Explicit native profile. The transport refuses all relay setting mutations.
(async()=>{
 const sleep=ms=>new Promise(r=>setTimeout(r,ms)),one=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)];
 const assert=(ok,message)=>{if(!ok)throw Error(message);},invoke=window.__TAURI_INTERNALS__.invoke;
 const emit=(stage,rest={})=>invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...rest}});
 const wait=async(test,label)=>{for(let i=0;i<250;i++){if(test())return;await sleep(100);}throw Error(label);};
 const check=()=>one('[aria-label="开启跨端续播"]');let stageCount=0;
 async function capture(name){check().scrollIntoView({block:'center'});await sleep(350);assert(document.documentElement.scrollWidth<=innerWidth,'horizontal overflow');const account=await invoke('account_snapshot'),page=await invoke('relay_view',{epoch:account.epoch,refresh:false}),state=page.value.settings;assert(state&&typeof state.enabled==='boolean','device setting unavailable');assert(state.operations.length===0,'unexpected live setting mutation');assert(check().checked===state.enabled&&check().disabled===!state.canChange,'UI differs from native setting');await emit(`ux_${String(++stageCount).padStart(2,'0')}_${name}`,{width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,readOnly:true,enabled:state.enabled,canChange:state.canChange,settingRevision:state.revision,availability:page.value.availability});await sleep(900);}
 async function theme(value){const node=one(`input[name="settings-theme"][value="${value}"]`);node.scrollIntoView({block:'nearest'});node.click();await wait(()=>document.documentElement.dataset.theme===value,'theme');}
 try{
  await wait(()=>one('.account-button')&&!one('.account-button').disabled,'account');one('[aria-label="打开设置"]').click();await wait(()=>check()&&one('.relay-settings')?.getAttribute('aria-busy')==='false','device setting');assert(!one('.relay-error'),'live relay read failed');
  await theme('light');await capture('live_light');await theme('dark');await capture('live_dark');await emit('resize',{width:760,height:540});await wait(()=>innerWidth===760,'small window');await capture('live_small_dark');await theme('light');await capture('live_small_light');
  await emit('complete',{ok:true,stageCount,readOnly:true,settingsWrites:0,playbackVerified:false});
 }catch(error){await emit('failed',{ok:false,error:String(error),stageCount});}
})();
