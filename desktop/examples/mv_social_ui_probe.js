(async()=>{
 if(window.__mvSocialStarted)return;window.__mvSocialStarted=true;
 const sleep=ms=>new Promise(r=>setTimeout(r,ms)),el=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)];
 const invoke=(name,args={})=>window.__TAURI_INTERNALS__.invoke(name,args);
 const report=(stage,data={})=>invoke('plugin:event|emit',{event:'mv-social-proof',payload:{stage,...data}});
 const wait=async(test,label,ms=65000)=>{const end=Date.now()+ms;while(Date.now()<end){const v=await test();if(v)return v;await sleep(100);}throw Error(label);};
 const button=text=>all('button').find(n=>n.textContent.trim()===text);
 const click=async n=>{if(!n||n.disabled)throw Error('control_unavailable');n.scrollIntoView({block:'nearest'});n.click();await sleep(100);};
 const change=(label,value)=>{const n=el(`[aria-label="${label}"]`);if(!n||n.disabled)throw Error('input_unavailable');n.value=value;n.dispatchEvent(new Event('input',{bubbles:true}));n.dispatchEvent(new Event('change',{bubbles:true}));};
 const stage=async(name,data={})=>{await sleep(400);await report(name,{...data,horizontalOverflow:document.documentElement.scrollWidth>innerWidth});await sleep(900);};
 const closeLyrics=async()=>{if(el('[aria-label="收起播放详情"]'))await click(el('[aria-label="收起播放详情"]'));};
 let step='bootstrap',restored=!window.__mvSocialProof.mutate,epoch;
 const id=window.__mvSocialProof.sample.id;
 try{
  await wait(()=>all('.track-row').length,'bootstrap');epoch=(await invoke('account_snapshot')).epoch;
  await click(button('收藏的 MV'));await wait(()=>el('[aria-label="MV编号或链接"]'),'mv_library');change('MV编号或链接',id);el('[aria-label="MV编号或链接"]').closest('form').dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));
  step='detail';await wait(()=>el('.mv-like')&&!el('.mv-like').disabled&&el('[aria-label="播放MV关联歌曲"]')&&all('[data-recommended-mv]').length,'mv_social_detail');
  await wait(()=>Number(el('.video-canvas')?.dataset.renderedFrames)>0,'native_video_frames');
  const native=(await invoke('mv_detail',{epoch,id})).value;const song=native.relatedSongs[0];if(!song||native.relatedPrivilegeFailed)throw Error('related_song_privileges_missing');
  await stage('social_detail',{knownLike:native.liked!==null,relatedSongs:native.relatedSongs.length,recommendations:all('[data-recommended-mv]').length,nativeFrames:true});
  if(window.__mvSocialProof.mutate){
   step='like';if(el('.mv-like').getAttribute('aria-pressed')!=='false')throw Error('preserve_existing_like');await click(el('.mv-like'));await wait(()=>el('.mv-like')?.getAttribute('aria-pressed')==='true'&&!el('.mv-like').disabled,'like_confirmation');await stage('social_liked',{liked:true});
   step='restore';await click(el('.mv-like'));await wait(()=>el('.mv-like')?.getAttribute('aria-pressed')==='false'&&!el('.mv-like').disabled,'unlike_confirmation');
   const value=(await invoke('mv_detail',{epoch,id})).value;if(value.liked!==false||value.likeOperation)throw Error('like_baseline_not_restored');restored=true;await stage('social_restored',{liked:false,pending:false});
  }
  step='related';await click(el('[aria-label="播放MV关联歌曲"]'));await wait(()=>el('.playerbar [aria-label="暂停"]')&&el('.lyrics-panel'),'related_song_playback');
  const before=await invoke('player_queue',{epoch});if(before.tracks.find(t=>t.entryId===before.current)?.track.id!==song.id)throw Error('wrong_related_song');
  if(!(await invoke('video_snapshot',{epoch})).paused)throw Error('video_not_paused');await stage('social_related',{songSelected:true,videoPaused:true,lyricsOpened:true});
  await closeLyrics();change('播放进度','20');await wait(()=>Number(el('[aria-label="播放进度"]')?.value)>=19,'seek_related_song');
  step='resume';await click(el('.mv-detail [aria-label="播放MV"]'));await wait(async()=>!(await invoke('video_snapshot',{epoch})).paused,'mv_resume');await click(el('[aria-label="播放MV关联歌曲"]'));await wait(()=>el('.playerbar [aria-label="暂停"]')&&el('.lyrics-panel'),'related_resume');
  const after=await invoke('player_queue',{epoch});if(before.current!==after.current||before.revision!==after.revision)throw Error('related_resume_replaced_queue');if(Number(el('[aria-label="播放进度"]')?.value)<19)throw Error('related_resume_lost_position');await closeLyrics();
  await stage('social_resumed',{sameQueueEntry:true,positionPreserved:true,videoPaused:(await invoke('video_snapshot',{epoch})).paused});
  step='recommendation';const card=all('[data-recommended-mv]').find(n=>n.dataset.recommendedMv!==id);if(!card)throw Error('no_distinct_recommendation');const next=card.dataset.recommendedMv;await click(card);
  await wait(async()=>{const v=await invoke('video_snapshot',{epoch});return v.id===next&&v.state==='playing';},'recommended_mv_playing');await wait(()=>Number(el('.video-canvas')?.dataset.renderedFrames)>0,'recommended_mv_frames');
  await stage('social_recommendation',{navigated:true,playing:true,nativeFrames:true,audioPaused:!!el('.playerbar [aria-label="播放"]')});
  await click(el('.mv-detail [aria-label="关闭视频"]'));await click(el('.playerbar [aria-label="停止播放"]'));
  await report('complete',{ok:true,likeRoundtrip:window.__mvSocialProof.mutate,baselinePreserved:restored,relatedSongPlayback:true,sameQueueEntry:true,positionPreserved:true,recommendationNavigation:true,mediaFocus:true,remoteMutations:window.__mvSocialProof.mutate?2:0,publications:0,messages:0,payments:0,audioOutput:'null',width:innerWidth,height:innerHeight});
 }catch(e){await report('failed',{ok:false,step,error:e instanceof Error?e.message:'native_operation_failed',baselinePreserved:restored});}
})();
