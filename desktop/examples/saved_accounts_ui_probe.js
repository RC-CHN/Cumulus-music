(async()=>{
 const sleep=ms=>new Promise(r=>setTimeout(r,ms)),el=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)];
 const report=(stage,data={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'saved-account-proof',payload:{stage,...data}});
 const wait=async(fn,label)=>{const end=Date.now()+30000;while(Date.now()<end){if(fn())return;await sleep(100);}throw Error(label);};
 const click=async n=>{if(!n)throw Error('missing_control');await wait(()=>n.isConnected&&!n.disabled,'disabled_control');n.scrollIntoView({block:'nearest'});n.click();await sleep(150);};
 const text=s=>all('button').find(b=>b.textContent.trim()===s);
 const rows=()=>all('.saved-account');
 const remove=()=>el('[aria-label="移除账号 邮箱验证账号"]');
 async function stage(name){await sleep(400);const d=el('dialog'),r=d?.getBoundingClientRect();if(document.documentElement.scrollWidth>innerWidth||d&&(d.scrollWidth>d.clientWidth+1||r.top<0||r.bottom>innerHeight+1))throw Error('layout_overflow');window.__savedAck='';await report(name,{overflow:false,width:innerWidth,height:innerHeight});await wait(()=>window.__savedAck===name,'native_ack');await sleep(900);}
 try{
  await wait(()=>el('.account-button')?.textContent.includes('二次验证账号')&&!el('.account-button').disabled,'current_account');
  if(window.__savedReopen)await stage('saved_reopen_main');
  await click(el('.account-button'));await wait(()=>el('.account-add')&&!el('.account-dialog [role="status"]'),'accounts');
  if(window.__savedReopen){
   if(rows().length!==1||all('.saved-remove').length)throw Error('removed_account_returned');
   await stage('saved_reopen_accounts');
  }else{
   if(rows().length!==2||all('.saved-remove').length!==1)throw Error('current_account_delete_exposed');
   await stage('saved_list');await click(remove());await click(text('取消'));
   if(rows().length!==2)throw Error('cancel_removed_account');await stage('saved_cancelled');
   await click(remove());await stage('saved_confirm');await click(text('确认移除'));
   await wait(()=>rows().some(r=>r.textContent.includes('凭据清理未完成')),'pending_cleanup');
   const pending=rows().find(r=>r.textContent.includes('邮箱验证账号'));if(!pending.disabled)throw Error('pending_account_switchable');
   await stage('saved_pending');await click(text('继续移除'));await click(text('确认移除'));
   await wait(()=>rows().length===1&&!el('.account-dialog [role="alert"]'),'cleanup_retry');
   await stage('saved_done');
  }
  await report('complete',{ok:true,reopened:window.__savedReopen});
 }catch(e){await report('failed',{ok:false,error:e instanceof Error?e.message:'unknown'});}
})();
