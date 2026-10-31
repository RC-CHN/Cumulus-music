// Actual registered native trial IPC and libmpv, not a consent-dialog proof.
// All service responses/quota writes are synthetic and isolated by the host.
(async () => {
  const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const one=s=>document.querySelector(s);
  const invoke=(...args)=>(window.__CUMULUS_PROOF_INVOKE__??window.__TAURI_INTERNALS__.invoke)(...args);
  const emit=(stage,value={})=>invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...value}});
  const assert=(ok,label)=>{if(!ok)throw Error(label);};
  const wait=async(test,label)=>{for(let i=0;i<240;i++){if(test())return;await sleep(70);}throw Error(label);};
  const bar=()=>one('.playerbar');
  const click=node=>{assert(node&&!node.disabled,'control unavailable');node.focus();node.click();};
  let epoch='',count=0,operation='';
  const details=()=>invoke('player_quality_details',{epoch,playId:bar().dataset.playId});
  const trial=command=>invoke('player_quality_trial',{epoch,command});
  async function fixture(kind){window.__qualityFixtureAck=null;await emit('quality-fixture',{kind});await wait(()=>window.__qualityFixtureAck,'fixture acknowledgement');}
  async function capture(label,extra={}) {
    await wait(()=>bar().dataset.state==='paused'&&Math.abs(Number(bar().dataset.position)-10)<.2,'native pause and seek retained');
    await sleep(200);
    assert(bar().dataset.quality==='standard','actual stream remains honest about downgrade');
    await emit(`ux_${String(++count).padStart(2,'0')}_${label}`,{...extra,playId:bar().dataset.playId,
      requested:bar().dataset.requestedQuality,preferred:bar().dataset.preferredQuality,
      actual:bar().dataset.quality,state:bar().dataset.state,position:Number(bar().dataset.position),
      width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme});
  }
  try {
    await wait(()=>bar()&&one('.track-row .track-play:not(:disabled)'),'ready');
    epoch=(await invoke('account_snapshot')).epoch;
    if(window.__trialReopen) {
      await wait(()=>bar().dataset.state==='paused'&&bar().dataset.preferredQuality==='jymaster','restored bound trial');
      const info=await details();assert(info.trials.length===1&&info.trials[0].canUse,'restored active journal');
      operation=info.trials[0].operation;await capture('reopened_active');
      await fixture('trial-expire');
      await wait(()=>bar().dataset.preferredQuality==='lossless','native background expiry after restart');
      await capture('reopened_expired');
    } else {
      click(one('.track-row .track-play:not(:disabled)'));
      await wait(()=>bar().dataset.state==='playing'&&Number(bar().dataset.position)>.2,'actual decoder');
      click(one('.playerbar [aria-label="暂停"]'));await wait(()=>bar().dataset.state==='paused','pause');
      const seek=one('[aria-label="播放进度"]');seek.value='10';seek.dispatchEvent(new Event('change',{bubbles:true}));
      await wait(()=>Math.abs(Number(bar().dataset.position)-10)<.2&&!seek.disabled,'seek');
      await invoke('player_action',{epoch,action:{kind:'quality',level:'lossless'}});
      await wait(()=>bar().dataset.preferredQuality==='lossless','initial preference');
      let info=await details();await capture('before_activation');
      let result=await trial({kind:'start',play_id:info.playId,details_revision:info.revision,level:'jymaster'});
      assert(result.trial.canUse&&result.trial.acknowledged&&result.playback&&!result.playbackError,'positive receipt and actual trial playback');
      operation=result.trial.operation;await wait(()=>bar().dataset.preferredQuality==='jymaster','bound preference');
      await capture('acknowledged_and_playing',{trial:result.trial});
      const before=bar().dataset.playId;
      await fixture('trial-expire');await wait(()=>bar().dataset.preferredQuality==='lossless','native background expiry with menu closed');
      assert(bar().dataset.playId===before&&bar().dataset.requestedQuality==='jymaster','expiry never reloads audio');
      await capture('expired_restored');
      await fixture('trial-eligible');await fixture('trial-lost-response');info=await details();
      result=await trial({kind:'start',play_id:info.playId,details_revision:info.revision,level:'jymaster'});
      assert(result.trial.phase==='uncertain'&&!result.trial.acknowledged&&!result.playback,'lost response is unresolved');
      operation=result.trial.operation;await capture('lost_receipt',{trial:result.trial});
      result=await trial({kind:'reconcile',right:'jymaster',operation});
      assert(result.trial.canUse&&!result.trial.acknowledged&&!result.playback,'query establishes rights without inventing receipt');
      await capture('read_only_reconciled',{trial:result.trial});
      info=await details();result=await trial({kind:'use_active',play_id:info.playId,details_revision:info.revision,level:'jymaster'});
      assert(result.playback&&!result.playbackError&&!result.trial.acknowledged,'use active without another start');
      await wait(()=>bar().dataset.preferredQuality==='jymaster','reconciled preference');
      await capture('active_playback_retry',{trial:result.trial});
    }
    await emit('complete',{ok:true,stageCount:count,reopened:!!window.__trialReopen,operation,
      nativeTrialIpcVerified:true,consentDialogVerified:false,realMusicRequests:0,accountWrites:0});
  } catch(error) {await emit('failed',{ok:false,error:String(error),stageCount:count,operation});}
})();
