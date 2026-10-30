// Explicit opt-in real-account download proof. Native media stays inside a new
// proof directory; no account resources are changed. DOM drives the workflow.
window.__runDownloadProof=async({wait,stage,sleep})=>{
  const el=s=>document.querySelector(s), invoke=(name,args={})=>window.__TAURI_INTERNALS__.invoke(name,args);
  const epoch=(await invoke('account_snapshot')).epoch, root=window.__downloadProofRoot;
  const cloud=window.__cloudDownloadProof===true;
  const cloudQuery={tab:'uploaded',keyword:'',sort:'uploaded',descending:true,offset:0,limit:200};
  const cloudSnapshot=(refresh=false)=>invoke('cloud_page',{epoch,query:cloudQuery,refresh});
  const cloudKeys=v=>v.tracks.map(t=>[t.track.id,t.fileName,t.fileSize]);
  let cloudBaseline,expectedBytes,ordinaryOptions;
  if(!root)throw Error('download_proof_root_missing');
  const button=(text,scope=document)=>[...scope.querySelectorAll('button')].find(n=>n.textContent.trim()===text);
  const click=async n=>{if(!n||n.disabled)throw Error('download_control_unavailable');n.scrollIntoView({block:'nearest'});n.click();await sleep(70);};
  const input=(label,value,change=false)=>{const n=el(`[aria-label="${label}"]`);if(!n)throw Error('download_input_missing');n.value=value;n.dispatchEvent(new Event(change?'change':'input',{bubbles:true}));};
  const snapshot=tab=>invoke('downloads',{epoch,query:{tab,keyword:'',sort:'completed',descending:true,offset:0,limit:100}});
  const waitNative=async(test,label)=>{const until=Date.now()+100000;while(Date.now()<until){const value=await snapshot('completed');if(test(value))return value;await sleep(180);}throw Error(label);};
  const closed=()=>!el('.download-dialog');
  const submit=async()=>{await click(el('.download-dialog button.primary'));await wait(closed,'download_dialog_submit');};
  const manage=async()=>{await wait(()=>el('.download-row-actions button[aria-label^="管理下载"]'),'download_file_action');await click(el('.download-row-actions button[aria-label^="管理下载"]'));await wait(()=>el('.download-file-actions'),'download_file_dialog');};
  let taskId,cleaned=false,pauseObserved=false;
  window.__downloadProofFacts=()=>({dialogTitle:el('#download-dialog-title')?.textContent,dialogError:el('.download-dialog .error-state')?.textContent,pageError:el('.downloads-page .error-state')?.textContent,cleanupComplete:cleaned});
  try {
    if((await snapshot('active')).activeCount || (await snapshot('completed')).completedCount)throw Error('download_proof_requires_empty_data');
    await click(button('下载管理'));await wait(()=>el('.downloads-page'),'download_page');
    await click(button('下载设置'));await wait(()=>el('[aria-label="下载目录"]'),'download_settings');
    input('下载目录',root+'/media');input('下载音质','lossless',true);
    for(const label of ['同时保存 LRC 歌词']){const n=[...document.querySelectorAll('.download-dialog label')].find(n=>n.textContent.includes(label))?.querySelector('input');if(n&&!n.checked)await click(n);}
    await stage('download_settings',{nativeOptions:true,plainAudio:true,quality:'lossless',lyrics:true});
    // The external Linux runner captures and cancels the actual native chooser.
    await click(button('选择目录',el('.download-dialog')));await stage('download_picker',{nativeDialogRequested:true});
    await wait(()=>!button('选择目录',el('.download-dialog')).disabled,'download_picker_cancelled');
    await submit();
    if(cloud){
      ordinaryOptions=(await snapshot('active')).options;
      await click(button('音乐云盘'));await wait(()=>el('.cloud-table tbody tr'),'cloud_download_catalog');
      const view=await cloudSnapshot();
      if(view.total!==view.tracks.length)throw Error('cloud_proof_requires_single_page_baseline');
      cloudBaseline=cloudKeys(view);
      const candidate=view.tracks.filter(t=>['aac','mp3','m4a','flac'].includes(t.format)&&t.fileSize>131071&&t.fileSize<8000000).sort((a,b)=>a.fileSize-b.fileSize)[0];
      if(!candidate)throw Error('cloud_download_sample_missing');
      expectedBytes=candidate.fileSize;
      const index=view.tracks.findIndex(t=>t.track.id===candidate.track.id);
      await click([...document.querySelectorAll('.cloud-table tbody input[type="checkbox"]')][index]);
      await click(button('下载所选',el('.cloud-toolbar')));
      await wait(()=>el('.download-dialog'),'cloud_download_dialog');
      if(el('[aria-label="下载音质"]')||!el('#download-dialog-title')?.textContent.includes('云盘原文件'))throw Error('cloud_original_options_missing');
      await stage('cloud_download_options',{cloudOriginal:true,qualitySelector:false,lyricsOption:true,horizontalOverflow:document.documentElement.scrollWidth>innerWidth});
    }else{
      input('搜索音乐','十年 陈奕迅');el('.searchbox').dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));
      await wait(()=>el('.search-heading-copy h1')?.textContent==='十年 陈奕迅'&&el('[data-category="songs"]'),'download_song_search');
      await click(el('[data-category="songs"]'));await wait(()=>el('.search-result-panel')?.getAttribute('aria-busy')==='false'&&el('.track-more'),'download_song_results');
      await click(el('.track-more'));await wait(()=>el('.song-action-menu')?.style.visibility==='visible','download_song_menu');await click(button('下载',el('.song-action-menu')));
      await wait(()=>el('.download-dialog'),'download_start_dialog');
    }
    await submit();
    await wait(()=>el('.downloads-page'),'download_created');
    const active=await snapshot('active'), completed=await snapshot('completed');
    const all=[...active.tasks,...completed.tasks];if(all.length!==1)throw Error('download_isolated_identity');taskId=all[0].id;
    if(active.activeCount){
      await click(button('全部暂停'));
      const until=Date.now()+15000;while(Date.now()<until){const v=await snapshot('active');if(v.tasks.some(t=>t.status==='paused')){pauseObserved=true;break;}if(!v.activeCount)break;await sleep(100);}
      if(pauseObserved){await wait(()=>el('.download-progress')?.textContent.includes('已暂停'),'pause_ui');await stage('download_paused',{pauseObserved:true});await click(button('全部下载'));}
    }
    let v=await waitNative(v=>v.tasks.some(t=>t.id===taskId&&!t.cleanupPending),'download_publication');
    let task=v.tasks.find(t=>t.id===taskId);
    if(task.encrypted||!task.completedMs)throw Error('download_output_contract');
    if(cloud){
      if(task.source!=='cloud'||task.actualQuality!==null||task.bytes!==expectedBytes||!['complete','empty'].includes(task.lyricsStatus))throw Error('cloud_original_output_contract');
      if(JSON.stringify(ordinaryOptions)!==JSON.stringify(v.options))throw Error('cloud_download_changed_ordinary_preferences');
    }else if(task.actualQuality!=='lossless'||task.lyricsStatus!=='complete')throw Error('download_output_contract');
    await click([...document.querySelectorAll('[role="tab"]')].find(n=>n.textContent.includes('已下载')));
    await wait(()=>el('.download-row-actions button[aria-label^="管理下载"]'),'download_completed_row');
    if(document.documentElement.scrollWidth>innerWidth||el('.download-scroll').clientHeight<56)throw Error(`download_layout_clipped_width_${document.documentElement.scrollWidth}_list_${el('.download-scroll').clientHeight}`);
    if(cloud&&el('.download-quality')?.textContent.trim()!=='云盘原文件')throw Error('cloud_original_badge_missing');
    await stage('download_complete',{source:task.source,actualQuality:task.actualQuality,encrypted:false,lyricsStatus:task.lyricsStatus,filePath:task.path,bytes:task.bytes,completedTimePresent:true,pauseObserved,horizontalOverflow:false});
    const volume=el('.playerbar [aria-label="音量"]');volume.value='0';volume.dispatchEvent(new Event('change',{bubbles:true}));
    await wait(()=>el('.playerbar').dataset.volume==='0','download_player_muted');
    await click(el('.download-row button[aria-label^="播放下载"]'));
    await wait(()=>el('.playerbar').dataset.state==='playing'&&Number(el('.playerbar').dataset.position)>.3,'download_native_playback');
    await click(el('.playerbar [aria-label="暂停"]'));await wait(()=>el('.playerbar').dataset.state==='paused','download_native_pause');
    const position=el('.playerbar [aria-label="播放进度"]');position.value='20';position.dispatchEvent(new Event('change',{bubbles:true}));
    await wait(()=>Math.abs(Number(el('.playerbar').dataset.position)-20)<.3,'download_native_seek');
    if(cloud&&el('.playerbar').dataset.quality)throw Error('cloud_original_quality_fabricated');
    await stage('download_playing',{realDecode:true,audioOutput:'null',actualQuality:el('.playerbar').dataset.quality,localQueue:true,paused:true,seekSeconds:Number(el('.playerbar').dataset.position)});
    await click(el('.playerbar [aria-label="停止播放"]'));await wait(()=>el('.playerbar').dataset.state==='idle','download_stop');
    await manage();await click(button('重命名',el('.download-dialog')));input('新文件名','Cumulus下载验证');await submit();
    v=await waitNative(v=>v.tasks.some(t=>t.id===taskId&&t.path.includes('Cumulus下载验证')&&!t.fileOperation),'download_renamed');
    await stage('download_rename',{nameUpdated:true,filePath:v.tasks.find(t=>t.id===taskId).path});
    await click(button('下载设置'));input('下载目录',root+'/migrated');
    await wait(()=>el('input[name="directory-mode"][value="migrate"]'),'download_migration_choice');
    const radio=el('input[name="directory-mode"][value="migrate"]');await click(radio);await submit();
    v=await waitNative(v=>!v.migration&&v.directory.root===root+'/migrated'&&v.tasks.some(t=>t.id===taskId&&!t.fileOperation),'download_migrated');
    task=v.tasks.find(t=>t.id===taskId);
    await stage('download_migrate',{directoryUpdated:true,filePath:task.path,actualQuality:task.actualQuality});
    // Search is an actual native query, followed by row restoration.
    input('搜索下载的歌曲','no-such-download-proof');el('.download-search').dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));
    await wait(()=>el('.downloads-page .empty-state')?.textContent.includes('没有找到'),'download_search_empty');
    await click(el('[aria-label="清空下载搜索"]'));await wait(()=>el('.download-row-actions button[aria-label^="管理下载"]'),'download_search_restore');
    await manage();await click(button('删除本地文件',el('.download-dialog')));await click(button('确认删除本地文件',el('.download-dialog')));await wait(closed,'download_delete_submitted');
    await waitNative(v=>v.completedCount===0&&v.jobs.every(j=>!j.running),'download_delete_complete');
    await wait(()=>el('.downloads-page .empty-state'),'download_deleted_ui');
    const queue=await invoke('player_queue',{epoch});if(queue.tracks.length)throw Error('deleted_download_queue_retained');
    if(cloud){const final=await cloudSnapshot(true);if(JSON.stringify(cloudKeys(final))!==JSON.stringify(cloudBaseline))throw Error('cloud_download_changed_account_resources');}
    cleaned=true;
    await stage('download_cleanup',{downloadCount:0,queueCount:0,cleanupComplete:true});
    return {downloadFlow:true,cloudOriginal:cloud,ordinaryPreferencesPreserved:cloud||undefined,cloudBaselineCount:cloudBaseline?.length,localPlayback:true,rename:true,directoryMigration:true,delete:true,pauseObserved,cleanupComplete:true,remoteAccountMutations:0};
  } finally {
    // New per-run data is mandatory above. Cleanup never enumerates arbitrary
    // media directories and uses the same guarded business commands as the UI.
    if(!cleaned){
      await invoke('player_action',{epoch,action:{kind:'stop'}}).catch(()=>{});
      let active=await snapshot('active');
      for(const t of active.tasks)await invoke('download_command',{epoch,command:{kind:'control',task_ids:[t.id],action:'cancel'}});
      const v=await snapshot('completed');
      for(const t of v.tasks)if(!t.fileOperation&&!t.cleanupPending)await invoke('download_command',{epoch,command:{kind:'file',task_id:t.id,action:'delete',target:null}});
      const until=Date.now()+20000;
      while(Date.now()<until){const v=await snapshot('completed');const a=await snapshot('active');if(!v.completedCount&&!a.activeCount&&v.jobs.every(j=>!j.running)){cleaned=true;break;}await sleep(200);}
    }
  }
};
