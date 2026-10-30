// Production editor page / real IPC and core journals, isolated synthetic server.
(async()=>{
  const sleep=ms=>new Promise(r=>setTimeout(r,ms)),one=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)];
  const assert=(v,m)=>{if(!v)throw Error(m);};
  const wait=async(f,m)=>{for(let i=0;i<180;i++){if(f())return;await sleep(100);}throw Error(m);};
  const button=(label,root=document)=>[...root.querySelectorAll('button')].find(n=>n.getAttribute('aria-label')===label||n.textContent.trim()===label);
  const click=n=>{assert(n&&!n.disabled,'unavailable '+n?.outerHTML);n.focus({preventScroll:true});n.click();};
  const emit=(stage,data={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...data}});
  const rect=n=>{const r=n.getBoundingClientRect();return{x:r.x,y:r.y,right:r.right,bottom:r.bottom,width:r.width,height:r.height};};
  const visible=n=>{const r=rect(n);return r.width>0&&r.height>0&&r.x>=0&&r.y>=0&&r.right<=innerWidth+1&&r.bottom<=innerHeight+1;};
  const hit=n=>{const r=rect(n);const at=document.elementFromPoint(r.x+r.width/2,r.y+r.height/2);return visible(n)&&(at===n||n.contains(at));};
  const focusFits=n=>{const r=rect(n),body=rect(one('.playlist-editor-body')),style=getComputedStyle(n),outset=Math.max(0,parseFloat(style.outlineWidth)+parseFloat(style.outlineOffset));return style.outlineStyle!=='none'&&r.x-outset>=body.x&&r.y-outset>=body.y&&r.right+outset<=body.right&&r.bottom+outset<=body.bottom;};
  const input=(label,value)=>{const n=one(`[aria-label="${label}"]`);n.value=value;n.dispatchEvent(new Event('input',{bubbles:true}));};
  async function mode(value,failRead=0){window.__editorFixtureAck=false;await emit('editor-fixture',{mode:value,failRead});await wait(()=>window.__editorFixtureAck,'fixture control');}
  async function edit(id,ready=true){
    click(button('我的歌单'));await wait(()=>button(`管理歌单：合成歌单 ${id}`),'library list');
    click(button(`管理歌单：合成歌单 ${id}`));await wait(()=>one('.action-menu'),'card menu');click(button('编辑歌单信息',one('.action-menu')));
    await wait(()=>one('.playlist-editor-page'),'page opened');if(ready)await wait(()=>one('[aria-label="歌单名称"]')?.value,'editor loaded');await sleep(250);
  }
  async function close(discard=false){click(button('取消编辑歌单'));if(discard){await wait(()=>one('.playlist-editor-confirm'),'leave confirmation');click(button('放弃修改并离开'));}await new Promise(requestAnimationFrame);const previous=one('.content-page-previous');if(one('.playlist-editor-page'))assert(getComputedStyle(previous).display==='none'||getComputedStyle(previous).visibility==='hidden','retained page must not overlap editor exit animation');await wait(()=>!one('.playlist-editor-page'),'editor closed');await sleep(150);}
  let stageCount=0;const checks=[];
  async function capture(label){
    await sleep(250);const page=one('.playlist-editor-page'),modal=one('dialog[open]');
    const popup=modal||one('.tag-picker');const buttons=popup?[...popup.querySelectorAll('footer button')]:all('.playlist-editor-actions > button,.playlist-editor-actions .menu-trigger');
    const row={label,noOverflow:document.documentElement.scrollWidth<=innerWidth,controlsHit:buttons.every(hit),pageWithinContent:!page||rect(page).bottom<=rect(one('.playerbar')).y+1,modalVisible:!modal||visible(modal)};
    assert(Object.entries(row).every(([k,v])=>k==='label'||v),JSON.stringify(row));checks.push(row);
    await emit(`ux_${String(++stageCount).padStart(2,'0')}_${label}`,{width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,checks:row,editor:page&&rect(page),fields:one('.playlist-editor-fields')&&rect(one('.playlist-editor-fields')),cover:one('.playlist-cover-edit button')&&rect(one('.playlist-cover-edit button')),footer:one('.playlist-editor-actions')&&rect(one('.playlist-editor-actions'))});await sleep(850);
  }
  try{
    await wait(()=>one('.track-row'),'initial page');
    for(const [width,height,value] of [[1120,760,'light'],[760,540,'light'],[760,540,'dark'],[1120,760,'dark']]){
      await emit('resize',{width,height});await wait(()=>innerWidth===width&&innerHeight===height,'resize');
      click(button('外观主题'));await wait(()=>one('.action-menu'),'theme');click(button(value==='dark'?'深色':'浅色'));await wait(()=>!one('.action-menu')&&document.documentElement.dataset.theme===value,'theme applied');
      await edit(3);const suffix=width+'_'+value;
      assert(!one('.playlist-dialog[open]')&&one('.main-content .playlist-editor-page'),'editor must be a content page');
      assert(button('保存修改').disabled,'unchanged metadata cannot be sent');
      assert(rect(one('.playlist-cover-edit')).x>rect(one('.playlist-editor-fields')).right,'cover must be right of fields');
      assert(rect(one('[aria-label="歌单名称"]')).height===36,'name field source height');
      assert(focusFits(one('[aria-label="歌单名称"]')),'initial name focus ring must not be clipped');
      if(width===1120){assert(rect(one('.playlist-cover-edit button')).width===178,'official cover box');assert(Math.abs(rect(one('.playlist-cover-edit')).x-rect(one('.playlist-editor-fields')).right-45)<1,'official column gap');}
      await capture('editor_'+suffix);
      click(button('选择标签'));await wait(()=>one('.tag-picker section'),'tags');click(button('摇滚',one('.tag-picker')));await sleep(100);click(button('古典',one('.tag-picker')));await sleep(100);assert(button('爵士',one('.tag-picker')).disabled,'three-tag limit');click(button('移除标签 古典'));await sleep(100);assert(!button('爵士',one('.tag-picker')).disabled,'removing tag re-enables choice');one('.playlist-editor-body').scrollTop=10000;await capture('tags_'+suffix);click(button('完成选择'));await wait(()=>!one('.tag-picker'),'tags closed');click(button('选择标签'));await wait(()=>one('.tag-picker'),'tags reopen');button('选择标签').dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true}));await wait(()=>!one('.tag-picker'),'tag trigger Escape');
      const cover=button('替换歌单封面');cover.scrollIntoView({block:'nearest'});cover.focus();await sleep(180);assert(getComputedStyle(cover).borderRadius==='12px'&&getComputedStyle(cover.querySelector('span')).flexDirection==='row'&&Number(getComputedStyle(cover.querySelector('span')).opacity)===1,'cover source geometry and keyboard overlay');await capture('cover_focus_'+suffix);window.__coverFixtureAck=null;await emit('cover-fixture',{kind:'landscape',playlistId:'3'});await wait(()=>window.__coverFixtureAck,'cover fixture');assert(window.__coverFixtureAck.ok,'cover fixture prepared');click(cover);await wait(()=>one('.cover-dialog[open]'),'cover modal');await capture('cover_'+suffix);click(button('关闭封面编辑'));await wait(()=>!one('.cover-dialog'),'cover closed');assert(document.activeElement===cover,'cover returns focus: '+document.activeElement?.outerHTML.slice(0,240)+' connected='+cover.isConnected+' disabled='+cover.disabled);
      input('歌单名称','未保存草稿');input('歌单简介','保留这段简介');await sleep(100);
      click(button('每日推荐'));await wait(()=>one('.playlist-editor-confirm'),'navigation guarded');await capture('leave_'+suffix);assert(document.activeElement===button('继续编辑'),'safe leave focus');if(value==='dark')one('.playlist-editor-confirm').dispatchEvent(new Event('cancel',{cancelable:true}));else click(button('继续编辑'));await wait(()=>!one('.playlist-editor-confirm'),'continue');assert(one('[aria-label="歌单名称"]').value==='未保存草稿','draft survived cancelled navigation');await capture('draft_'+suffix);
      for(const label of ['歌单名称','歌单简介']){const field=one(`[aria-label="${label}"]`);field.focus();await sleep(100);assert(visible(field)&&focusFits(field),'field and focus ring scroll into view');}
      await capture('focus_'+suffix);click(button('其他歌单操作'));await wait(()=>one('.action-menu'),'management menu');assert(one('.playlist-editor-page')&&one('[aria-label="歌单名称"]').value==='未保存草稿'&&!one('.mutation-result'),'opening menu must not submit metadata');await capture('management_'+suffix);document.activeElement.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true}));await wait(()=>!one('.action-menu'),'management escape');await close(true);
    }
    await edit(3);input('歌单名称','@错误');input('歌单简介','中'.repeat(501));await capture('invalid');assert(button('保存修改').disabled,'invalid form disabled');await close(true);
    await mode('hold_read');await edit(4,false);await wait(()=>one('.playlist-editor-page .spinner'),'read loading');await capture('read_loading');await mode('success',4);await wait(()=>one('.playlist-editor-page .error-state'),'read failure');await capture('read_failed');click(button('重新读取歌单'));await wait(()=>one('[aria-label="歌单名称"]')?.value,'read retry');await capture('read_recovered');click(button('返回上一页'));await wait(()=>!one('.playlist-editor-page'),'back returns from editor');
    for(const [id,failure] of [[5,'reject'],[6,'partial'],[7,'uncertain']]){
      await mode('success');await edit(id);input('歌单名称',`测试修改 ${id}`);input('歌单简介',`合成简介 ${id}`);await sleep(100);await mode(failure);click(button('保存修改'));
      await wait(()=>one('.mutation-result')?.dataset.phase&&(!button('保存修改')||button('保存修改').textContent.trim()!=='正在保存…'),'write result');
      const phase=one('.mutation-result').dataset.phase;assert(phase==={reject:'rejected',partial:'partial',uncertain:'uncertain'}[failure],'expected '+failure+', got '+phase);
      assert(one('[aria-label="歌单名称"]').value===`测试修改 ${id}`&&one('[aria-label="歌单简介"]').value===`合成简介 ${id}`,'failed write must preserve text draft');await capture(failure);
      await mode('success');if(failure==='uncertain'){assert(button('保存修改').disabled,'pending write cannot repeat');click(button('核对结果'));}else click(button('保存修改'));
      await wait(()=>!one('.playlist-editor-page'),'save/reconcile confirmed');assert(all('.playlist-card').some(n=>n.getAttribute('aria-label')===`测试修改 ${id}`&&n.querySelector('.card-title').textContent===`测试修改 ${id}`),'confirmed name must be read back in library');await capture(failure+'_recovered');
    }
    await mode('success');await edit(8);input('歌单名称','保存中的歌单');await sleep(100);await mode('hold_write');click(button('保存修改'));
    await wait(()=>button('保存修改')?.textContent.trim()==='正在保存…','write busy');click(button('每日推荐'));await sleep(100);assert(one('.playlist-editor-page')&&one('.playlist-editor-page .notice')?.textContent.includes('请稍候'),'busy departure must stay on editor');await capture('write_busy');
    await mode('success');await wait(()=>!one('.playlist-editor-page'),'busy write confirmed');assert(all('.playlist-card').some(n=>n.getAttribute('aria-label')==='保存中的歌单'&&n.querySelector('.card-title').textContent==='保存中的歌单'),'saved name readback');await capture('write_confirmed');
    await edit(9);input('歌单名称','放弃的草稿');await sleep(100);click(button('每日推荐'));await wait(()=>one('.playlist-editor-confirm'),'discard navigation prompt');click(button('放弃修改并离开'));await wait(()=>!one('.playlist-editor-page')&&one('.daily-heading'),'discard navigated');await capture('navigation_discarded');
    await emit('complete',{ok:true,stageCount,checks,realMusicRequests:0,realAccountWrites:0,saveConfirmed:true,draftRetained:true,navigationGuard:true,reconciliationWithoutDuplicateWrite:true});
  }catch(e){await emit('failed',{message:String(e),stack:e?.stack,stageCount});}
})();
