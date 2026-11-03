(async()=>{
 const sleep=ms=>new Promise(r=>setTimeout(r,ms)),el=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)];
 const invoke=(name,args={})=>window.__TAURI_INTERNALS__.invoke(name,args);
 const report=(stage,data={})=>invoke('plugin:event|emit',{event:'equalizer-proof',payload:{stage,...data}});
 const wait=async(fn,label)=>{for(let i=0;i<300;i++){if(await fn())return;await sleep(100);}throw Error(label);};
 const click=async node=>{if(!node)throw Error('missing_control');await wait(()=>node.isConnected&&!node.disabled,'disabled_control');node.click();await sleep(100);};
 const byText=text=>all('button').find(n=>n.textContent.trim()===text);
 const ranges=()=>all('.equalizer-band input');
 const gains=()=>ranges().map(n=>Number(n.value));
 const tone=()=>['低音增益','高音增益'].map(label=>Number(el('[aria-label="'+label+'"]')?.value));
 function toneInput(label,value){
  const input=el('[aria-label="'+label+'"]');input.value=String(value);input.dispatchEvent(new Event('input',{bubbles:true}));
 }
 const enabled=()=>el('[aria-label="启用均衡器"]')?.getAttribute('aria-checked')==='true';
 const master=()=>el('[aria-label="启用均衡器和低高音增强"]');
 async function toggleMaster(on){
   const revision=el('.equalizer-dialog').dataset.revision;
   await click(master());
   await wait(()=>settled()&&el('.equalizer-dialog').dataset.revision!==revision&&
     master().getAttribute('aria-checked')===String(on),'master_commit');
 }
 const settled=()=>el('.equalizer-dialog')&&!el('[aria-label="均衡器预设"]')?.disabled&&el('.equalizer-status')?.dataset.status!=='pending'&&el('.equalizer-dialog').dataset.draftState==='settled';
 const pop=[-1.5,-1,0,1.79,4,3.79,1.79,.5,-1,1],custom=[...pop];custom[4]=-3.5;
 let epoch='',step='bootstrap';
 async function stage(name,expect){await wait(settled,'settle');await sleep(450);checkLauncher(({eq_initial:'off',eq_enabled:'processing',eq_off:'off',eq_independent_tone:'processing',eq_flat:'flat',eq_master_bypassed:'off',eq_master_saved:'off',eq_reopened:'off',eq_master_edited:'processing',eq_missing:'problem'})[name]);const dialog=el('.equalizer-dialog'),r=dialog.getBoundingClientRect();const overflow=document.documentElement.scrollWidth>innerWidth||dialog.scrollWidth>dialog.clientWidth+1||r.top<0||r.bottom>innerHeight+1;if(overflow)throw Error('equalizer_layout_overflow');window.__eqAck='';await report(name,{epoch,expect,gains:gains(),enabled:enabled(),tone:tone(),status:el('.equalizer-status').dataset.status,width:innerWidth,height:innerHeight,overflow});await wait(()=>window.__eqAck===name,'native_ack');await sleep(1000);}
 async function preset(index){const revision=el('.equalizer-dialog').dataset.revision;const n=el('[aria-label="均衡器预设"]');await wait(()=>!n.disabled,'preset_ready');n.value=String(index);n.dispatchEvent(new Event('change',{bubbles:true}));await wait(()=>settled()&&el('.equalizer-dialog').dataset.revision!==revision,'preset_commit');}
 async function toggle(){const revision=el('.equalizer-dialog').dataset.revision;await click(el('[aria-label="启用均衡器"]'));await wait(()=>settled()&&el('.equalizer-dialog').dataset.revision!==revision,'toggle_commit');}
 const launcher=()=>el('button[aria-label="均衡器"]');
 function checkLauncher(state){
   const button=launcher(),description=button.getAttribute('aria-describedby');
   if(button.getAttribute('aria-haspopup')!=='dialog'||button.hasAttribute('aria-pressed')||
      !document.getElementById(description)?.textContent.trim())throw Error('equalizer_launcher_semantics');
   if(state&&(button.dataset.equalizerState!==state||
      (button.querySelector('.equalizer-marker')?.textContent||'')!==({processing:'•',problem:'!'}[state]||'')))
     throw Error('equalizer_launcher_state_'+state);
 }
 async function open(){checkLauncher();await click(launcher());await wait(settled,'dialog_ready');}
 async function close(){await click(el('[aria-label="关闭均衡器"]'));await wait(()=>!el('.equalizer-dialog'),'dialog_closed');}
 try{
  await wait(()=>el('.welcome'),'guest_bootstrap');epoch=(await invoke('account_snapshot')).epoch;
  await click(byText('本地音乐'));await wait(()=>all('.local-tracks tbody tr').length===1,'local_fixture');
  if(window.__eqProof.reopen){
   step='reopened';await wait(()=>el('.playerbar [aria-label="播放"]')&&Number(el('[aria-label="播放进度"]')?.value)>=11,'restored_pause');
   await open();await stage('eq_reopened',{enabled:true,gains:pop});await toggleMaster(true);await close();
   await click(el('.playerbar [aria-label="播放"]'));await wait(()=>Number(el('[aria-label="播放进度"]')?.value)>12.2,'restored_playing');await open();
   await wait(()=>el('.equalizer-status')?.dataset.status==='processing','restored_dsp');await stage('eq_reopened_playing',{enabled:true,gains:pop});
   step='reopened_custom';await wait(()=>el('[aria-label="均衡器预设"] option[value="-2"]'),'saved custom available after reopening');
   await preset(-2);await wait(()=>el('.equalizer-status').dataset.status==='processing','recalled custom after reopening reaches DSP');
   await stage('eq_reopened_custom_recalled',{enabled:true,gains:custom});
   step='tone_reset';const toneRevision=el('.equalizer-dialog').dataset.revision;
   await click(byText('重置低音和高音'));
   await wait(()=>settled()&&el('.equalizer-dialog').dataset.revision!==toneRevision&&tone().every(v=>v===0),'tone reset preserves GEQ');
   await stage('eq_reopened_tone_reset',{enabled:true,gains:custom});
  }else{
   step='initial';await open();await stage('eq_initial',{enabled:false,gains:Array(10).fill(0)});
   step='preset';await preset(1);
   if(window.__eqProof.missing){
    await wait(()=>el('.equalizer-status')?.dataset.status==='failed','missing_error');await stage('eq_missing',{enabled:true,gains:pop});
    await toggle();await close();await click(el('[aria-label^="播放本地歌曲"]'));await wait(()=>el('.playerbar [aria-label="暂停"]')&&Number(el('[aria-label="播放进度"]')?.value)>.2,'ordinary_recovery');await open();await stage('eq_disabled_recovery',{enabled:false,gains:pop});
   }else{
    await close();await click(el('[aria-label^="播放本地歌曲"]'));await wait(()=>el('.playerbar [aria-label="暂停"]')&&Number(el('[aria-label="播放进度"]')?.value)>.2,'actual_playback');await open();await wait(()=>el('.equalizer-status').dataset.status==='processing','actual_dsp');await stage('eq_enabled',{enabled:true,gains:pop});
    step='focused_gain';const focusedGain=ranges()[3];
    focusedGain.focus({preventScroll:true});
    await wait(()=>{
      const label=focusedGain.closest('.equalizer-band')?.querySelector('.equalizer-band-value');
      return document.activeElement===focusedGain&&label?.getBoundingClientRect().width>0&&
        label.textContent.trim()==='1.79 dB'&&focusedGain.getAttribute('aria-valuetext')==='1.79 分贝';
    },'focused equalizer gain is visible and announced');
    focusedGain.blur();
    step='same_curve_identity';const sameRevision=el('.equalizer-dialog').dataset.revision,sameBand=ranges()[4];
    for(const gain of [3,pop[4]]){sameBand.value=String(gain);sameBand.dispatchEvent(new Event('input',{bubbles:true}));}
    await wait(()=>settled()&&el('.equalizer-dialog').dataset.revision!==sameRevision&&el('[aria-label="均衡器预设"]').value==='-1'&&!byText('保存自定义预设').disabled,'manual equal to built-in');
    await click(byText('保存自定义预设'));
    await wait(()=>settled()&&el('[aria-label="均衡器预设"]').value==='-2','saved equal to built-in');
    await stage('eq_same_curve_saved',{enabled:true,gains:pop});
    await preset(1);
    await wait(()=>el('[aria-label="均衡器预设"]').value==='1','explicit equal built-in remains selected');
    await stage('eq_same_curve_builtin',{enabled:true,gains:pop});
    step='slider_pending';
    const revision=el('.equalizer-dialog').dataset.revision,n=ranges()[4];
    const provides=el('#app').__vue_app__._context.provides;
    const core=provides[Reflect.ownKeys(provides).find(key=>typeof key==='symbol'&&key.description==='core-client')];
    const playerAction=core.playerAction;
    const requests=[], acknowledgements=[];
    let release;
    const gate=new Promise(resolve=>{release=resolve;});
    core.playerAction=async (owner,command)=>{
      if(command.kind!=='equalizer')return playerAction(owner,command);
      const index=requests.push({gain:command.settings.gains[4],revision:command.expected_revision})-1;
      const result=await playerAction(owner,command);
      acknowledgements[index]={gain:result.equalizer.settings.gains[4],revision:result.equalizer.revision};
      if(index===0)await gate;
      return result;
    };
    const input=value=>{n.value=value;n.dispatchEvent(new Event('input',{bubbles:true}));};
    try{
      input('-1');
      await wait(()=>acknowledgements[0]?.gain===-1,'first real native gain acknowledged');
      input('-2');input('-3.5');
      await sleep(250);
      if(requests.length!==1||n.value!=='-3.5')throw Error('latest draft must remain visible with only one write in flight');
      await close();
      if(requests.length!==1)throw Error('dialog closure must not flush a competing write');
      release();
      await wait(()=>acknowledgements[1]?.gain===-3.5,'latest closed-dialog gain acknowledged');
      await open();
      await wait(()=>settled()&&el('.equalizer-status').dataset.status==='processing','latest input reaches native DSP');
      if(requests.length!==2||requests[0].revision!==revision||requests[1].revision!==acknowledgements[0].revision||
        requests[0].gain!==-1||requests[1].gain!==-3.5||gains()[4]!==-3.5)
        throw Error('rapid input must preserve the final gain and native revision order');
    }finally{release();core.playerAction=playerAction;}
    await stage('eq_custom',{enabled:true,gains:custom});
    step='save_custom';const beforeSave=el('.equalizer-dialog').dataset.revision;await click(byText('保存自定义预设'));
    await wait(()=>{
      const error=el('.equalizer-error');if(error)throw Error(error.textContent.trim());
      return settled()&&el('[aria-label="均衡器预设"] option[value="-2"]')&&byText('保存自定义预设').disabled;
    },'custom slot acknowledged');
    if(el('.equalizer-dialog').dataset.revision===beforeSave)throw Error('custom_save_identity_not_persisted');
    await stage('eq_custom_saved',{enabled:true,gains:custom});
    step='builtin_after_custom';await preset(1);await stage('eq_custom_built_in',{enabled:true,gains:pop});
    step='recall_custom';await preset(-2);await wait(()=>el('.equalizer-status').dataset.status==='processing','recalled custom reaches DSP');
    await stage('eq_custom_recalled',{enabled:true,gains:custom});
    step='off';await toggle();await stage('eq_off',{enabled:false,gains:custom});
    step='independent_tone';const beforeIndependent=el('.equalizer-dialog').dataset.revision;
    toneInput('低音增益',6);
    await wait(()=>settled()&&el('.equalizer-dialog').dataset.revision!==beforeIndependent&&
      el('.equalizer-status').dataset.status==='processing','bass-only processing');
    await stage('eq_independent_tone',{enabled:false,gains:custom});
    await click(byText('重置低音和高音'));
    await wait(()=>settled()&&tone().every(value=>value===0),'reset independent tone');
    checkLauncher('off');
    step='reenable';await toggle();await wait(()=>el('.equalizer-status').dataset.status==='processing','reenabled_dsp');await stage('eq_reenabled',{enabled:true,gains:custom});
    step='reset';const beforeReset=el('.equalizer-dialog').dataset.revision;await click(byText('恢复平直'));await wait(()=>settled()&&el('.equalizer-dialog').dataset.revision!==beforeReset&&gains().every(n=>n===0),'flat_reset');await stage('eq_flat',{enabled:true,gains:Array(10).fill(0)});await preset(1);
    step='adjust_tone';const beforeTone=el('.equalizer-dialog').dataset.revision;
    toneInput('低音增益',6);toneInput('高音增益',-3);
    await wait(()=>settled()&&el('.equalizer-dialog').dataset.revision!==beforeTone&&el('.equalizer-status').dataset.status==='processing','both tone gains reach native processing');
    await stage('eq_tone',{enabled:true,gains:pop});
    step='master_bypass';await toggleMaster(false);await stage('eq_master_bypassed',{enabled:true,gains:pop});
    step='master_edit';await preset(1);
    await wait(()=>master().getAttribute('aria-checked')==='true'&&el('.equalizer-status').dataset.status==='processing','preset edit reactivates bypassed effect');
    await stage('eq_master_edited',{enabled:true,gains:pop});
    step='seek';await close();const seek=el('[aria-label="播放进度"]');seek.value='12';seek.dispatchEvent(new Event('change',{bubbles:true}));await wait(()=>Number(seek.value)>=11&&!el('.playerbar [aria-label="暂停"]')?.disabled,'seek');await click(el('.playerbar [aria-label="暂停"]'));await wait(()=>el('.playerbar [aria-label="播放"]'),'pause');await open();await stage('eq_saved',{enabled:true,gains:pop});
    step='save_bypass';await toggleMaster(false);await stage('eq_master_saved',{enabled:true,gains:pop});
   }
  }
  await report('complete',{ok:true,epoch,reopened:window.__eqProof.reopen,missing:window.__eqProof.missing,networkRequests:0,remoteMutations:0,audioOutput:'null'});
 }catch(e){
   const dialog=el('.equalizer-dialog');
   await report('failed',{ok:false,step,...(epoch?{epoch}:{}),
     error:e instanceof Error?e.message:'equalizer_ui_failure',
     equalizer:dialog?{revision:dialog.dataset.revision,draftState:dialog.dataset.draftState,
       status:el('.equalizer-status')?.dataset.status,tone:tone(),enabled:enabled(),
       error:el('.equalizer-error')?.textContent.trim()}:null});
 }
})();
