(async()=>{
 const invoke=(name,args={})=>window.__TAURI_INTERNALS__.invoke(name,args);
 const sleep=ms=>new Promise(r=>setTimeout(r,ms)),el=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)];
 const button=text=>all('button').find(b=>b.textContent.trim()===text);
 const wait=async(fn,name)=>{for(let i=0;i<400;i++){if(await fn())return;await sleep(100);}throw Error('timeout_'+name);};
 const click=async(node)=>{await sleep(100);if(!node||!node.isConnected||node.disabled)throw Error('button_unavailable');node.click();await sleep(100);};
 const report=(stage,data={})=>invoke('plugin:event|emit',{event:'matching-proof',payload:{stage,...data}});
 const stage=async(name,data={})=>{await sleep(400);if(document.documentElement.scrollWidth>innerWidth+1)throw Error('horizontal_overflow');await report(name,{width:innerWidth,height:innerHeight,...data});await sleep(900);};
 const query={keyword:'',sort:'title',descending:true,group:'list',groupKey:null,includeMissing:true,offset:0,limit:200,revision:null};
 let step='bootstrap';
 const wanted=`.local-match-dialog [data-song-id="${window.__matchingProof.songId}"]`;
 try {
  await wait(()=>button('本地音乐')&&!el('.main-content .spinner'),'account');
  const account=await invoke('account_snapshot');if(account.status!=='authenticated')throw Error('account_required');const epoch=account.epoch;
  await click(button('本地音乐'));await wait(()=>all('.local-tracks tbody tr').length===3,'local_rows');
  const page=await invoke('local_page',{query}),ids=page.tracks.map(r=>r.track.id);
  const plain=page.tracks.find(r=>r.fileName==='plain.flac');if(!plain)throw Error('plain_fixture_missing');
  const state=async()=> (await invoke('local_match_view',{epoch,ids})).value;
  const row=()=>el(`[data-local-id="${plain.track.id}"]`);
  if(window.__matchingProof.reopen) {
   step='restore';await wait(async()=> (await state()).links.length===3,'restored_links');
   await stage('matching_reopened',{links:3,restored:true});
  } else {
   step='matching';await wait(()=>!button('匹配音乐').disabled,'match_ready');await click(button('匹配音乐'));
   await wait(()=>button('暂停匹配')&&!button('暂停匹配').disabled,'pause_available');await click(button('暂停匹配'));
   await wait(async()=> (await state()).job?.phase==='paused','paused');await stage('matching_paused',{paused:true});
   await click(button('继续匹配'));await wait(async()=> (await state()).job?.phase==='complete','auto_complete');
   const auto=await state();await stage('matching_auto',{matched:auto.job.matched,unmatched:auto.job.unmatched,total:auto.job.total});
   step='manual';await click(row().querySelector('.local-online .text-button'));await wait(()=>el('.local-match-dialog'),'dialog');
   if(button('解除关联')) {await click(button('解除关联'));await wait(()=>!el('.local-match-dialog'),'clear');await click(row().querySelector('.local-online .text-button'));}
   await click(button('搜索'));await wait(()=>all('.local-match-dialog li').length||el('.local-match-dialog [role="alert"]'),'song_search_response');
   await stage('matching_search',{publicSearch:true,resultCount:all('.local-match-dialog li').length,ids:all('.local-match-dialog li').map(n=>n.dataset.songId),error:el('.local-match-dialog [role="alert"]')?.textContent||null});
   await wait(()=>el(wanted),'song_search');
   await click(el(wanted+' button'));await wait(()=>!el('.local-match-dialog'),'bind_saved');
   await wait(async()=> (await state()).links.length===3,'all_linked');await stage('matching_linked',{links:3,manualBinding:true});
   step='comments';await wait(()=>row().querySelector('[aria-label$="的线上评论"]'),'comment_entry');await click(row().querySelector('[aria-label$="的线上评论"]'));
   await wait(()=>el('.comments-page')&&!el('.comments-page .spinner'),'comments_loaded');await stage('matching_comments',{onlineComments:true,remoteMutations:0});
   await click(button('本地音乐'));await wait(()=>row(),'local_return');
  }
  step='playback';await wait(()=>!row().querySelector('.local-title').disabled,'play_ready');await click(row().querySelector('.local-title'));
  await wait(()=>el('.playerbar [aria-label="暂停"]'),'local_playback');
  await click(el('.playerbar [aria-label="暂停"]'));
  const queue=await invoke('player_queue',{epoch});if(queue.tracks.length!==3||!queue.tracks.every(q=>q.track.local))throw Error('local_queue_changed_to_online');
  await stage('matching_playback',{localQueue:3,paused:true,nativeDecoder:true});
  await report('complete',{ok:true,reopened:window.__matchingProof.reopen,width:innerWidth,height:innerHeight,remoteMutations:0,audioOutput:'null'});
 }catch(e){await report('failed',{ok:false,step,error:e instanceof Error?e.message:'operation_failed',searchIds:all('.local-match-dialog li').map(n=>n.dataset.songId),searchError:el('.local-match-dialog [role="alert"]')?.textContent||null});}
})();
