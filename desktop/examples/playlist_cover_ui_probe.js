// Drives production DOM controls. No business commands are invoked directly.
(async () => {
  const el=s=>document.querySelector(s), all=s=>[...document.querySelectorAll(s)];
  const sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const wait=async(fn,label)=>{const end=Date.now()+60000;while(Date.now()<end){if(fn())return;await sleep(100);}throw Error(label);};
  const button=(text,root=document)=>[...root.querySelectorAll('button')].find(n=>n.textContent.trim()===text);
  const click=async n=>{if(!n)throw Error('missing_control');await wait(()=>n.isConnected&&!n.disabled,'disabled_control');n.scrollIntoView({block:'nearest'});n.click();await sleep(100);};
  const input=(label,value)=>{const n=el(`[aria-label="${label}"]`);if(!n)throw Error('missing_input');n.value=value;n.dispatchEvent(new Event('input',{bubbles:true}));};
  const report=(stage,data={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'cover-proof',payload:{stage,...data}});
  const crop=()=>JSON.parse(el('.cover-dialog').dataset.crop||'null');
  const stable=()=>!el('.cover-dialog button.primary')?.disabled;
  const facts=()=>({crop:el('.cover-dialog')?crop():null,phase:el('.cover-dialog')?.dataset.phase,errors:all('.error-state').map(n=>n.textContent.trim()),width:innerWidth,height:innerHeight});
  async function stage(name,data={}) {
    await sleep(400);
    if(document.documentElement.scrollWidth>innerWidth||document.documentElement.scrollHeight>innerHeight)throw Error('page_overflow');
    const dialog=el('.cover-dialog');
    if(dialog){const r=dialog.getBoundingClientRect(),footer=dialog.querySelector('footer').getBoundingClientRect();if(r.left<0||r.top<0||r.right>innerWidth+1||r.bottom>innerHeight+1||footer.bottom>innerHeight)throw Error('cover_dialog_overflow');}
    window.__coverAck='';await report(name,{...facts(),...data});await wait(()=>window.__coverAck===name,'native_ack');await sleep(1100);
  }
  let step='account';
  try {
    await wait(()=>el('.account-button')&&!el('.account-button').disabled&&!el('.welcome'),'account_ready');
    step='library';await click(button('我的歌单'));await wait(()=>button('打开歌单链接'),'library');
    if(window.__coverProof.reopen){
      await wait(()=>all('button').find(n=>n.textContent.trim().startsWith('歌单操作记录')),'pending_badge');
      await click(all('button').find(n=>n.textContent.trim().startsWith('歌单操作记录')));
      await wait(()=>button('处理封面'),'pending_cover_record');await stage('cover_recovery_record');
      await click(button('处理封面'));await wait(()=>el('.cover-dialog .cover-crop-image')&&stable(),'restored_preview');
      await stage('cover_reopened');
    } else {
      await click(button('打开歌单链接'));await wait(()=>el('[aria-label="歌单分享链接"]'),'open_dialog');
      input('歌单分享链接',window.__coverProof.id);await click(el('.playlist-dialog button[type="submit"]'));
      await wait(()=>el('.playlist-hero')&&button('编辑歌单'),'playlist_loaded');
      await click(button('编辑歌单'));await wait(()=>el('[aria-label="替换歌单封面"]')&&!el('.playlist-dialog .spinner'),'editor_loaded');
      input('歌单名称','UnsavedCoverDraft');input('歌单简介','Unsaved description stays local');
      await click(el('[aria-label="替换歌单封面"]'));await wait(()=>button('选择图片',el('.cover-dialog')),'cover_open');
      await stage('cover_empty');
      step='picker_cancel';await click(button('选择图片',el('.cover-dialog')));await stage('cover_picker_cancel');
      await wait(()=>!button('选择图片',el('.cover-dialog')).disabled,'picker_cancelled');
      if(el('.cover-crop-image')||el('.cover-dialog').dataset.phase!=='empty')throw Error('cancel_created_task');
      step='invalid_image';await click(button('选择图片',el('.cover-dialog')));await stage('cover_picker_invalid');
      await wait(()=>el('.cover-dialog .error-state'),'invalid_error');
      if(!el('.cover-dialog .error-state').textContent.includes('320'))throw Error('incorrect_image_error');
      await stage('cover_invalid_rejected');
      step='select_image';await click(button('选择图片',el('.cover-dialog')));await stage('cover_picker_select');
      await wait(()=>el('.cover-crop-image')&&stable(),'preview_ready');
      if(JSON.stringify(crop())!==JSON.stringify({x:143,y:63,size:354}))throw Error('default_crop_differs');
      await stage('cover_selected');
      if(window.__coverProof.hold){await report('complete',{ok:true,held:true,...facts()});return;}
    }
    // Normal run filled the text draft before opening the nested cover modal.
    // Reopen restores the persisted image, not a discarded text form draft.
    const expectedName=el('[aria-label="歌单名称"]').value, expectedDescription=el('[aria-label="歌单简介"]').value;
    step='crop';const slider=el('[aria-label="封面缩放"]');slider.value='2';slider.dispatchEvent(new Event('input',{bubbles:true}));await sleep(200);
    const area=el('.cover-crop-area');area.focus();
    area.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowLeft',shiftKey:true,bubbles:true}));
    area.dispatchEvent(new PointerEvent('pointerdown',{pointerId:1,clientX:180,clientY:130,button:0,bubbles:true}));
    area.dispatchEvent(new PointerEvent('pointermove',{pointerId:1,clientX:210,clientY:140,bubbles:true}));
    area.dispatchEvent(new PointerEvent('pointerup',{pointerId:1,bubbles:true}));await sleep(200);
    const intended=crop();if(intended.size!==177||intended.x===232||intended.y===152)throw Error('crop_controls_did_not_move');
    await stage('cover_cropped',{intended});
    if(!window.__coverProof.reopen){
      step='replace_cancel';await click(button('重新选取图片',el('.cover-dialog')));await stage('cover_picker_replace_cancel');
      await wait(stable,'replace_cancelled');if(JSON.stringify(crop())!==JSON.stringify(intended))throw Error('cancel_lost_crop');
    }
    step='save';await click(button('保存',el('.cover-dialog')));
    await wait(()=>el('.cover-dialog')?.dataset.phase==='confirmed'&&button('完成',el('.cover-dialog'))&&!button('完成',el('.cover-dialog')).disabled,'cover_confirmed');
    await stage('cover_confirmed',{intended});
    await click(button('完成',el('.cover-dialog')));await wait(()=>!el('.cover-dialog'),'cover_closed');
    if(el('[aria-label="歌单名称"]').value!==expectedName||el('[aria-label="歌单简介"]').value!==expectedDescription)throw Error('metadata_draft_lost');
    await wait(()=>el('.playlist-cover-edit img')?.complete&&el('.playlist-cover-edit img')?.naturalWidth>0,'updated_cover_visible');
    await stage('cover_draft_preserved',{nameDraft:el('[aria-label="歌单名称"]').value,descriptionDraft:el('[aria-label="歌单简介"]').value,coverUrl:el('.playlist-cover-edit img').src});
    await click(el('[aria-label="关闭歌单操作"]'));await wait(()=>!el('.playlist-dialog'),'editor_closed_without_metadata_save');
    await report('complete',{ok:true,intended,held:false});
  } catch(e) {await report('failed',{ok:false,step,error:e instanceof Error?e.message:'unknown',...facts()});}
})();
