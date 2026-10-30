// Real UI actions; account mutations remain zero. Queue/settings are local to
// the explicitly isolated CUMULUS_DATA_DIR. Volume is confirmed zero first.
window.__runPlayerProof=async({wait,stage,sleep})=>{
  const el=selector=>document.querySelector(selector);
  const control=label=>el(`.playerbar [aria-label="${label}"]`);
  window.__playerProofFacts=()=>({state:el('.playerbar')?.dataset.state,actualQuality:el('.playerbar')?.dataset.quality,requestedQuality:control('音质')?.value,position:Number(el('.playerbar')?.dataset.position),message:el('.playback-message')?.textContent?.trim().slice(0,180),lyricsOpen:!!el('.lyrics-panel'),lyricsEmpty:el('.lyrics-column .empty-state')?.textContent?.trim().slice(0,100),queueCount:Number(el('.queue-toggle small')?.textContent),queueRemoveDisabled:el('.queue-row [aria-label="移出队列"]')?.disabled,queueError:el('.queue-panel .error-state')?.textContent?.trim().slice(0,100)});
  const value=(label,v)=>{const node=control(label);node.value=String(v);node.dispatchEvent(new Event('change',{bubbles:true}));};
  await wait(()=>control('音量')&&!control('音量').disabled,'player_ready');
  value('音量',0);await wait(()=>el('.playerbar').dataset.volume==='0','muted_native');
  document.querySelectorAll('.nav-main button')[2].click();
  await wait(()=>el('.playlist-hero')&&el('.track-play:not(:disabled)'),'favorite_before_play');
  el('.track-play:not(:disabled)').click();
  await wait(()=>el('.playerbar').dataset.state==='playing'&&Number(el('.playerbar').dataset.position)>.3,'native_play_progress');
  const count=Number(el('.queue-toggle small').textContent);
  if(count<200)throw new Error('playlist_queue_truncated');
  await stage('playing',{nativeProgress:true,completeQueueCount:count,muted:el('.playerbar').dataset.volume==='0'});
  control('暂停').click();await wait(()=>el('.playerbar').dataset.state==='paused','native_pause');
  const position=Number(el('.playerbar').dataset.position);await sleep(500);
  if(Math.abs(Number(el('.playerbar').dataset.position)-position)>.15)throw new Error('paused_clock_moved');
  value('播放进度',20);await wait(()=>Math.abs(Number(el('.playerbar').dataset.position)-20)<.15,'native_seek');
  value('播放速度',1.25);await wait(()=>el('.playerbar').dataset.speed==='1.25'&&!control('音质').disabled,'native_speed');
  value('音质','lossless');await wait(()=>el('.playerbar').dataset.requestedQuality==='lossless'&&!control('音质').disabled&&el('.playerbar').dataset.state==='paused','quality_preserves_pause');
  const qualityResult={requested:el('.playerbar').dataset.requestedQuality,actual:el('.playerbar').dataset.quality};
  // Wait for the actual queue mutation and enabled controls. Fixed sleeps can
  // click disabled buttons while a large queue is still being persisted.
  control('播放队列').click();await wait(()=>document.querySelectorAll('.queue-row').length>0,'queue_render');
  if(document.querySelectorAll('.queue-row').length>=40)throw new Error('queue_not_virtualized');
  const first=el('.queue-row').dataset.entryId;
  await wait(()=>!el('.queue-row [aria-label="下移歌曲"]').disabled,'queue_reorder_ready');
  el('.queue-row [aria-label="下移歌曲"]').click();
  await wait(()=>el('.queue-row').dataset.entryId!==first&&!el('.queue-row [aria-label="移出队列"]').disabled,'queue_reorder_committed');
  const scroll=el('.queue-scroll');scroll.scrollTop=62*150;scroll.dispatchEvent(new Event('scroll'));
  await wait(()=>el('.queue-row').dataset.entryId!==first&&scroll.scrollTop>62*100&&!el('.queue-row [aria-label="移出队列"]').disabled,'queue_remove_ready');
  const removed=el('.queue-row').dataset.entryId;
  el('.queue-row [aria-label="移出队列"]').click();
  await wait(()=>Number(el('.queue-toggle small').textContent)===count-1&&!el(`[data-entry-id="${removed}"]`),'queue_remove');
  await stage('queue',{queueCount:count-1,visibleRows:document.querySelectorAll('.queue-row').length,localReorder:true,localRemove:true});
  el('[aria-label="关闭播放队列"]').click();
  control('下一首').click();await wait(()=>el('.playerbar').dataset.state==='playing'&&control('暂停')&&!control('暂停').disabled,'next_track');
  control('暂停').click();await wait(()=>el('.playerbar').dataset.state==='paused','pause_next');
  if(document.documentElement.scrollWidth>innerWidth)throw new Error('player_horizontal_overflow');
  await stage('controls',{seek:true,speed:true,quality:true,actualQuality:el('.playerbar').dataset.quality,next:true,pause:true,horizontalOverflow:false});
  // First cover the favorite's genuine empty/pure-lyric branch.
  control('歌词').click();
  await wait(()=>document.querySelectorAll('.lyric-line').length>0||/^(暂无歌词|纯音乐，请欣赏)$/.test(el('.lyrics-column .empty-state')?.textContent?.trim()??''),'favorite_lyrics');
  const noLyricsObserved=document.querySelectorAll('.lyric-line').length<=1;
  el('[aria-label="收起播放详情"]').click();
  // A separate normal vocal search supplies a real timed lyric sample, rather
  // than counting the server's one-line instrumental placeholder as a pass.
  const input=el('.searchbox input');input.value='十年 陈奕迅';input.dispatchEvent(new Event('input',{bubbles:true}));
  el('.searchbox').dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));
  await wait(()=>el('.search-heading-copy h1')?.textContent==='十年 陈奕迅'&&el('.track-play:not(:disabled)'),'lyric_sample_search');
  el('.track-play:not(:disabled)').click();
  await wait(()=>el('.playerbar').dataset.state==='playing'&&control('暂停')&&!control('暂停').disabled,'lyric_sample_play');
  control('暂停').click();await wait(()=>el('.playerbar').dataset.state==='paused','lyric_sample_pause');
  control('歌词').click();
  await wait(()=>document.querySelectorAll('.lyric-line:not(:disabled)').length>4,'timed_lyrics_render');
  const seekable=[...document.querySelectorAll('.lyric-line')].find(node=>!node.disabled&&Number(node.dataset.line)>2);
  const before=Number(el('.playerbar').dataset.position);seekable.click();
  await wait(()=>Math.abs(Number(el('.playerbar').dataset.position)-before)>1&&!control('播放').disabled,'lyric_click_seek');
  const translation=[...document.querySelectorAll('.lyric-tools button')].find(n=>n.textContent.trim()==='译');
  const original=translation.getAttribute('aria-pressed');
  translation.click();await wait(()=>translation.getAttribute('aria-pressed')!==original,'translation_preference');
  translation.click();await wait(()=>translation.getAttribute('aria-pressed')===original,'translation_restore');
  const earlier=el('[aria-label="歌词提前半秒"]'),later=el('[aria-label="歌词延后半秒"]');
  const offset=el('.lyric-tools small').textContent;
  if(earlier.disabled)throw new Error('timed_lyric_offset_disabled');
  earlier.click();await wait(()=>el('.lyric-tools small').textContent!==offset,'lyric_offset');
  later.click();await wait(()=>el('.lyric-tools small').textContent===offset,'offset_restore');
  const words=[...document.querySelectorAll('.timed-word')];
  const progress=()=>words.map(w=>w.style.getPropertyValue('--word-progress')).join(',');
  const priorProgress=progress();
  if(!words.length)throw new Error('no_word_timing_sample');
  control('播放').click();
  await wait(()=>el('.playerbar').dataset.state==='playing'&&progress()!==priorProgress,'native_word_progress');
  control('暂停').click();await wait(()=>el('.playerbar').dataset.state==='paused','word_progress_pause');
  await stage('lyrics',{lines:document.querySelectorAll('.lyric-line').length,timedWords:words.length,nativeWordProgress:true,paused:el('.playerbar').dataset.state==='paused',qualityResult,noLyricsObserved,clickSeek:true,preferencesRestored:true});
  el('[aria-label="收起播放详情"]').click();
  control('停止播放').click();await wait(()=>el('.playerbar').dataset.state==='idle','stop');
};
