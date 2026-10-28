// Explicit existing profile only. Native allowlist rejects all settings writes.
(async()=>{
  const sleep=ms=>new Promise(r=>setTimeout(r,ms)),all=s=>[...document.querySelectorAll(s)],one=s=>document.querySelector(s);
  const assert=(ok,message)=>{if(!ok)throw Error(message);};
  const invoke=window.__TAURI_INTERNALS__.invoke;
  const emit=(stage,rest={})=>invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...rest}});
  const wait=async(test,label)=>{for(let i=0;i<250;i++){if(test())return;await sleep(100);}throw Error(label);};
  const ready=()=>one('.settings-page')?.getAttribute('aria-busy')==='false';
  let stageCount=0;
  async function capture(name){await sleep(400);assert(document.documentElement.scrollWidth<=innerWidth,'horizontal overflow');const page=one('.settings-page').getBoundingClientRect(),main=one('.main-content').getBoundingClientRect();assert(page.bottom<=main.bottom+1,'settings clipped');await emit(`ux_${String(++stageCount).padStart(2,'0')}_${name}`,{width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,pageBottom:page.bottom,mainBottom:main.bottom,checked:all('.privacy-options input:checked').map(n=>({name:n.name,value:n.value})),readOnly:true});await sleep(900);}
  async function theme(value){const node=all('input[name="settings-theme"]').find(n=>n.value===value);node.scrollIntoView({block:'nearest'});node.click();await wait(()=>document.documentElement.dataset.theme===value,'theme');}
  try{
    await wait(()=>one('.account-button')&&!one('.account-button').disabled,'account ready');await sleep(350);one('[aria-label="打开设置"]').click();await wait(()=>ready()&&one('.privacy-options'),'settings read');assert(!one('.catalog-error'),'account settings read failed');
    await theme('light');await capture('live_appearance_light');
    one('input[name="shareSetting"]').scrollIntoView({block:'start'});await capture('live_privacy_light');
    one('.settings-linked summary').scrollIntoView({block:'center'});one('.settings-linked summary').click();await capture('live_linked_privacy');
    await theme('dark');one('input[name="playRecordSetting"]').scrollIntoView({block:'center'});await capture('live_privacy_dark');
    await emit('resize',{width:760,height:540});await wait(()=>innerWidth===760,'small window');await capture('live_small_dark');
    const account=await invoke('account_snapshot');const settings=await invoke('account_settings_page',{epoch:account.epoch,refresh:false});assert(settings.value.operations.length===0,'unexpected mutation in isolated read proof');
    await emit('complete',{ok:true,stageCount,readOnly:true,settings:settings.value,playbackVerified:false});
  }catch(error){await emit('failed',{ok:false,error:String(error),stageCount});}
})();
