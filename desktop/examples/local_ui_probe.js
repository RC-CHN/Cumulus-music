(async()=>{
 const sleep=ms=>new Promise(r=>setTimeout(r,ms)),el=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)];
 const invoke=(name,args={})=>window.__TAURI_INTERNALS__.invoke(name,args);
 const report=(stage,data={})=>invoke('plugin:event|emit',{event:'local-proof',payload:{stage,...data}});
 const wait=async(test,label,ms=60000)=>{const end=Date.now()+ms;while(Date.now()<end){const v=await test();if(v)return v;await sleep(100);}throw Error(label);};
 const button=text=>all('button').find(n=>n.textContent.trim()===text);
 const click=async n=>{if(!n)throw Error('control_missing');await sleep(100);await wait(()=>n.isConnected&&!n.disabled,'control_not_ready',15000);n.scrollIntoView({block:'nearest'});n.click();await sleep(100);};
 const rowGeometry=()=>all('.local-tracks tbody tr').map(row=>{
  const cells=[...row.cells],button=cells[0].querySelector('button').getBoundingClientRect(),cell=cells[0].getBoundingClientRect();
  const textRect=node=>{const r=document.createRange();r.selectNodeContents(node);const box=r.getBoundingClientRect();return {left:box.left,right:box.right};};
  const format=textRect(cells[4]),duration=textRect(cells[5]);
  if(button.left<cell.left-1||button.right>cell.right+1)throw Error('local_play_button_clipped');
  if(format.right+8>duration.left)throw Error('local_format_duration_overlap');
  return {playButtonWidth:button.width,playCellWidth:cell.width,formatToDurationGap:duration.left-format.right};
 });
 const stage=async(name,data={})=>{await sleep(400);const overflow=document.documentElement.scrollWidth>innerWidth||el('.local-page').scrollWidth>el('.local-page').clientWidth+1;if(overflow)throw Error('local_layout_overflow');await report(name,{...data,horizontalOverflow:overflow,rows:rowGeometry()});await sleep(900);};
 const change=(label,value)=>{const n=el(`[aria-label="${label}"]`);if(!n||n.disabled)throw Error('input_unavailable');n.value=value;n.dispatchEvent(new Event('input',{bubbles:true}));n.dispatchEvent(new Event('change',{bubbles:true}));};
 const query={keyword:'',sort:'title',descending:true,group:'list',groupKey:null,includeMissing:true,offset:0,limit:200,revision:null};
 let step='bootstrap';
 try{
  await wait(()=>el('.welcome')&&!el('.main-content .spinner'),'guest_bootstrap');
  const account=await invoke('account_snapshot');if(account.status==='authenticated')throw Error('guest_required');const epoch=account.epoch;
  await click(button('本地音乐'));await wait(()=>el('.local-page'),'local_page');
  if(window.__localProof.reopen){
   step='restore';await wait(()=>el('.playerbar [aria-label="播放"]')&&Number(el('[aria-label="播放进度"]')?.value)>=11,'guest_restore');
   const queue=await invoke('player_queue',{epoch});if(queue.tracks.length!==4||!queue.tracks.every(t=>t.track.local))throw Error('persisted_local_queue');
   await stage('local_reopened',{guest:true,queue:queue.tracks.length,positionPreserved:true,paused:true,rootsRestored:(await invoke('local_page',{query})).roots.length===1});await click(button('歌手'));await wait(()=>el('.local-groups'),'restored_groups');await stage('local_groups',{group:'artist',restored:true});await click(button('歌曲'));
  } else {
   step='picker';await click(button('选择目录'));await stage('local_picker',{nativeDialog:true});
   await wait(()=>all('.local-tracks tbody tr').length===4,'scan_finished');
   const page=await invoke('local_page',{query});if(page.total!==4||page.tracks.filter(t=>t.cue).length!==2||!page.tracks.some(t=>t.encrypted))throw Error('native_index_facts');
   await stage('local_library',{total:page.total,cueTracks:2,ncmTracks:1,guest:true});
   step='cue';await wait(()=>!button('播放全部').disabled,'player_ready');await click(el('[aria-label="播放本地歌曲 夜行 · 下篇"]'));
   await wait(()=>el('.playerbar [aria-label="暂停"]'),'cue_playing');
   let queue=await invoke('player_queue',{epoch});if(queue.tracks.length!==4)throw Error('incomplete_queue');
   const state=await invoke('player_action',{epoch,action:{kind:'pause',paused:true}});
   if(state.timeline.mode!=='cue'||state.timeline.media_start!==30||state.timeline.total!==90)throw Error('cue_bounds');
   step='cue_resume';await wait(()=>el('.playerbar [aria-label="播放"]'),'pause_presented');
   await click(el('.playerbar [aria-label="播放"]'));await click(el('[aria-label="打开播放详情"]'));
   await wait(()=>el('.lyrics-panel')?.textContent.includes('沿着灯火向前'),'cue_local_lyrics');
   await stage('local_cue',{nativePlayback:true,mediaStart:30,duration:90,sidecarLyrics:true,fullQueue:4});
   await click(el('[aria-label="收起播放详情"]'));
   step='groups';await click(button('歌手'));await wait(()=>el('.local-groups'),'artist_groups');await click(all('.local-groups button').find(n=>n.querySelector('span')?.textContent==='Cumulus'));
   await wait(()=>all('.local-tracks tbody tr').length===3,'artist_filter');await stage('local_groups',{group:'artist',tracks:3});
   await click(button('歌曲'));await wait(()=>all('.local-tracks tbody tr').length===4,'all_rows');
   step='history_replay';await click(button('最近播放'));await wait(()=>{if(el('.history-page .catalog-error'))throw Error('history_read: '+el('.history-page .catalog-error').textContent);return el('.history-page')?.getAttribute('aria-busy')==='false'&&el('.history-actions button');},'guest_history');
   const history=await invoke('history_page',{epoch,kind:'song',refresh:true});
   if(history.value.canSync||history.value.entries.length!==1||!history.value.entries[0].resource.track.local)throw Error('guest_history_scope');
   await sleep(400);await report('local_history_page',{guest:true,historyRows:all('.history-window .history-row').length,canSync:false});await sleep(900);
   await click(el('.history-actions [aria-label^="播放"]'));await sleep(1400);
   const replay=await invoke('player_action',{epoch,action:{kind:'pause',paused:true}});
   if(replay.queueLen!==1||replay.timeline.mode!=='cue'||replay.timeline.media_start!==30||replay.timeline.total!==90||replay.position<0.2)throw Error('history_native_cue_replay');
   await report('local_history_replay',{guest:true,queue:1,mediaStart:30,duration:90,position:replay.position,audioOutput:'null',historyReplayVerified:true});await sleep(900);
   await click(button('本地音乐'));await wait(()=>all('.local-tracks tbody tr').length===4,'library_after_history');
   step='history_fault';await report('history_fault',{enabled:true});await wait(()=>window.__historyFaultReady===true,'fault_ready');
   step='ncm';await click(el('[aria-label="播放本地歌曲 归途"]'));await wait(()=>el('.playerbar [aria-label="暂停"]')&&el('.playerbar').textContent.includes('归途'),'ncm_playing');
   await wait(()=>el('.playback-message')?.textContent.includes('最近播放暂未保存'),'history_warning');
   const beforeHistoryRetry=Number(el('[aria-label="播放进度"]').value);await sleep(1200);
   if(Number(el('[aria-label="播放进度"]').value)<=beforeHistoryRetry)throw Error('history_failure_stopped_audio');
   await stage('local_history_warning',{nativePlayback:true,historyWarningVisible:true,audioStillAdvancing:true});
   await report('history_fault',{enabled:false});await wait(()=>window.__historyFaultReady===false,'fault_removed');
   await wait(()=>!el('.playback-message')?.textContent.includes('最近播放暂未保存'),'history_recovered');
   step='ncm_seek';change('播放进度','12');await sleep(200);await wait(()=>{const pause=el('.playerbar [aria-label="暂停"]');return pause&&!pause.disabled&&Number(el('[aria-label="播放进度"]')?.value)>=11;},'ncm_seek');step='ncm_pause';await click(el('.playerbar [aria-label="暂停"]'));
   await wait(()=>el('.playerbar [aria-label="播放"]'),'ncm_pause');await stage('local_ncm',{nativePlayback:true,ncmDecoded:true,position:12,paused:true});
   const details=el('.local-roots');details.open=true;await click(button('暂停扫描'));await wait(()=>!!button('恢复扫描'),'watch_disable');await click(button('恢复扫描'));await wait(()=>!!button('暂停扫描'),'watch_enable');
  }
  await report('complete',{ok:true,reopened:window.__localProof.reopen,guest:true,networkRequests:0,remoteMutations:0,audioOutput:'null',historyRecorderVerified:true,width:innerWidth,height:innerHeight});
 }catch(e){await report('failed',{ok:false,step,error:e instanceof Error?e.message:'native_operation_failed',buttons:all('button[aria-label]').map(n=>({label:n.getAttribute('aria-label'),disabled:n.disabled})).slice(0,80)});}
})();
