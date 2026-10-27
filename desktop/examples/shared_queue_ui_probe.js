(async()=>{
  if(window.__sharedQueueStarted)return;window.__sharedQueueStarted=true;
  const invoke=(name,args={})=>window.__TAURI_INTERNALS__.invoke(name,args);
  const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const el=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)];
  const button=t=>all('button').find(n=>n.textContent.trim()===t);
  const report=(stage,data={})=>invoke('plugin:event|emit',{event:'shared-queue-proof',payload:{stage,...data}});
  const wait=async(test,label)=>{const until=Date.now()+30000;while(Date.now()<until){const v=await test();if(v)return v;await sleep(100);}throw Error(label);};
  const click=async node=>{if(!node||node.disabled)throw Error('button_unavailable');node.scrollIntoView({block:'nearest'});node.click();await sleep(150);};
  const capture=async(stage,data)=>{await sleep(400);await report(stage,{...data,width:innerWidth,height:innerHeight,horizontalOverflow:document.documentElement.scrollWidth>innerWidth});await sleep(1500);};
  let epoch,step='bootstrap';
  const view=async()=>(await invoke('together_view',{epoch,refresh:false})).value;
  try{
    await wait(()=>el('.account-button')?.textContent.includes('本地验证账号')&&!el('.account-button').disabled,'fixture_account');
    epoch=(await invoke('account_snapshot')).epoch;
    await invoke('play_selection',{epoch,selection:{kind:'songs',ids:['91','92','93'],index:0,quality:'standard'}});
    await click(button('一起听'));await wait(()=>button('创建房间'),'room_page');
    await click(button('创建房间'));
    await wait(async()=>{const v=await view();return v.connection.state==='connected'&&v.tracks.length===3&&v.targetId&&!v.pending.length;},'created');
    step='peer';await report('inject-peer');
    await wait(async()=>(await view()).sharedPhase==='applied','shared_joined');
    step='preview';await report('inject-queue');
    await wait(()=>all('.together-track-title').some(n=>n.textContent.includes('共同试听 00:40–00:50'))&&el('.playerbar')?.dataset.state==='paused'&&Number(el('.playerbar').dataset.position)===47,'preview_rendered');
    const limited=all('.together-track-title').find(n=>n.textContent.includes('对方需会员权益'));
    if(!limited?.disabled)throw Error('unplayable_button_enabled');
    const preview=all('.together-track-title').find(n=>n.textContent.includes('共同试听'));
    if(preview.disabled)throw Error('preview_button_disabled');
    if(!preview.closest('.together-track').classList.contains('current'))throw Error('native_current_row_not_marked');
    el('.together-queue').scrollIntoView({block:'start'});
    await capture('shared_preview',{blockedDisabled:true,previewEnabled:true,position:47,state:'paused',fixtureAudio:true});
    step='blocked';await report('inject-blocked');
    await wait(()=>all('.together-track-title').length===3&&all('.together-track-title').every(n=>n.disabled)&&el('.playerbar')?.dataset.state==='idle','all_blocked_stopped');
    await capture('shared_blocked',{allDisabled:true,nativeState:'idle'});
    step='exit';await click(button('退出房间'));await click(button('确认退出'));
    await wait(async()=>{const v=await view();return !v.room&&!v.localRecovery;},'clean_exit');
    await capture('shared_exit',{roomAbsent:true,localRestored:true});
    await report('complete',{ok:true,scope:'production WebView and IPC with local API/IM/audio fixtures'});
  }catch(error){await report('failed',{step,error:String(error),pageErrors:all('.together-page [role="alert"]').map(n=>n.textContent)});}
})();
