(async()=>{
 if(window.__podcastStarted)return;window.__podcastStarted=true;
 const sleep=ms=>new Promise(r=>setTimeout(r,ms)),el=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)];
 const invoke=(name,args={})=>window.__TAURI_INTERNALS__.invoke(name,args), input=window.__podcastProof;
 const report=(stage,data={})=>invoke('plugin:event|emit',{event:'podcast-proof',payload:{stage,...data}});
 const wait=async(test,label,ms=90000)=>{const end=Date.now()+ms;while(Date.now()<end){const v=await test();if(v)return v;await sleep(150);}throw Error(label);};
 const button=(text,scope=document)=>[...scope.querySelectorAll('button')].find(n=>n.textContent.trim()===text);
 const click=async n=>{if(!n||n.disabled)throw Error('control_unavailable_'+(n?.getAttribute('aria-label')||n?.textContent?.trim()||'missing'));n.scrollIntoView({block:'nearest'});n.click();await sleep(120);};
 const change=(label,value)=>{const n=el(`[aria-label="${label}"]`);if(!n||n.disabled)throw Error('input_unavailable');n.value=value;n.dispatchEvent(new Event('input',{bubbles:true}));n.dispatchEvent(new Event('change',{bubbles:true}));};
 const stage=async(name,data={})=>{await sleep(400);await report(name,{...data,horizontalOverflow:document.documentElement.scrollWidth>innerWidth,layout:{documentHeight:document.documentElement.scrollHeight,bodyHeight:document.body.scrollHeight,mainBottom:el('.main-content').getBoundingClientRect().bottom,pageBottom:el('.podcast-page')?.getBoundingClientRect().bottom,playerTop:el('.playerbar').getBoundingClientRect().top,playerBottom:el('.playerbar').getBoundingClientRect().bottom}});await sleep(900);};
 let step='bootstrap',epoch,taskId;
 const downloads=tab=>invoke('downloads',{epoch,query:{tab,keyword:'',sort:'completed',descending:true,offset:0,limit:100}});
 try{
  await wait(()=>all('.track-row').length,'bootstrap');epoch=(await invoke('account_snapshot')).epoch;
  await click(button('我的播客'));await wait(()=>el(`[data-radio-id="${input.radio}"]`),'podcast_library');
  if(input.reopen){
   await wait(()=>el('.playerbar').dataset.state==='paused'&&Math.abs(Number(el('.playerbar').dataset.position)-12)<.4,'restored_paused_position');const saved=await invoke('player_queue',{epoch});if(saved.tracks.find(t=>t.entryId===saved.current)?.track.voiceId!==input.voice)throw Error('restored_voice_identity');
   await stage('podcast_reopened',{libraryReloaded:true,pausedPositionRestored:true,width:innerWidth,height:innerHeight});
  }else{
   await stage('podcast_library',{cards:all('.podcast-card').length,tabs:all('.podcast-tabs button').length});
   for(const label of ['有声书','我的创建','我的购买','点赞的单集']){
    await click(button(label,el('.podcast-page')));await wait(()=>!el('.podcast-page .spinner')&&!el('.podcast-page .error-state'),'tab_'+label);
   }
   await stage('podcast_empty',{likedEpisodes:all('.episode-row').length});
   await click(button('我的收藏',el('.podcast-page')));await wait(()=>el(`[data-radio-id="${input.radio}"]`),'return_library');
  }
  step='detail';await click(el(`[data-radio-id="${input.radio}"]`));await wait(()=>el(`[data-voice-id="${input.voice}"]`),'detail_rows');
  const query={scope:{kind:'radio',id:input.radio,ascending:true},keyword:'',offset:0,limit:50,revision:null};
  const native=await invoke('podcast_page',{epoch,query,refresh:false});
  if(native.total!==all('.episode-row').length||!native.episodes.some(v=>v.id===input.voice))throw Error('program_projection_mismatch');
  el('.episode-row:last-child').scrollIntoView({block:'end'});await sleep(100);const box=el('.podcast-page').getBoundingClientRect(),last=el('.episode-row:last-child').getBoundingClientRect();if(last.bottom>box.bottom+2||last.top<box.top||last.bottom>el('.playerbar').getBoundingClientRect().top+2)throw Error('last_episode_clipped');el('.podcast-page').scrollTop=0;if(document.documentElement.scrollHeight>innerHeight+2)throw Error('unexpected_window_vertical_overflow');
  await stage('podcast_detail',{lastEpisodeReachable:true,episodes:native.total,resumeSettingKnown:native.resumeEnabled!==null,subscriptionKnown:native.radio.subscribed!==null});
  step='play';await click(el(`[data-voice-id="${input.voice}"] button`));
  await wait(()=>el('.playerbar').dataset.state==='playing'&&Number(el('.playerbar').dataset.position)>.3,'native_program_progress');
  const queue=await invoke('player_queue',{epoch}),track=queue.tracks.find(t=>t.entryId===queue.current)?.track;
  if(queue.tracks.length!==native.total||track.voiceId!==input.voice||track.id===input.voice)throw Error('program_queue_identity');
  await click(el('.playerbar [aria-label="暂停"]'));change('播放进度','12');await wait(()=>Math.abs(Number(el('.playerbar').dataset.position)-12)<.4,'native_seek');
  await stage('podcast_playing',{nativeDecode:true,audioOutput:'null',queueCount:queue.tracks.length,programIdentity:true,paused:true,seekSeconds:12});
  step='comments';await click(el(`[data-voice-id="${input.voice}"] [aria-label="单集评论"]`));await wait(()=>el('.comments-page')&&!el('.comments-page .spinner'),'voice_comments');
  if(el('.comments-page .error-state'))throw Error('voice_comments_failed');await stage('podcast_comments',{programResource:true});
  await click(el('[aria-label="返回上一页"]'));await wait(()=>{const n=el(`[data-voice-id="${input.voice}"] [aria-label="下载单集"]`);return n&&!n.disabled;},'return_detail');
  if(input.reopen){await click(el('.playerbar [aria-label="停止播放"]'));await report('complete',{ok:true,reopened:true,smallWindow:true,programPlayback:true,comments:true,remoteWrites:0});return;}
  step='download';const ordinary=(await downloads('active')).options;
  await click(el(`[data-voice-id="${input.voice}"] [aria-label="下载单集"]`));await wait(()=>el('.download-dialog'),'voice_download_dialog');
  if(el('[aria-label="保存格式"]')||el('.download-dialog').textContent.includes('同时保存 LRC 歌词'))throw Error('unsupported_voice_download_option');
  change('下载目录',input.root+'/media');change('下载音质','standard');await wait(()=>!el('.download-dialog button.primary').disabled,'download_form_ready');await click(el('.download-dialog button.primary'));
  await wait(()=>el('.downloads-page')&&!el('.download-dialog'),'download_created');
  const completed=await wait(async()=>{const v=await downloads('completed');return v.tasks.length===1&&!v.tasks[0].cleanupPending?v:null;},'voice_download_complete',140000);
  const task=completed.tasks[0];taskId=task.id;
  if(task.source!=='voice'||task.track.voiceId!==input.voice||task.encrypted||JSON.stringify(completed.options)!==JSON.stringify(ordinary))throw Error('voice_download_contract');
  await click(all('[role="tab"]').find(n=>n.textContent.includes('已下载')));await wait(()=>el('.download-row button[aria-label^="播放下载"]'),'completed_row');
  await stage('podcast_download',{ordinaryPreferencesPreserved:true,voiceIdentity:true,encrypted:false,bytes:task.bytes,actualQuality:task.actualQuality});
  step='local';await click(el('.download-row button[aria-label^="播放下载"]'));await wait(()=>el('.playerbar').dataset.state==='playing'&&Number(el('.playerbar').dataset.position)>.3,'local_decode');
  const local=await invoke('player_queue',{epoch});if(local.tracks.find(t=>t.entryId===local.current)?.track.voiceId!==input.voice)throw Error('local_program_identity_lost');
  await stage('podcast_local',{localDecode:true,voiceIdentity:true});await click(el('.playerbar [aria-label="停止播放"]'));
  await click(el('.download-row-actions button[aria-label^="管理下载"]'));await click(button('删除本地文件',el('.download-dialog')));await click(button('确认删除本地文件',el('.download-dialog')));
  await wait(async()=>!(await downloads('completed')).completedCount,'cleanup');
  step='persist';await click(button('我的播客'));await wait(()=>{const n=el(`[data-voice-id="${input.voice}"] button`);return n&&!n.disabled;},'return_after_download');await click(el(`[data-voice-id="${input.voice}"] button`));await wait(()=>el('.playerbar').dataset.state==='playing','reselect_before_exit');await click(el('.playerbar [aria-label="暂停"]'));change('播放进度','12');await wait(()=>Math.abs(Number(el('.playerbar').dataset.position)-12)<.4,'saved_pause_before_exit');
  await report('complete',{ok:true,pausedQueueSaved:true,programPlayback:true,fullQueue:true,comments:true,download:true,localPlayback:true,cleanup:true,remoteWrites:0,width:innerWidth,height:innerHeight});
 }catch(e){await report('failed',{ok:false,step,error:e instanceof Error?e.message:'native_failure',pageError:el('.podcast-page .error-state')?.textContent,playerError:el('.player-error')?.textContent,taskCreated:!!taskId});}
})();
