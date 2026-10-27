(async()=>{
 if(window.__podcastSocialStarted)return;window.__podcastSocialStarted=true;
 const sleep=ms=>new Promise(r=>setTimeout(r,ms)),el=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)],input=window.__podcastSocial;
 const invoke=(name,args={})=>window.__TAURI_INTERNALS__.invoke(name,args);
 const report=(stage,data={})=>invoke('plugin:event|emit',{event:'podcast-social-proof',payload:{stage,...data}});
 const wait=async(test,label,ms=90000)=>{const end=Date.now()+ms;while(Date.now()<end){const v=await test();if(v)return v;await sleep(150);}throw Error(label);};
 const button=(text,scope=document)=>[...scope.querySelectorAll('button')].find(n=>n.textContent.trim()===text);
 const click=async n=>{if(!n||n.disabled)throw Error('disabled_'+(n?.getAttribute('aria-label')||n?.textContent.trim()||'missing'));n.scrollIntoView({block:'nearest'});n.click();await sleep(140);};
 const stage=async(name,data={})=>{await sleep(300);await report(name,{...data,horizontalOverflow:document.documentElement.scrollWidth>innerWidth});await sleep(1000);};
 const stable=()=>!el('.podcast-page .spinner')&&!el('.podcast-page .error-state')&&!el('.podcast-pending');
 async function library(){if(button('返回播客列表',el('.podcast-page')))await click(button('返回播客列表',el('.podcast-page')));await wait(()=>el('[aria-label="播客编号或链接"]')&&stable(),'library');}
 async function open(id){await library();const n=el('[aria-label="播客编号或链接"]');n.value='https://music.163.com/#/djradio?id='+id;n.dispatchEvent(new Event('input',{bubbles:true}));await wait(()=>!button('打开播客').disabled,'open_ready');await click(button('打开播客'));await wait(()=>el('.podcast-cover')&&stable()&&!el('.podcast-actions .secondary').disabled,'open_detail');}
 const voice=()=>el(`[data-voice-id="${input.voice}"]`),like=()=>voice()?.querySelector('[aria-pressed]');
 const resume=()=>el('.podcast-actions input[type="checkbox"]');
 async function play(){await wait(()=>voice()?.querySelector('button')&&!voice().querySelector('button').disabled,'play_ready');await click(voice().querySelector('button'));await wait(()=>el('.playerbar').dataset.state==='playing'&&Number(el('.playerbar').dataset.position)>.3,'decoded');}
 async function stop(){const n=el('.playerbar [aria-label="停止播放"]');if(n&&!n.disabled){await click(n);await wait(()=>el('.playerbar').dataset.state==='idle','stopped');}}
 async function seek(seconds){await click(el('.playerbar [aria-label="暂停"]'));const n=el('[aria-label="播放进度"]');n.value=String(seconds);n.dispatchEvent(new Event('change',{bubbles:true}));await wait(()=>Math.abs(Number(el('.playerbar').dataset.position)-seconds)<.4,'seek');}
 let step='bootstrap',epoch;
 try{
  await wait(()=>all('.track-row').length,'bootstrap');epoch=(await invoke('account_snapshot')).epoch;await click(button('我的播客'));
  step='subscribe';await open(input.radio);if(!button('收藏播客'))throw Error('preserve_existing_subscription');await click(button('收藏播客'));
  await wait(()=>button('已收藏')&&!button('已收藏').disabled&&stable(),'subscribe_confirmed');await stage('social_subscribed',{subscribed:true});
  await library();await wait(()=>el(`[data-radio-id="${input.radio}"]`),'new_subscription_card');await click(el(`[data-radio-id="${input.radio}"]`));await wait(()=>button('已收藏')&&!button('已收藏').disabled,'subscribed_detail');
  step='unsubscribe';await click(button('已收藏'));await wait(()=>button('收藏播客')&&!button('收藏播客').disabled&&stable(),'unsubscribe_confirmed');await library();await wait(()=>!el(`[data-radio-id="${input.radio}"]`)&&stable(),'subscription_removed');await stage('social_unsubscribed',{subscriptionRestored:true});
  step='like';await open(input.resumeRadio);await wait(()=>like()&&!like().disabled,'voice_like_ready');if(like().getAttribute('aria-pressed')!=='false')throw Error('preserve_existing_like');await click(like());
  if(input.dropAck){
   await wait(()=>el('.podcast-pending .secondary')&&!el('.podcast-pending .secondary').disabled,'uncertain_like_visible');
   const pending=(await invoke('podcast_state',{epoch,refresh:false})).value.operations;
   if(pending.length!==1||pending[0].action!=='like'||pending[0].id!==input.voice||!like().disabled)throw Error('lost_reply_not_fenced');
   await stage('social_like_uncertain',{pending:1,duplicateWriteDisabled:true});await click(el('.podcast-pending .secondary'));
  }
  await wait(()=>like()?.getAttribute('aria-pressed')==='true' &&!like().disabled&&stable(),'voice_like_confirmed');
  await library();await click(button('点赞的单集'));await wait(()=>voice()&&like()?.getAttribute('aria-pressed')==='true'&&!like().disabled&&stable(),'liked_library_updated');await stage('social_liked',{liked:true,listedInLiked:true});
  step='unlike';await click(like());await wait(()=>!voice()&&stable(),'unlike_removed_from_library');await stage('social_unliked',{likeRestored:true});
  step='resume_change';await open(input.resumeRadio);await wait(()=>resume()&&!resume().disabled,'resume_ready');if(resume().checked!==input.resumeBefore)throw Error('resume_baseline_changed');await click(resume());const desired=!input.resumeBefore;
  await wait(()=>resume()?.checked===desired&&!resume().disabled&&stable(),'resume_change_confirmed');await stage(desired?'social_resume_on':'social_resume_off',{enabled:desired});
  step='resume_position';await play();await seek(37);await stop();await play();const position=Number(el('.playerbar').dataset.position);
  if(desired?position<36.5:position>6)throw Error('resume_switch_did_not_control_start');
  await stage('social_resume_position',{enabled:desired,position,localCheckpoint:true});await seek(37);await stop();
  step='resume_restore';await click(resume());await wait(()=>resume()?.checked===input.resumeBefore&&!resume().disabled&&stable(),'resume_restore_confirmed');
  await play();const restoredPosition=Number(el('.playerbar').dataset.position);if(input.resumeBefore?restoredPosition<36.5:restoredPosition>6)throw Error('restored_setting_not_applied');await stop();
  await stage(input.resumeBefore?'social_resume_on':'social_resume_off',{enabled:input.resumeBefore,restored:true,startPosition:restoredPosition});
  const state=(await invoke('podcast_state',{epoch,refresh:false})).value;if(state.operations.length)throw Error('unresolved_mutations');
  await report('complete',{ok:true,subscribeRoundtrip:true,voiceLikeRoundtrip:true,resumeRoundtrip:true,resumeControlsPlayback:true,unresolved:0,remoteWritesExpected:6,lostLikeAcknowledgement:input.dropAck,audioOutput:'null'});
 }catch(error){await stop().catch(()=>{});await report('failed',{ok:false,step,error:error instanceof Error?error.message:'native_failure',pageError:el('.podcast-page .error-state')?.textContent});}
})();
