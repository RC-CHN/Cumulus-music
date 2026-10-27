// Explicit opt-in only. The sole mutation target is the fresh local fixture.
window.__runCloudMutationProof=async({wait,stage,sleep,baseline,epoch,query,snapshot})=>{
  const fixture=window.__cloudProofFixture;
  const el=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)];
  const invoke=(name,args={})=>window.__TAURI_INTERNALS__.invoke(name,args);
  const button=(text,root=document)=>[...root.querySelectorAll('button')].find(b=>b.textContent.trim()===text);
  const click=async b=>{if(!b)throw Error('missing cloud roundtrip control');await wait(()=>!b.disabled,'roundtrip_control_ready');b.click();};
  const privateReport=value=>invoke('plugin:event|emit',{event:'cloud-proof-private',payload:{baseline,fixture,...value}});
  const readAll=async()=>{
    let first;
    for(let attempt=0;attempt<10;attempt++){
      try{first=await snapshot(query,true);break;}catch(error){if(error?.code!=='busy')throw error;await sleep(300);}
    }
    if(!first)throw Error('cloud_roundtrip_list_busy');
    const rows=[];
    for(let offset=0;offset<first.total;offset+=200){const p=await snapshot({...query,offset});if(p.revision!==first.revision)throw Error('cloud_roundtrip_snapshot_changed');rows.push(...p.tracks);}
    return {view:first,rows};
  };
  const candidate=rows=>{
    const matches=rows.filter(r=>!baseline.includes(r.track.id)&&r.fileName===fixture.name&&r.fileSize===fixture.bytes);
    if(matches.length>1)throw Error('ambiguous_fixture_identity');
    return matches[0];
  };
  const restored=rows=>JSON.stringify(rows.map(r=>r.track.id))===JSON.stringify(baseline);
  let submitted=false,uiDeleted=false,cleaned=false,taskId;
  await privateReport({phase:'prepared',cleanupRequired:false});
  try{
    const before=await readAll();if(!restored(before.rows))throw Error('baseline_changed_before_upload');
    if(fixture.resumeTaskId){
      const state=await snapshot({...query,tab:'uploading'});const task=state.uploads.find(t=>t.id===fixture.resumeTaskId);
      if(state.uploads.length!==1||!task||task.fileName!==fixture.name||task.total!==fixture.bytes||!task.canResume)throw Error('prior_upload_task_identity_or_state_mismatch');
      taskId=task.id;
      await click(all('.cloud-page [role="tab"]').find(b=>b.textContent.includes('上传任务')));
      await wait(()=>all('.cloud-upload-row').length===1,'previous_upload_task_render');
      if(el('.cloud-upload-name small')?.textContent!==fixture.name)throw Error('wrong_upload_task_rendered');
      await privateReport({phase:'resuming',cleanupRequired:true,taskId});submitted=true;
      await click(button('继续',el('.cloud-upload-row')));
      await stage('cloud_roundtrip_selected',{selectedFiles:1,sourceIsSynthetic:true,previousTaskResumed:true});
    }else{
    await click(button('上传音乐',el('.cloud-toolbar')));await wait(()=>el('.cloud-upload-dialog')?.open,'roundtrip_dialog');
    await click(button('选择音频文件',el('.cloud-upload-dialog')));
    await stage('cloud_roundtrip_picker',{nativePickerRequested:true});
    await wait(()=>all('.cloud-file-list article').length===1,'fixture_selected_in_native_picker');
    if(el('.cloud-file-title strong')?.textContent!==fixture.name)throw Error('wrong_file_selected');
    await stage('cloud_roundtrip_selected',{selectedFiles:1,sourceIsSynthetic:true});
    await privateReport({phase:'submitting',cleanupRequired:true});submitted=true;
    await click([...el('.cloud-upload-dialog').querySelectorAll('button')].find(b=>b.textContent.trim().startsWith('开始上传')));
    await wait(()=>!el('.cloud-upload-dialog')&&!!el('.cloud-upload-row'),'native_upload_task_created');
    }
    const deadline=Date.now()+150000;
    let uploaded=false;
    while(Date.now()<deadline){
      const state=await snapshot({...query,tab:'uploading'});const task=state.uploads.find(t=>t.fileName===fixture.name);
      if(task){taskId=task.id;if(task.phase==='complete'){uploaded=true;break;}if(task.error&&!task.running&&!task.enabled)throw Error('native_upload_stopped:'+task.error);}
      await sleep(750);
    }
    if(!uploaded)throw Error('native_upload_timeout');
    await stage('cloud_roundtrip_uploaded',{nativeTaskComplete:true,uploadedFiles:1});
    const after=await readAll();const created=candidate(after.rows);if(!created)throw Error('uploaded_fixture_not_identified');
    if(after.rows.length!==baseline.length+1||after.rows.some(r=>r.track.id!==created.track.id&&!baseline.includes(r.track.id)))throw Error('unexpected_remote_resource_change');
    await privateReport({phase:'published',cleanupRequired:true,taskId,songId:created.track.id});
    await click(all('.cloud-page [role="tab"]').find(b=>b.textContent.includes('已上传')));
    await wait(()=>all('.cloud-table tbody tr').length>0,'uploaded_catalog_after_mutation');
    const search=el('.cloud-search input');search.value=fixture.name;search.dispatchEvent(new Event('input',{bubbles:true}));el('.cloud-search').dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));
    await wait(()=>all('.cloud-table tbody tr').length===1,'isolated_uploaded_fixture_filter');
    const displayed=el('.cloud-track-title strong')?.textContent;
    if(displayed!==created.track.name)throw Error('isolated_fixture_row_mismatch');
    all('.cloud-table tbody input[type="checkbox"]')[0].click();await click(button('删除所选',el('.cloud-toolbar')));
    await wait(()=>!!el('#cloud-delete-title'),'isolated_delete_dialog');
    await stage('cloud_roundtrip_delete',{isolatedNewResource:true,selectedFiles:1});
    await click(button('删除歌曲',el('dialog')));
    await wait(()=>!el('#cloud-delete-title'),'isolated_delete_result');
    let result=await readAll();
    if(!restored(result.rows)){
      const op=result.view.deletions.find(o=>['sending','verifying','uncertain'].includes(o.phase));
      if(op){await invoke('cloud_command',{epoch,command:{kind:'reconcile_delete',id:op.id}});result=await readAll();}
    }
    if(!restored(result.rows))throw Error('cloud_ui_delete_not_confirmed');
    uiDeleted=true;cleaned=true;
    await privateReport({phase:'restored',cleanupRequired:false,taskId});
    await stage('cloud_roundtrip_cleanup',{baselineRestored:true,existingResourcesDeleted:0,uiDeleteConfirmed:true});
    return {cloudUploadUi:true,cloudDeleteUi:true,nativePickerSelected:!fixture.resumeTaskId,previousTaskResumed:!!fixture.resumeTaskId,baselineRestored:true,syntheticFilesUploaded:1,isolatedNewResourcesDeleted:1,existingResourcesDeleted:0,remoteAccountMutations:2};
  }finally{
    if(submitted&&!cleaned){
      // Read and identify before cleanup. Never choose a pre-existing ID, guess
      // from title alone, or replay an unknown delete.
      let tasks=await snapshot({...query,tab:'uploading'});
      let task=tasks.uploads.find(t=>t.fileName===fixture.name);
      if(task){
        taskId=task.id;
        if(task.phase!=='complete'&&(task.running||task.enabled))await invoke('cloud_command',{epoch,command:{kind:'pause',ids:[task.id]}});
        const until=Date.now()+60000;
        while(task.running&&Date.now()<until){await sleep(500);tasks=await snapshot({...query,tab:'uploading'});task=tasks.uploads.find(t=>t.id===taskId);if(!task)throw Error('cleanup_upload_disappeared');}
        if(task.running)throw Error('cleanup_waiting_for_active_upload');
      }
      let state=await readAll();let created=candidate(state.rows);
      if(!created&&taskId){
        const tasks=await snapshot({...query,tab:'uploading'});const task=tasks.uploads.find(t=>t.id===taskId);
        if(task?.phase==='publication_pending'){
          await invoke('cloud_command',{epoch,command:{kind:'reconcile_upload',id:taskId}});
          state=await readAll();created=candidate(state.rows);
        }
      }
      if(created){
        const pending=state.view.deletions.filter(o=>['sending','verifying','uncertain'].includes(o.phase));
        if(pending.length){for(const op of pending)await invoke('cloud_command',{epoch,command:{kind:'reconcile_delete',id:op.id}});}
        else await invoke('cloud_command',{epoch,command:{kind:'delete',ids:[created.track.id],revision:state.view.revision}});
      }
      const final=await readAll();const unknownUpload=task?.error==='write_outcome_unknown'&&!created&&task?.phase!=='publication_pending';
      cleaned=restored(final.rows)&&!unknownUpload;
      await privateReport({phase:cleaned?'restored':'needs_review',cleanupRequired:!cleaned,uiDeleted,taskId});
      await stage('cloud_roundtrip_cleanup',{baselineRestored:restored(final.rows),cleanupComplete:cleaned,existingResourcesDeleted:0,uiDeleteConfirmed:uiDeleted,fallbackCleanup:true});
    }
  }
};
