// Only enabled in the standalone real-WebView probe. All mutations target a
// newly created private playlist. Original complete IDs/order are read first.
window.__runPlaylistProof=async({wait,stage,sleep})=>{
  const el=s=>document.querySelector(s);
  const invoke=(name,args={})=>window.__TAURI_INTERNALS__.invoke(name,args);
  const account=await invoke('account_snapshot'),epoch=account.epoch;
  const baseline=(await invoke('account_playlists')).value.map(p=>p.id);
  const name=`UIRust${crypto.randomUUID().replaceAll('-','').slice(0,8)}`,renamed=name.replace('UIRust','UIEdit');
  let id,cleaned=false,edited=false,originalOrder;
  const click=async node=>{if(!node||node.disabled)throw new Error('playlist_control_unavailable');node.scrollIntoView({block:'nearest'});await sleep(100);node.click();};
  const button=(text,root=document)=>[...root.querySelectorAll('button')].find(n=>n.getAttribute('aria-label')===text||n.textContent.trim()===text);
  const input=(label,value)=>{const n=el(`.playlist-dialog [aria-label="${label}"], .playlist-editor-page [aria-label="${label}"]`);if(!n)throw new Error('missing_playlist_input');n.value=value;n.dispatchEvent(new Event('input',{bubbles:true}));};
  const closed=()=>!el('.playlist-dialog')&&!el('.playlist-editor-page');
  const snapshot=async()=> (await invoke('playlist_editor',{epoch,id})).value;
  const edit=async()=>{await click(button('编辑歌单'));await wait(()=>el('.playlist-editor-page [aria-label="歌单名称"]'),'editor_loaded');};
  const submit=async()=>{const n=el('.playlist-editor-confirm button.primary, .playlist-dialog button[type="submit"], .playlist-editor-page button[type="submit"]');await wait(()=>n&&!n.disabled&&!el('.playlist-dialog fieldset')?.disabled,'playlist_submit_ready');await click(n);await wait(closed,'playlist_submit_confirmed');};
  window.__playlistProofFacts=()=>({dialog:!!el('.playlist-dialog'),message:el('.playlist-dialog .error-state')?.textContent?.trim().slice(0,180),result:el('.mutation-result')?.dataset.phase,isolatedCreated:!!id,cleanupComplete:cleaned});
  try{
    await click(document.querySelectorAll('.nav-main button')[1]);await wait(()=>el('.playlist-card'),'playlist_library');
    await click(button('新建歌单'));await wait(()=>el('.playlist-dialog [aria-label="歌单名称"]'),'create_dialog');
    input('歌单名称',name);
    if(!el('[aria-label="设为私密歌单"]').checked)throw new Error('create_not_private');
    await submit();
    const lists=(await invoke('account_playlists')).value;
    const matches=lists.filter(p=>p.name===name&&p.private&&p.owned&&!p.favorite&&!baseline.includes(p.id));
    if(matches.length!==1)throw new Error('created_identity_ambiguous');id=matches[0].id;
    await wait(()=>el('.playlist-hero h1')?.textContent===name,'created_page');
    await stage('playlist_create',{private:true,newPlaylistOnly:true,baselineCount:baseline.length});
    await edit();input('歌单名称',renamed);input('歌单简介','Tauri form and Rust core verification');
    await click(button('选择标签',el('.playlist-editor-page')));await wait(()=>button('电子',el('.tag-picker')),'official_tags');
    await click(button('电子',el('.tag-picker')));await submit();edited=true;
    let p=await snapshot();if(p.name!==renamed||p.description!=='Tauri form and Rust core verification'||!p.tags.includes('电子'))throw new Error('metadata_not_observed');
    await edit();await stage('playlist_edit',{name:true,description:true,officialTag:true,readbackConfirmed:true});
    await click(el('[aria-label="取消编辑歌单"]'));
    await click(document.querySelectorAll('.nav-main button')[0]);await wait(()=>el('h1')?.textContent==='每日推荐'&&document.querySelectorAll('.track-more').length>1,'daily_add_candidates');
    for(let i=0;i<2;i++){
      await click(document.querySelectorAll('.track-more')[i]);await click(button('收藏',el('.song-action-menu')));await wait(()=>el('[aria-label="选择目标歌单"]'),'track_menu');
      const select=el('[aria-label="选择目标歌单"]');if(![...select.options].some(o=>o.value===id))throw new Error('isolated_destination_missing');
      select.value=id;select.dispatchEvent(new Event('change',{bubbles:true}));
      await click(button('添加到歌单',el('.playlist-dialog')));await wait(closed,'add_song_confirmed');
      p=await snapshot();if(p.trackIds.length!==i+1)throw new Error('added_count_mismatch');
    }
    await click(document.querySelectorAll('.nav-main button')[1]);await wait(()=>[...document.querySelectorAll('.playlist-card')].some(n=>n.querySelector('.card-title')?.textContent===renamed),'library_new_card');
    await click([...document.querySelectorAll('.playlist-card')].find(n=>n.querySelector('.card-title')?.textContent===renamed));
    await wait(()=>document.querySelectorAll('.track-more').length===2,'two_tracks');
    originalOrder=(await snapshot()).trackIds;
    await click(el('.track-more'));await click(button('调整歌曲顺序…',el('.song-action-menu')));await click(button('下移一首',el('.playlist-dialog')));await wait(closed,'move_song_confirmed');
    p=await snapshot();if(JSON.stringify(p.trackIds)!==JSON.stringify([...originalOrder].reverse()))throw new Error('reorder_not_observed');
    const viewport=el('.track-scroll');
    if(viewport.clientHeight<112)throw new Error('playlist_rows_clipped_by_header');
    if(document.documentElement.scrollWidth>innerWidth)throw new Error('playlist_horizontal_overflow');
    await stage('playlist_reorder',{tracks:2,fullOrderReversed:true,listHeight:viewport.clientHeight,horizontalOverflow:false});
    await click(el('.track-more'));await click(button('从歌单中删除',el('.song-action-menu')));await click(button('删除',el('.playlist-dialog')));await wait(closed,'remove_song_confirmed');
    if((await snapshot()).trackIds.length!==1)throw new Error('remove_not_observed');
    await edit();await click(button('其他歌单操作'));await wait(()=>el('.action-menu'),'editor_action_menu');await click(button('清空歌曲',el('.action-menu')));await wait(()=>el('.playlist-editor-confirm'),'editor_confirmation');await submit();
    if((await snapshot()).trackIds.length)throw new Error('clear_not_observed');
    await stage('playlist_clear',{remaining:0,clearUsesDeletion:true});
    await edit();await click(button('其他歌单操作'));await wait(()=>el('.action-menu'),'editor_action_menu');await click(button('删除歌单',el('.action-menu')));await wait(()=>el('.playlist-editor-confirm'),'editor_confirmation');await submit();
    await wait(()=>el('h1')?.textContent==='我的歌单','delete_returns_library');
  }finally{
    // Cleanup is a typed native command after UI assertions, never an unsafe
    // general API replay. Preserve anything not provably this run's resource.
    const current=(await invoke('account_playlists')).value;
    if(!id){const found=current.filter(p=>[name,renamed].includes(p.name)&&p.owned&&p.private&&!p.favorite&&!baseline.includes(p.id));if(found.length===1)id=found[0].id;}
    if(id&&current.some(p=>p.id===id)){
      const p=await snapshot();if(!p.owned||!p.private||p.favorite||![name,renamed].includes(p.name)||baseline.includes(id))throw new Error('cleanup_ownership_guard');
      const pending=(await invoke('playlist_operations',{epoch})).value.filter(o=>['sending','verifying','uncertain'].includes(o.phase));
      for(const op of pending)await invoke('reconcile_playlist',{epoch,operationId:op.operationId});
      const fresh=await snapshot();const receipt=(await invoke('mutate_playlist',{epoch,command:{kind:'delete',id,revision:fresh.revision}})).value;
      if(receipt.phase!=='confirmed')throw new Error('cleanup_pending');
    }
    const after=(await invoke('account_playlists')).value.map(p=>p.id);
    if(JSON.stringify(after)!==JSON.stringify(baseline))throw new Error('original_playlist_order_not_restored');
    cleaned=true;
  }
  await stage('playlist_cleanup',{cleanupComplete:cleaned,fullOriginalOrderRestored:true,metadataEdited:edited,baselineCount:baseline.length});
  return {isolatedPlaylistMutations:true,cleanupComplete:cleaned,fullOriginalOrderRestored:true};
};
