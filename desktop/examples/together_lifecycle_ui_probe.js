(async()=>{
  if(window.__lifecycleStarted)return;window.__lifecycleStarted=true;
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
  const restored=async()=>{
    const v=await view(), bar=el('.playerbar');
    return !v.room&&!v.attached&&!v.localRecovery&&v.lifecycleNotice&&bar?.dataset.speed==='1.25'
      &&bar.dataset.state==='paused'&&Number(bar.dataset.position)===47
      &&el('.playerbar [aria-label="顺序播放"]')
      &&all('.together-page [role="status"]').some(n=>n.textContent.includes('已恢复本机播放设置'));
  };
  try{
    await wait(()=>el('.account-button')?.textContent.includes('本地验证账号')&&!el('.account-button').disabled,'fixture_account');
    epoch=(await invoke('account_snapshot')).epoch;
    await invoke('play_selection',{epoch,selection:{kind:'songs',ids:['91','92','93'],index:0,quality:'standard'}});
    await invoke('player_action',{epoch,action:{kind:'speed',value:1.25}});
    await invoke('player_action',{epoch,action:{kind:'mode',mode:'sequential'}});
    await click(button('一起听'));await wait(()=>button('创建房间'),'room_page');
    for(const scenario of ['auto','lost']){
      step=scenario+'_create';await click(button('创建房间'));
      await wait(async()=>{const v=await view();return v.connection.state==='connected'&&v.tracks.length===3&&v.targetId&&!v.pending.length;},'created');
      await report('inject-peer');await wait(async()=>(await view()).sharedPhase==='applied','joined');
      await report('inject-preview-only');
      await wait(()=>all('.together-track-title').some(n=>n.textContent.includes('共同试听 00:40–00:50'))&&el('.playerbar')?.dataset.state==='paused'&&Number(el('.playerbar').dataset.position)===47,'preview');
      if(scenario==='auto'){
        el('.together-queue').scrollIntoView({block:'start'});
        await capture('lifecycle_preview',{position:47,speed:Number(el('.playerbar').dataset.speed)});
      }
      step=scenario+'_end';await report(scenario==='auto'?'inject-end':'inject-lost-end');
      if(scenario==='lost'){
        await wait(async()=>{const v=await view();return v.ending&&v.pending.length===1&&v.connection.state==='closed'&&button('核对状态');},'pending_end');
        await wait(()=>button('重新同步当前播放')?.disabled,'ending_controls_disabled');
        el('.together-pending').scrollIntoView({block:'center'});
        await capture('lifecycle_pending',{ending:true,pending:1,connection:'closed',syncDisabled:true});
        await click(button('核对状态'));
      }
      await wait(restored,'automatic_local_restore');
      el('.together-heading').scrollIntoView({block:'start'});
      await capture(scenario==='auto'?'lifecycle_auto':'lifecycle_recovered',{roomAbsent:true,localRestored:true,position:47,speed:1.25,mode:'sequential'});
    }
    await report('complete',{ok:true,scope:'actual WebView with fixture API/IM/audio; automatic end and lost-ack read-only recovery'});
  }catch(error){await report('failed',{step,error:String(error),pageErrors:all('.together-page [role="alert"]').map(n=>n.textContent)});}
})();
