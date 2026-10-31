// Read-only real-account cloud UI proof. Existing resources are never deleted.
window.__runCloudProof=async({wait,stage,sleep})=>{
  const el=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)];
  const invoke=(name,args={})=>window.__TAURI_INTERNALS__.invoke(name,args);
  const epoch=(await invoke('account_snapshot')).epoch;
  const query={tab:'uploaded',keyword:'',sort:'uploaded',descending:true,offset:0,limit:200};
  const snapshot=(q=query,refresh=false)=>invoke('cloud_page',{epoch,query:q,refresh});
  const button=(text,root=document)=>[...root.querySelectorAll('button')].find(b=>b.textContent.trim()===text);
  const click=async b=>{if(!b)throw Error('missing cloud control');await wait(()=>!b.disabled,'cloud_control_ready');b.click();};
  window.__cloudProofFacts=()=>({renderedRows:all('.cloud-table tbody tr').length,error:el('.cloud-error')?.textContent?.trim().slice(0,150),playbackMessage:el('.playback-message span')?.textContent?.trim().slice(0,150),playerState:el('.playerbar')?.dataset.state,position:Number(el('.playerbar')?.dataset.position),dialog:el('dialog')?.open});
  try{
    const nav=all('.sidebar .nav-item').find(b=>b.textContent.trim()==='音乐云盘');await click(nav);
    await wait(()=>all('.cloud-table tbody tr').length>0,'cloud_catalog_render');
    const initial=await snapshot();
    if(!initial.total||initial.maxSize===null||initial.size===null)throw Error('cloud_capacity_missing');
    const baseline=[];
    for(let offset=0;offset<initial.total;offset+=200){const page=await snapshot({...query,offset});if(page.revision!==initial.revision)throw Error('cloud_catalog_changed');baseline.push(...page.tracks.map(t=>t.track.id));}
    if(document.documentElement.scrollWidth>innerWidth)throw Error('cloud_horizontal_overflow');
    await stage('cloud_uploaded',{count:initial.total,capacityPresent:true,renderedRows:all('.cloud-table tbody tr').length,horizontalOverflow:false});
    if(window.__cloudProofFixture?.resumeTaskId){
      if(JSON.stringify(baseline)!==JSON.stringify(window.__cloudProofFixture.baseline))throw Error('prior_fixture_baseline_changed');
      return {cloudCatalog:true,accountResourcesPreserved:true,...await window.__runCloudMutationProof({wait,stage,sleep,baseline,epoch,query,snapshot})};
    }
    const search=el('.cloud-search input');search.value=initial.tracks[0].track.name;search.dispatchEvent(new Event('input',{bubbles:true}));el('.cloud-search').dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));
    await sleep(600);await wait(()=>all('.cloud-table tbody tr').length>0,'cloud_filter_render');
    const filtered=await snapshot({...query,keyword:search.value});
    if(all('.cloud-table tbody tr').length!==filtered.tracks.length||!filtered.total)throw Error('cloud_filter_mismatch');
    await stage('cloud_search',{filteredCount:filtered.total,searchMatched:true});
    search.value='';search.dispatchEvent(new Event('input',{bubbles:true}));el('.cloud-search').dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));
    await wait(()=>all('.cloud-table tbody tr').length===Math.min(200,initial.total),'cloud_filter_reset');
    // Exercise the official title five-state and other-column three-state
    // cycles through actual DOM clicks; compare complete visible order to Rust.
    let sortCases=0;
    const checkSort=async(column,sort,descending)=>{
      await click(el(`[data-cloud-sort="${column}"]`));
      const page=await snapshot({...query,sort,descending});
      const expected=JSON.stringify(page.tracks.map(t=>t.track.id));
      await wait(()=>JSON.stringify(all('.cloud-table tbody tr').map(r=>r.dataset.cloudId))===expected,'cloud_sort_order_'+column+'_'+sort);
      const header=el(`[data-cloud-sort="${column}"]`).closest('th');
      if(header.getAttribute('aria-sort')!==(sort==='natural'?'none':descending?'descending':'ascending'))throw Error('cloud_sort_accessible_state');
      sortCases++;
    };
    for(const [sort,descending] of [['title',false],['title',true],['artist',false],['artist',true],['natural',false]]){
      await checkSort('title',sort,descending);
      if(sort==='artist'&&descending)await stage('cloud_sort',{state:'artist_reverse',visibleRows:all('.cloud-table tbody tr').length,horizontalOverflow:document.documentElement.scrollWidth>innerWidth});
    }
    for(const column of ['album','format','uploaded','size'])for(const [sort,descending] of [[column,false],[column,true],['natural',false]])await checkSort(column,sort,descending);
    await click(button('标题',el('.cloud-table thead')));await sleep(600);
    const sorted=await snapshot({...query,sort:'title',descending:false});
    if(el('.cloud-track-title strong')?.textContent!==sorted.tracks[0].track.name)throw Error('cloud_sort_mismatch');
    let index=sorted.tracks.findIndex(t=>['aac','m4a','mp3','flac'].includes(t.format)&&t.fileSize>1000000&&t.fileSize<10000000);
    if(index<0)index=0;
    el('.playerbar .volume-trigger').focus();await wait(()=>el('.playerbar [aria-label="音量"]'),'volume_slider_open');
    const volume=el('.playerbar [aria-label="音量"]');volume.value='0';volume.dispatchEvent(new Event('change',{bubbles:true}));
    await wait(()=>Number(el('.playerbar').dataset.volume)===0,'muted_before_playback');
    await click(all('.cloud-row-play')[index]);
    await wait(()=>el('.playerbar')?.dataset.state==='playing'&&Number(el('.playerbar').dataset.position)>.3,'cloud_native_playback');
    await click(el('.playerbar [aria-label="暂停"]'));await wait(()=>el('.playerbar').dataset.state==='paused','cloud_native_pause');
    const position=el('.playerbar [aria-label="播放进度"]');const target=Math.min(20,Number(position.max)/2);if(target<1)throw Error('cloud_duration_missing');position.value=String(target);position.dispatchEvent(new Event('change',{bubbles:true}));
    await wait(()=>Math.abs(Number(el('.playerbar').dataset.position)-target)<.3,'cloud_native_seek');
    await stage('cloud_playing',{actualDecode:true,audioOutput:'null',paused:true,seekSeconds:Number(el('.playerbar').dataset.position),quality:el('.playerbar').dataset.quality});
    await click(el('.playerbar [aria-label="播放队列"]'));await wait(()=>all('.queue-row').length>0,'cloud_queue_render');
    const queue=await invoke('player_queue',{epoch});if(queue.tracks.length!==initial.total)throw Error('cloud_play_all_truncated');
    await stage('cloud_queue',{queueCount:queue.tracks.length,renderedRows:all('.queue-row').length});await click(el('[aria-label="关闭播放队列"]'));
    await click(el('.playerbar [aria-label="打开播放详情"]'));await wait(()=>!!el('.lyrics-panel'),'cloud_lyrics_open');
    await wait(()=>all('.lyric-line').length>0||/暂无歌词|纯音乐/.test(el('.lyrics-column .empty-state')?.textContent??''),'cloud_lyrics_result');
    await stage('cloud_lyrics',{lineCount:all('.lyric-line').length,emptyResult:all('.lyric-line').length===0});await click(el('[aria-label="收起播放详情"]'));
    await click(el('.playerbar [aria-label="停止播放"]'));
    await click(button('上传音乐',el('.cloud-toolbar')));await wait(()=>el('.cloud-upload-dialog')?.open,'cloud_upload_dialog_open');
    await stage('cloud_upload_dialog',{dialogOpen:true});await click(button('选择音频文件',el('.cloud-upload-dialog')));
    await stage('cloud_picker',{nativePickerRequested:true});
    await wait(()=>!button('选择音频文件',el('.cloud-upload-dialog')).disabled,'cloud_picker_cancelled');
    if(all('.cloud-file-list article').length)throw Error('cancelled_picker_added_files');await click(el('[aria-label="关闭上传窗口"]'));
    all('.cloud-table tbody input[type="checkbox"]')[0].click();await click(button('删除所选',el('.cloud-toolbar')));await wait(()=>!!el('#cloud-delete-title'),'cloud_delete_confirmation');
    await stage('cloud_delete_cancel',{confirmationOpen:true,submitted:false});await click(button('保留歌曲',el('dialog')));
    const tab=all('.cloud-page [role="tab"]').find(b=>b.textContent.includes('上传任务'));await click(tab);await wait(()=>!!el('.cloud-page .empty-state'),'cloud_empty_uploads');await stage('cloud_tasks',{pendingTasks:0});
    const final=await snapshot(query,true);const after=[];
    for(let offset=0;offset<final.total;offset+=200){const page=await snapshot({...query,offset});after.push(...page.tracks.map(t=>t.track.id));}
    if(JSON.stringify(after)!==JSON.stringify(baseline))throw Error('cloud_account_resources_changed');
    if(final.deletions.length)throw Error('cancelled_confirmation_sent_delete');
    const mutation=window.__runCloudMutationProof?await window.__runCloudMutationProof({wait,stage,sleep,baseline,epoch,query,snapshot}):{};
    return {cloudCatalog:true,filterAndSort:true,sortCases,cloudPlayback:true,queueCount:queue.tracks.length,cloudLyrics:true,uploadDialog:true,nativePickerCancelled:true,deleteConfirmationCancelled:true,accountResourcesPreserved:true,remoteAccountMutations:0,...mutation};
  }finally{await invoke('player_action',{epoch,action:{kind:'stop'}}).catch(()=>{});}
};
