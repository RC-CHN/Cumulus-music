(async()=>{
  const sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const all=s=>[...document.querySelectorAll(s)],one=s=>document.querySelector(s);
  const invoke=(name,args={})=>window.__TAURI_INTERNALS__.invoke(name,args);
  const emit=(stage,rest={})=>invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...rest}});
  const wait=async(test,label)=>{for(let i=0;i<400;i++){if(await test())return;await sleep(100);}throw Error(label);};
  const button=text=>all('button').find(n=>n.textContent.trim()===text);
  const click=node=>{if(!node||node.disabled)throw Error('control unavailable');node.scrollIntoView({block:'nearest'});node.focus({preventScroll:true});node.click();};
  const counts={};let stageCount=0;
  try{
    await wait(()=>button('最近播放')&&one('.track-row'),'authenticated start');
    const account=await invoke('account_snapshot');if(account.status!=='authenticated')throw Error('existing authenticated profile required');
    click(button('最近播放'));
    for(const [kind,label] of [['song','歌曲'],['podcast','播客'],['audiobook','有声书'],['voice','声音'],['playlist','歌单'],['album','专辑']]){
      if(kind!=='song')click(all('[aria-label="播放记录类型"] button').find(n=>n.textContent.trim()===label));
      await wait(()=>one('.history-page')?.getAttribute('aria-busy')==='false'&&one('[aria-label="播放记录类型"] [aria-current]')?.textContent.trim()===label,'history '+kind);
      if(one('.history-page .catalog-error'))throw Error('history remote read failed '+kind);
      const result=await invoke('history_page',{epoch:account.epoch,kind,refresh:false});
      if(!result.value.remoteIncluded)throw Error('missing native remote snapshot');
      if(result.value.entries.length&&Number(one('.history-table')?.getAttribute('aria-rowcount'))!==result.value.entries.length+1)throw Error('native/UI count mismatch');
      counts[kind]=result.value.entries.length;
      await sleep(450);await emit(`ux_${String(++stageCount).padStart(2,'0')}_${kind}`,{kind,count:counts[kind],rows:all('.history-window .history-row').length,remoteIncluded:true});await sleep(900);
    }
    click(all('[aria-label="播放记录类型"] button').find(n=>n.textContent.trim()==='歌曲'));
    await wait(()=>one('.history-page')?.getAttribute('aria-busy')==='false'&&one('[aria-label="播放记录类型"] [aria-current]')?.textContent.trim()==='歌曲'&&one('.history-actions [aria-label^="播放"]')?.disabled===false&&!one('.history-page')?.closest('.page-leave-active'),'song replay ready');
    await sleep(450);click(one('.history-actions [aria-label^="播放"]'));
    await sleep(500);if(one('.playback-message')?.textContent.includes('失败'))throw Error('playback error: '+one('.playback-message').textContent);
    await wait(()=>one('.playerbar [aria-label="暂停"]')&&Number(one('[aria-label="播放进度"]')?.value)>0.5,'history native playback');
    await emit('ux_07_playing',{playing:true,source:'history',output:'null'});await sleep(1400);
    await emit('complete',{ok:true,counts,stageCount:7,explicitHistoryWrites:0,playbackOutput:'null'});
  }catch(error){await emit('failed',{ok:false,error:String(error),counts,stageCount,playerMessage:one('.playback-message')?.textContent,historyBusy:one('.history-page')?.getAttribute('aria-busy')});}
})();
