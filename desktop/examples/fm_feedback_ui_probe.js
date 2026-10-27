(async()=>{
  const sleep=ms=>new Promise(r=>setTimeout(r,ms)),el=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)];
  const button=t=>all('button').find(n=>n.textContent.trim()===t);
  const wait=async(fn,label)=>{const end=Date.now()+45000;while(Date.now()<end){if(fn())return;await sleep(100);}throw Error(label);};
  const click=async n=>{if(!n)throw Error('missing_control');await wait(()=>n.isConnected&&!n.disabled,'disabled_control');n.scrollIntoView({block:'center'});n.click();await sleep(200);};
  const report=(stage,data={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'fm-feedback-proof',payload:{stage,...data}});
  async function stage(name,data={}){el('.main-content').scrollTo(0,0);await sleep(500);const main=el('.main-content');if(document.documentElement.scrollWidth>innerWidth||main.scrollWidth>main.clientWidth+1)throw Error('overflow');window.__feedbackAck='';await report(name,{overflow:false,width:innerWidth,height:innerHeight,...data});await wait(()=>window.__feedbackAck===name,'native_ack');await sleep(1100);}
  let step='boot';
  try {
    await wait(()=>el('.account-button')&&!el('.account-button').disabled&&!el('.welcome'),'account');
    await click(button('私人漫游'));await wait(()=>button('开始漫游'),'FM');await click(button('开始漫游'));await wait(()=>button('暂停漫游')&&Number(el('[aria-label="播放进度"]').value)>.3,'real_audio');
    await click(button('减少推荐'));await wait(()=>el('.fm-dialog[open] .fm-reason[data-artist-id]'),'reasons');await stage('feedback_ready');
    const target=window.__feedbackTarget;
    step='feedback';await click(el(`.fm-dialog [data-reason-key="${target.reasonKey}"]`));await wait(()=>!el('.fm-dialog[open]')&&button('减少推荐')&&!button('减少推荐').disabled,'feedback_returned');
    await stage('feedback_changed');
    step='reconcile';const reconcile=button('核对状态');if(reconcile){await click(reconcile);await wait(()=>!button('核对状态'),'artist_confirmed');}
    await stage('feedback_reconciled');
    step='restore_song';await click(button('减少推荐列表'));await wait(()=>el(`[data-song-id="${target.songId}"] button`),'new_trash_song');
    await click(el(`[data-song-id="${target.songId}"] button`));await wait(()=>!el(`[data-song-id="${target.songId}"]`)&&el(`.fm-artists [data-artist-id="${target.artistId}"] button`)&&!el(`.fm-artists [data-artist-id="${target.artistId}"] button`).disabled,'song_restored');
    await stage('feedback_song_restored');
    step='restore_artist';await wait(()=>el(`.fm-artists [data-artist-id="${target.artistId}"] button`),'blocked_artist');await click(el(`.fm-artists [data-artist-id="${target.artistId}"] button`));
    await wait(()=>!el(`.fm-artists [data-artist-id="${target.artistId}"]`),'artist_restored');await stage('feedback_restored');
    await report('complete',{ok:true,screenshots:5,audioOutput:'null'});
  }catch(e){await report('failed',{ok:false,step,error:e instanceof Error?e.message:'unknown'});}
})();
