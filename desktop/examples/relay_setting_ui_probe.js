(async()=>{
 const sleep=ms=>new Promise(r=>setTimeout(r,ms)),all=s=>[...document.querySelectorAll(s)],one=s=>document.querySelector(s);
 const assert=(ok,message)=>{if(!ok)throw Error(message);},emit=(stage,rest={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...rest}});
 const wait=async(test,label)=>{for(let i=0;i<200;i++){if(test())return;await sleep(100);}throw Error(label);};
 const click=n=>{assert(n&&!n.disabled,'button unavailable');n.scrollIntoView({block:'nearest'});n.focus({preventScroll:true});n.click();};
 const button=text=>all('.relay-settings button').find(n=>n.textContent.trim()===text),check=()=>one('[aria-label="开启跨端续播"]');
 const ready=()=>one('.relay-settings')?.getAttribute('aria-busy')==='false';
 let stageCount=0;
 async function capture(name){if(!one('dialog[open]'))check().scrollIntoView({block:'center'});await sleep(250);const bar=one('.playerbar').getBoundingClientRect(),scroll=one('.settings-scroll').getBoundingClientRect();assert(scroll.bottom<=bar.top+1,'settings overlaps player');assert(document.documentElement.scrollWidth<=innerWidth,'horizontal overflow');assert(!document.body.textContent.includes('SYNTHETIC_PRIVATE'),'private device exposed');await emit(`ux_${String(++stageCount).padStart(2,'0')}_${name}`,{width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,checked:check().checked,disabled:check().disabled,busy:!ready(),dialog:!!one('dialog[open]'),pending:all('.relay-setting-pending').length,scrollBottom:scroll.bottom,playerTop:bar.top});await sleep(850);}
 async function theme(value){click(one(`input[name="settings-theme"][value="${value}"]`));await wait(()=>document.documentElement.dataset.theme===value,'theme');}
 async function mode(value){await emit('fixture',{mode:value});}
 async function refresh(value){await mode(value);click(button('检查其他设备'));await sleep(100);await wait(ready,'refresh');}
 async function toggle(value){const previous=check().checked;click(check());assert(check().checked===previous,'optimistic checkbox state');await sleep(100);await wait(ready,'write completion');if(value!==null)assert(check().checked===value,'unexpected confirmed switch');}
 try{
  await wait(()=>one('.account-button')&&!one('.account-button').disabled,'account');click(one('[aria-label="打开设置"]'));await wait(()=>check()&&ready(),'setting control');one('.relay-switch').scrollIntoView({block:'center'});await theme('light');assert(!check().checked&&!check().disabled,'initial off');await capture('initial_light');
  await theme('dark');await capture('dark');await emit('resize',{width:760,height:540});await wait(()=>innerWidth===760,'small window');await capture('small_dark');await theme('light');await capture('small_light');
  click(check());assert(!check().checked,'optimistic enable');await sleep(60);assert(check().disabled,'pending switch enabled');await capture('pending_enable');await wait(ready,'enable');assert(check().checked,'enable not confirmed');await capture('enabled');
  await toggle(false);await capture('disabled');
  await refresh('setting-rejected');await toggle(false);assert(one('.relay-setting-result').textContent.includes('拒绝'),'rejection missing');await capture('rejected');
  await refresh('setting-lost-unapplied');await toggle(false);assert(check().disabled&&one('.relay-setting-pending'),'lost request unresolved');await capture('lost_unapplied');
  click(button('核对设备开关'));await sleep(100);await wait(ready,'readback unapplied');assert(button('保留当前设置'),'accept unavailable');await capture('readback_unapplied');
  click(button('保留当前设置'));await wait(()=>one('dialog[open]'),'accept dialog');await capture('accept_dialog');click(all('dialog[open] button').find(n=>n.textContent.trim()==='取消'));await wait(()=>!one('dialog[open]'),'cancel dialog');assert(document.activeElement===button('保留当前设置'),'accept cancel focus');await capture('accept_cancelled');
  click(button('保留当前设置'));await wait(()=>one('dialog[open]'),'accept dialog again');click(all('dialog[open] button').find(n=>n.textContent.trim()==='确认'));await wait(()=>!one('dialog[open]')&&ready(),'accepted current');assert(!check().checked&&!check().disabled&&!one('.relay-setting-pending'),'accept did not release intent');await capture('accepted_current');
  await refresh('setting-lost-applied');await toggle(false);assert(one('.relay-setting-pending'),'applied lost not unresolved');await capture('lost_applied');click(button('核对设备开关'));await sleep(100);await wait(ready,'readback applied');assert(check().checked&&!one('.relay-setting-pending'),'readback applied');await capture('confirmed_applied');
  await refresh('setting-read-failed');await toggle(true);assert(one('.relay-setting-pending'),'failed read lost intent');await capture('read_failed');await mode('setting-normal');click(button('核对设备开关'));await sleep(100);await wait(ready,'recovered read');assert(!check().checked,'recovered false');await capture('read_recovered');
  await refresh('setting-missing');assert(check().disabled&&one('.relay-switch').textContent.includes('未返回'),'missing switch');await capture('missing');await refresh('excluded');assert(check().disabled,'excluded setting writable');await capture('excluded');
  await refresh('offline');assert(check().disabled&&one('.relay-error'),'offline allows write');await capture('offline');await refresh('setting-normal');assert(!check().checked&&!check().disabled,'restored off');await capture('restored');
  await emit('complete',{ok:true,stageCount,realMusicRequests:0,playbackVerified:false,settingReadbackVerified:true,noOptimisticCheckbox:true,acceptCancelFocus:true});
 }catch(error){await emit('failed',{ok:false,error:String(error),stageCount});}
})();
