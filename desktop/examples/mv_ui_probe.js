(async()=>{
 if(window.__mvStarted)return;window.__mvStarted=true;
 const sleep=ms=>new Promise(r=>setTimeout(r,ms)),el=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)];
 const invoke=(name,args={})=>window.__TAURI_INTERNALS__.invoke(name,args);
 const report=(stage,detail={})=>invoke('plugin:event|emit',{event:'mv-proof',payload:{stage,...detail}});
 const wait=async(test,label,ms=70000)=>{const deadline=Date.now()+ms;while(Date.now()<deadline){const value=await test();if(value)return value;await sleep(100);}throw Error(label);};
 const button=(text,scope=document)=>[...scope.querySelectorAll('button')].find(n=>n.textContent.trim()===text);
 const click=async node=>{await sleep(40);if(!node||node.disabled)throw Error('control_unavailable_'+(node?.getAttribute('aria-label')||node?.textContent?.trim()||'missing'));node.scrollIntoView({block:'nearest'});node.click();await sleep(80);};
 const input=(label,value,change=false)=>{const node=el(`[aria-label="${label}"]`);if(!node)throw Error('input_missing');node.value=value;node.dispatchEvent(new Event(change?'change':'input',{bubbles:true}));};
 const stage=async(name,data={})=>{await sleep(400);await report(name,data);await sleep(900);};
 let epoch,downloaded=false,cleaned=false,step='start';
 const snapshot=()=>invoke('video_snapshot',{epoch});
 const downloads=tab=>invoke('downloads',{epoch,query:{tab,keyword:'',sort:'completed',descending:true,offset:0,limit:100}});
 async function frame(){const c=el('.video-canvas'),pixels=c.getContext('2d').getImageData(0,0,c.width,c.height).data;let nonblack=0;for(let n=0;n<pixels.length;n+=4)if(pixels[n]+pixels[n+1]+pixels[n+2]>20)nonblack++;if(nonblack<100)throw Error('blank_canvas');const digest=await crypto.subtle.digest('SHA-256',pixels);return {width:c.width,height:c.height,sequence:c.dataset.frameSequence,renderedFrames:Number(c.dataset.renderedFrames),nonblackPixels:nonblack,sha256:[...new Uint8Array(digest)].map(v=>v.toString(16).padStart(2,'0')).join('')};}
 try {
  await wait(()=>all('.track-row').length,'account_daily');epoch=(await invoke('account_snapshot')).epoch;
  if(window.__mvProof.download){
    if((await downloads('active')).activeCount || (await downloads('completed')).completedCount)throw Error('isolated_downloads_required');
    await click(button('下载管理'));await wait(()=>el('.downloads-page'),'downloads_page');await click(button('下载设置'));await wait(()=>el('[aria-label="下载目录"]'),'download_settings');input('下载目录',window.__mvProof.root+'/media');await click(el('.download-dialog button.primary'));await wait(()=>!el('.download-dialog'),'download_directory_saved');
  }
  step='collection';await click(button('收藏的 MV'));await wait(()=>all('.mv-card').length,'mv_collection');
  await stage('mv_collection',{cards:all('.mv-card').length,windowWidth:innerWidth,windowHeight:innerHeight,horizontalOverflow:document.documentElement.scrollWidth>innerWidth});
  input('MV编号或链接',window.__mvProof.id);el('[aria-label="MV编号或链接"]').closest('form').dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));
  step='stream';await wait(()=>Number(el('.video-canvas')?.dataset.renderedFrames)>10,'real_video_canvas');
  await wait(async()=>{const v=await snapshot();return v.state==='playing'&&v.position>0.4;},'native_video_playing');
  const initial=await snapshot();const before=Number(el('.video-canvas').dataset.renderedFrames),started=performance.now();await sleep(2200);const presented=(Number(el('.video-canvas').dataset.renderedFrames)-before)/((performance.now()-started)/1000);
  await stage('mv_playing',{frame:await frame(),requestedResolution:initial.requestedResolution,actualResolution:initial.actualResolution,position:initial.position,presentedFps:presented,audioOutput:'null'});
  if(window.__mvProof.collect){
    step='reversible_collection';
    const collection=()=>invoke('mv_page',{epoch,query:{keyword:'',offset:0,limit:100,revision:null},refresh:true});
    const baseline=window.__mvProof.baseline, id=window.__mvProof.id, before=await collection();
    if(before.cards.length!==before.total||JSON.stringify(before.cards.map(c=>c.id))!==JSON.stringify(baseline)||baseline.includes(id))throw Error('collection_baseline_changed');
    await wait(()=>button('收藏 MV',el('.mv-info'))&&!button('收藏 MV',el('.mv-info')).disabled,'subscribe_button');
    await click(button('收藏 MV',el('.mv-info')));await wait(()=>button('已收藏 · 取消',el('.mv-info'))&&!button('已收藏 · 取消',el('.mv-info')).disabled,'subscription_confirmed');
    const added=await collection();if(added.total!==baseline.length+1||!added.cards.some(c=>c.id===id))throw Error('subscription_readback');
    await stage('mv_subscribed',{newItemOnly:true,baselineCount:baseline.length,afterCount:added.total});
    await click(button('已收藏 · 取消',el('.mv-info')));await wait(()=>button('收藏 MV',el('.mv-info'))&&!button('收藏 MV',el('.mv-info')).disabled,'unsubscription_confirmed');
    const after=await collection();if(JSON.stringify(after.cards.map(c=>c.id))!==JSON.stringify(baseline))throw Error('collection_baseline_not_restored');
    await stage('mv_unsubscribed',{baselineCount:baseline.length,baselinePreserved:true,orderPreserved:true});
    await click(el('[aria-label="关闭视频"]'));await report('complete',{ok:true,collectionRoundtrip:true,baselinePreserved:true,baselineCount:baseline.length,remoteMutations:2,pointsSpent:0});return;
  }
  step='controls';await click(el('[aria-label="暂停MV"]'));await wait(async()=>(await snapshot()).paused,'video_paused');
  input('MV播放进度','20',true);await wait(async()=>Math.abs((await snapshot()).position-20)<0.3,'video_seek');input('MV音量','27',true);input('MV播放速度','1.25',true);
  await wait(()=>el('[aria-label="MV清晰度"]')?.options.length,'quality_options');
  const select=el('[aria-label="MV清晰度"]'),quality=[...select.options].map(o=>Number(o.value)).find(r=>r!==initial.requestedResolution);
  if(!quality)throw Error('alternative_quality_missing');input('MV清晰度',String(quality),true);
  const controls=await wait(async()=>{const v=await snapshot();return v.state==='paused'&&v.requestedResolution===quality&&Math.abs(v.position-20)<.3&&v.speed===1.25&&v.volume===27?v:null;},'quality_pause_preserved');
  await wait(()=>Number(el('.video-canvas')?.dataset.renderedFrames)>0,'changed_quality_canvas');
  await sleep(800);await stage('mv_controls',{requestedResolution:controls.requestedResolution,actualResolution:controls.actualResolution,paused:true,position:controls.position,speed:controls.speed,volume:controls.volume,frame:await frame()});
  step='fullscreen';await click(el('[aria-label="MV全屏"]'));await wait(()=>el('.mv-fullscreen'),'full_screen');await sleep(400);await stage('mv_fullscreen',{frame:await frame(),width:el('.video-canvas').getBoundingClientRect().width,height:el('.video-canvas').getBoundingClientRect().height});window.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));await wait(()=>!el('.mv-fullscreen'),'leave_fullscreen');
  step='mini';await click(button('收藏的 MV'));await wait(()=>el('.mv-mini'),'mini');await click(el('.mv-mini [aria-label="播放MV"]'));await wait(async()=>(await snapshot()).state==='playing','mini_play');await sleep(400);await stage('mv_mini',{frame:await frame(),width:el('.mv-mini').getBoundingClientRect().width,height:el('.mv-mini').getBoundingClientRect().height});
  step='focus';await click(button('每日推荐'));await wait(()=>button('播放全部'),'daily');await click(button('播放全部'));await wait(()=>el('.playerbar')?.dataset.state==='playing','audio_playing');await wait(async()=>(await snapshot()).paused,'video_yielded');
  await click(el('.mv-mini [aria-label="播放MV"]'));await wait(async()=>(await snapshot()).state==='playing','video_regained');await wait(()=>el('.playerbar')?.dataset.state==='paused','audio_yielded');await stage('mv_focus',{musicToVideo:true,videoToMusic:true,audioPaused:true,videoPlaying:true});
  await click(button('返回 MV'));await wait(()=>!el('.mv-mini')&&el('.mv-info'),'return_detail');await click(el('[aria-label="暂停MV"]'));
  if(window.__mvProof.download){
    step='download';await click(button('下载 MV'));await wait(()=>el('.mv-download-options'),'download_options');const option=all('.mv-download-options button').find(b=>!b.disabled&&b.textContent.trim().startsWith('240P'));if(!option)throw Error('free_240p_unavailable');await click(option);downloaded=true;
    await wait(()=>el('.mv-info .notice')?.textContent.includes('已加入下载'),'download_enqueued');await click(button('下载管理',el('.mv-info')));await wait(()=>el('.downloads-page'),'download_page');
    const completed=await wait(async()=>{const v=await downloads('completed');return v.tasks.find(t=>t.source==='mv'&&!t.cleanupPending);},'mv_download_published',150000);
    await click(all('[role="tab"]').find(n=>n.textContent.includes('已下载')));await wait(()=>el('.download-row button[aria-label^="播放下载"]'),'mv_completed_ui');await stage('mv_download',{bytes:completed.bytes,actualResolution:completed.actualResolution,encrypted:completed.encrypted,source:completed.source});
    step='local';await click(el('.download-row button[aria-label^="播放下载"]'));await wait(async()=>{const v=await snapshot();return v.local&&v.state==='playing';},'local_mv_playback');await wait(()=>el('.mv-play-facts')?.textContent.includes('本地文件'),'local_label');await sleep(400);await stage('mv_local',{local:true,frame:await frame()});
    input('MV播放进度','42',true);await wait(async()=>Math.abs((await snapshot()).position-42)<.5,'local_seek');
    await click(button('下载管理',el('.mv-info')));await wait(()=>el('.downloads-page')&&el('.mv-mini'),'return_downloads');
    const menu=async()=>{await wait(()=>el('.download-row-actions button[aria-label^="管理下载"]')&&!el('.download-row-actions button[aria-label^="管理下载"]').disabled,'file_menu_ready');await click(el('.download-row-actions button[aria-label^="管理下载"]'));await wait(()=>el('.download-file-actions'),'mv_file_menu');};
    const fileDone=async(suffix)=>wait(async()=>{const v=await downloads('completed'),task=v.tasks.find(t=>t.id===completed.id);return task?.path?.endsWith(suffix)&&!task.fileOperation&&v.jobs.every(j=>!j.running);},'file_operation_finished');
    step='active_rename';let beforeFile=await snapshot();await menu();await click(button('重命名',el('.download-dialog')));input('新文件名','mv-proof-renamed.mp4');await click(button('确认',el('.download-dialog')));await wait(()=>!el('.download-dialog'),'rename_submitted');await fileDone('/mv-proof-renamed.mp4');
    let afterFile=await wait(async()=>{const v=await snapshot();return v.local&&v.state==='playing'&&v.playId!==beforeFile.playId?v:null;},'rename_playback_restored');
    if(afterFile.position<beforeFile.position-.3||afterFile.position>beforeFile.position+8)throw Error('rename_position_lost');await sleep(500);await stage('mv_renamed',{whilePlaying:true,local:true,positionPreserved:true,speed:afterFile.speed,volume:afterFile.volume,frame:await frame()});
    step='paused_move';await click(el('.mv-mini [aria-label="暂停MV"]'));beforeFile=await wait(async()=>{const v=await snapshot();return v.state==='paused'?v:null;},'pause_before_move');await menu();await click(button('移动到…',el('.download-dialog')));input('移动目标目录',window.__mvProof.root+'/media/relocated');await click(button('确认',el('.download-dialog')));await wait(()=>!el('.download-dialog'),'move_submitted');await fileDone('/media/relocated/mv-proof-renamed.mp4');
    afterFile=await wait(async()=>{const v=await snapshot();return v.local&&v.state==='paused'&&v.playId!==beforeFile.playId?v:null;},'move_pause_restored');if(Math.abs(afterFile.position-beforeFile.position)>.3)throw Error('move_position_lost');await sleep(500);await stage('mv_moved',{paused:true,local:true,positionPreserved:true,frame:await frame()});
    step='active_migration';await click(el('.mv-mini [aria-label="播放MV"]'));beforeFile=await wait(async()=>{const v=await snapshot();return v.state==='playing'?v:null;},'play_before_migration');await click(button('下载设置'));await wait(()=>el('[aria-label="下载目录"]'),'migration_settings');input('下载目录',window.__mvProof.root+'/migrated');await wait(()=>el('input[name="directory-mode"][value="migrate"]'),'migration_option');await click(el('input[name="directory-mode"][value="migrate"]'));await click(el('.download-dialog button.primary'));await wait(()=>!el('.download-dialog'),'migration_submitted');
    await fileDone('/migrated/relocated/mv-proof-renamed.mp4');await wait(async()=>!(await downloads('completed')).migration,'batch_committed');afterFile=await wait(async()=>{const v=await snapshot();return v.local&&v.state==='playing'&&v.playId!==beforeFile.playId?v:null;},'migration_playback_restored');if(afterFile.position<beforeFile.position-.3||afterFile.position>beforeFile.position+10)throw Error('migration_position_lost');await sleep(500);await stage('mv_migrated',{whilePlaying:true,local:true,positionPreserved:true,frame:await frame()});
    step='active_delete';await menu();await click(button('删除本地文件',el('.download-dialog')));await click(button('确认删除本地文件',el('.download-dialog')));await wait(()=>!el('.download-dialog'),'mv_delete_submit');
    await wait(async()=>{const v=await downloads('completed');return v.completedCount===0&&v.jobs.every(j=>!j.running);},'mv_delete_done');cleaned=true;
    await wait(async()=>(await snapshot()).state==='idle','active_delete_stopped');await stage('mv_cleanup',{newMediaDeleted:true,completedDownloads:0,deletedWhilePlaying:true,automaticallyStopped:true});
  } else {await click(el('[aria-label="关闭视频"]'));cleaned=true;}
  await report('complete',{ok:true,realNativeVideo:true,rawFrameIpc:true,controls:true,fullscreen:true,mini:true,mediaFocus:true,downloadAndLocal:downloaded,activeLocalFiles:downloaded,cleanupComplete:cleaned,remoteMutations:0,pointsSpent:0,audioOutput:'null'});
 } catch(error) {
  if(epoch){await invoke('video_action',{epoch,action:{kind:'stop'}}).catch(()=>{});await invoke('player_action',{epoch,action:{kind:'stop'}}).catch(()=>{});}
  await report('failed',{ok:false,step,error:error instanceof Error?error.message:'native_action_failed',videoError:el('.video-message')?.textContent,pageError:el('.form-warning')?.textContent,cleanupComplete:cleaned});
 }
})();
