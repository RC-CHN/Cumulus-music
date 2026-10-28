(async()=>{
 const sleep=ms=>new Promise(r=>setTimeout(r,ms)),all=s=>[...document.querySelectorAll(s)],one=s=>document.querySelector(s);
 const assert=(ok,message)=>{if(!ok)throw Error(message);};
 const original=window.__TAURI_INTERNALS__.invoke;
 let fault='',delayed,readFailure=false,reconnect=false,stageCount=0;
 let nativeCommand;
 const emit=(stage,rest={})=>original('plugin:event|emit',{event:'ux-proof',payload:{stage,...rest}});
 const wait=async(test,label)=>{for(let i=0;i<200;i++){if(test())return;await sleep(100);}throw Error(label);};
 const button=text=>all('.relay-settings button').find(n=>n.textContent.trim()===text);
 const focusChecks=[];
 const focus=reason=>focusChecks.push({reason,passed:document.activeElement===button('刷新同步状态'),actual:document.activeElement===document.body?'BODY':document.activeElement?.textContent?.trim()});
 const click=n=>{assert(n&&!n.disabled,'button unavailable');n.scrollIntoView({block:'nearest'});n.focus({preventScroll:true});n.click();};
 const ready=()=>one('.relay-publication')?.getAttribute('aria-busy')==='false';
 const status=()=>one('.relay-publication [role="status"]')?.textContent??'';
 const state=()=>one('.playerbar')?.dataset.state;
 async function capture(name,playing=false){
   if(!one('dialog[open]'))one('.relay-publication').scrollIntoView({block:'center'});
   await sleep(300);assert(document.documentElement.scrollWidth<=innerWidth,'horizontal overflow');assert(!document.body.textContent.includes('SYNTHETIC_PRIVATE'),'private native data leaked');
   const bar=one('.playerbar').getBoundingClientRect(),area=one('.settings-scroll').getBoundingClientRect();assert(area.bottom<=bar.top+1,'settings overlaps player');
   if(one('dialog[open]')){const r=one('dialog[open]').getBoundingClientRect();assert(r.top>=0&&r.bottom<=innerHeight,'confirmation out of window');}
   await emit(`ux_${String(++stageCount).padStart(2,'0')}_${name}`,{width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,status:status(),playerState:state(),playing,dialog:!!one('dialog[open]'),scrollBottom:area.bottom,playerTop:bar.top});await sleep(850);
 }
 async function theme(value){click(one(`input[name="settings-theme"][value="${value}"]`));await wait(()=>document.documentElement.dataset.theme===value,'theme');}
 async function confirm(){await wait(()=>button('重新同步当前队列')&&!button('重新同步当前队列').disabled,'recovery enabled');click(button('重新同步当前队列'));await wait(()=>one('dialog[open]'),'confirmation');click(all('dialog[open] button').find(n=>n.textContent.trim()==='确认'));await wait(()=>!one('dialog[open]')&&ready(),'confirmation completed');}
 async function play(){await wait(()=>one('.relay-settings')?.getAttribute('aria-busy')==='false','pull ready');click(button('检查其他设备'));await sleep(150);await wait(()=>one('.relay-settings')?.getAttribute('aria-busy')==='false','pull refreshed');click(one('.relay-settings-device button'));await wait(()=>state()==='playing'&&Number(one('.playerbar').dataset.position)>0.1,'native playback');}
 async function stop(){click(one('[aria-label="停止播放"]'));await wait(()=>state()==='idle','audio stopped');}
 try{
   await wait(()=>one('#app')?.__vue_app__?._context?.provides,'typed client provider');
   const provides=one('#app').__vue_app__._context.provides;
   const core=provides[Reflect.ownKeys(provides).find(key=>typeof key==='symbol'&&key.description==='core-client')];
   assert(core?.relayPublicationCommand,'typed publication adapter');
   nativeCommand=core.relayPublicationCommand;
   const nativeRead=core.relayPublicationView;
   core.relayPublicationView=async(...args)=>{if(readFailure)throw {code:'network',message:'合成状态读取失败'};return nativeRead(...args);};
   const intercept=async(epoch,command)=>{
     if(reconnect){reconnect=false;await emit('fixture',{mode:'publication-ok'});}
     if(command.kind==='resynchronize'&&fault){const mode=fault;fault='';if(mode==='before')delayed=[epoch,command];else await nativeCommand(epoch,command);throw {code:'network',message:'合成操作应答丢失'};}
     return nativeCommand(epoch,command);
   };
   core.relayPublicationCommand=intercept;assert(core.relayPublicationCommand===intercept,'adapter fault injection unavailable');
   await wait(()=>one('.account-button')&&!one('.account-button').disabled,'account');click(one('[aria-label="打开设置"]'));await wait(()=>ready()&&one('input[name="shareSetting"]'),'settings');await theme('light');await capture('initial_light');
   await play();await wait(()=>status().includes('拒绝'),'rejected publication');await capture('blocked_playing',true);await stop();
   await theme('dark');await capture('blocked_dark');await emit('resize',{width:760,height:540});await wait(()=>innerWidth===760,'small');await capture('small_dark');
   await wait(()=>button('重新同步当前队列')&&!button('重新同步当前队列').disabled,'recovery');click(button('重新同步当前队列'));await wait(()=>one('dialog[open]'),'confirm');await capture('confirm');click(all('dialog[open] button').find(n=>n.textContent.trim()==='取消'));await wait(()=>!one('dialog[open]'),'cancel');assert(document.activeElement===button('重新同步当前队列'),'cancel focus');await capture('cancelled_dialog');
   fault='before';await confirm();await wait(()=>button('核对同步结果'),'unknown request');await capture('unknown_ipc');click(button('核对同步结果'));await wait(()=>ready()&&!button('核对同步结果'),'unknown resolved');assert(one('.relay-publication').textContent.includes('已取消尚未开始'),'cancel receipt');focus('removed reconciliation entry');await capture('cancelled_unseen');
   let fenced=false;try{await nativeCommand(...delayed);}catch{fenced=true;}assert(fenced,'late IPC escaped cancellation');
   await emit('fixture',{mode:'publication-ok'});fault='after';await confirm();await wait(()=>status().includes('已同步')&&!button('核对同步结果'),'lost ack readback');assert(state()==='idle','recovery restarted audio');assert(document.activeElement===button('刷新同步状态'),'recovery focus fallback');await capture('recovered_lost_ack');
   await emit('fixture',{mode:'publication-lost'});await play();await wait(()=>status().includes('未确认同步结果'),'uncertain publication');await capture('uncertain_playing',true);await stop();
   await emit('fixture',{mode:'publication-prepared-offline'});await confirm();await wait(()=>status().includes('等待连接恢复')&&button('重试连接'),'preflight offline');await capture('prepared_offline');
   reconnect=true;await wait(()=>button('重试连接')&&!button('重试连接').disabled,'retry ready');click(button('重试连接'));await wait(()=>status().includes('已同步')&&ready(),'prepared resumed');assert(state()==='idle','resume restarted audio');focus('removed retry entry');await capture('recovered');
   readFailure=true;await wait(ready,'local read ready');click(button('刷新同步状态'));await wait(()=>ready()&&one('.relay-publication .relay-error'),'local read failure');focus('read failure');await capture('read_failure');readFailure=false;await wait(ready,'read retry ready');click(button('刷新同步状态'));await wait(()=>ready()&&!one('.relay-publication .relay-error'),'local read recovered');focus('read recovery');await theme('light');await capture('read_recovered');
   assert(focusChecks.every(row=>row.passed),'publication recovery focus checks failed');
   await emit('complete',{ok:true,stageCount,realMusicRequests:0,playbackVerified:true,syntheticQueueApplications:2,decoder:'libmpv',audioOutput:'null',mediaSubstitution:'exact synthetic CDN URL to generated PCM',cancelFocus:true,lateIpcFenced:fenced,lostAckReadback:true,recoveryDidNotRestartAudio:state()==='idle',focusChecks});
 }catch(error){await emit('failed',{ok:false,error:String(error),stageCount,focusChecks});}
})();
