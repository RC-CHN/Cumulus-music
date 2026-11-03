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
 const settled=()=>el('.equalizer-dialog')&&!el('[aria-label="均衡器预设"]')?.disabled&&el('.equalizer-status')?.dataset.status!=='pending'&&el('.equalizer-dialog').dataset.draftState==='settled';
 const pop=[-1.5,-1,0,1.79,4,3.79,1.79,.5,-1,1],custom=[...pop];custom[4]=-3.5;
 let epoch='',step='bootstrap';
 async function stage(name,expect){await wait(settled,'settle');await sleep(450);const dialog=el('.equalizer-dialog'),r=dialog.getBoundingClientRect();const overflow=document.documentElement.scrollWidth>innerWidth||dialog.scrollWidth>dialog.clientWidth+1||r.top<0||r.bottom>innerHeight+1;if(overflow)throw Error('equalizer_layout_overflow');window.__eqAck='';await report(name,{epoch,expect,gains:gains(),enabled:enabled(),tone:tone(),status:el('.equalizer-status').dataset.status,width:innerWidth,height:innerHeight,overflow});await wait(()=>window.__eqAck===name,'native_ack');await sleep(1000);}
 async function preset(index){const revision=el('.equalizer-dialog').dataset.revision;const n=el('[aria-label="均衡器预设"]');await wait(()=>!n.disabled,'preset_ready');n.value=String(index);n.dispatchEvent(new Event('change',{bubbles:true}));await wait(()=>settled()&&el('.equalizer-dialog').dataset.revision!==revision,'preset_commit');}
 async function toggle(){const revision=el('.equalizer-dialog').dataset.revision;await click(el('[aria-label="启用均衡器"]'));await wait(()=>settled()&&el('.equalizer-dialog').dataset.revision!==revision,'toggle_commit');}
 async function open(){await click(el('button[aria-label="均衡器"]'));await wait(settled,'dialog_ready');}
 async function close(){await click(el('[aria-label="关闭均衡器"]'));await wait(()=>!el('.equalizer-dialog'),'dialog_closed');}
 try{
  await wait(()=>el('.welcome'),'guest_bootstrap');epoch=(await invoke('account_snapshot')).epoch;
  await click(byText('本地音乐'));await wait(()=>all('.local-tracks tbody tr').length===1,'local_fixture');
  if(window.__eqProof.reopen){
   step='reopened';await wait(()=>el('.playerbar [aria-label="播放"]')&&Number(el('[aria-label="播放进度"]')?.value)>=11,'restored_pause');
   await open();await stage('eq_reopened',{enabled:true,gains:pop});await close();
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
    step='slider';const revision=el('.equalizer-dialog').dataset.revision,n=ranges()[4];for(const value of ['-1','-2','-3.5']){n.value=value;n.dispatchEvent(new Event('input',{bubbles:true}));}await wait(()=>settled()&&el('.equalizer-dialog').dataset.revision!==revision&&el('.equalizer-status').dataset.status==='processing','input_only_slider_processed');await stage('eq_custom',{enabled:true,gains:custom});
    step='save_custom';const beforeSave=el('.equalizer-dialog').dataset.revision;await click(byText('保存自定义预设'));
    await wait(()=>{
      const error=el('.equalizer-error');if(error)throw Error(error.textContent.trim());
      return settled()&&el('[aria-label="均衡器预设"] option[value="-2"]')&&byText('保存自定义预设').disabled;
    },'custom slot acknowledged');
    if(el('.equalizer-dialog').dataset.revision!==beforeSave)throw Error('custom_save_changed_active_revision');
    await stage('eq_custom_saved',{enabled:true,gains:custom});
    step='builtin_after_custom';await preset(1);await stage('eq_custom_built_in',{enabled:true,gains:pop});
    step='recall_custom';await preset(-2);await wait(()=>el('.equalizer-status').dataset.status==='processing','recalled custom reaches DSP');
    await stage('eq_custom_recalled',{enabled:true,gains:custom});
    step='off';await toggle();await stage('eq_off',{enabled:false,gains:custom});
    step='reenable';await toggle();await wait(()=>el('.equalizer-status').dataset.status==='processing','reenabled_dsp');await stage('eq_reenabled',{enabled:true,gains:custom});
    step='reset';const beforeReset=el('.equalizer-dialog').dataset.revision;await click(byText('恢复平直'));await wait(()=>settled()&&el('.equalizer-dialog').dataset.revision!==beforeReset&&gains().every(n=>n===0),'flat_reset');await stage('eq_flat',{enabled:true,gains:Array(10).fill(0)});await preset(1);
    step='adjust_tone';const beforeTone=el('.equalizer-dialog').dataset.revision;
    toneInput('低音增益',6);toneInput('高音增益',-3);
    await wait(()=>settled()&&el('.equalizer-dialog').dataset.revision!==beforeTone&&el('.equalizer-status').dataset.status==='processing','both tone gains reach native processing');
    await stage('eq_tone',{enabled:true,gains:pop});
    step='seek';await close();const seek=el('[aria-label="播放进度"]');seek.value='12';seek.dispatchEvent(new Event('change',{bubbles:true}));await wait(()=>Number(seek.value)>=11&&!el('.playerbar [aria-label="暂停"]')?.disabled,'seek');await click(el('.playerbar [aria-label="暂停"]'));await wait(()=>el('.playerbar [aria-label="播放"]'),'pause');await open();await stage('eq_saved',{enabled:true,gains:pop});
   }
  }
  await report('complete',{ok:true,epoch,reopened:window.__eqProof.reopen,missing:window.__eqProof.missing,networkRequests:0,remoteMutations:0,audioOutput:'null'});
 }catch(e){await report('failed',{ok:false,step,error:e instanceof Error?e.message:'equalizer_ui_failure'});}
})();
