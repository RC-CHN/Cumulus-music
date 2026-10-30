// Actual production cover commands, core persistence and crop encoding against an in-memory service.
(async () => {
 const one=s=>document.querySelector(s), all=s=>[...document.querySelectorAll(s)], sleep=ms=>new Promise(r=>setTimeout(r,ms));
 const assert=(v,m)=>{if(!v)throw Error(m);};
 const wait=async(f,m)=>{for(let i=0;i<240;i++){if(f())return;await sleep(100);}throw Error(m);};
 const button=(label,root=document)=>[...root.querySelectorAll('button')].find(n=>n.getAttribute('aria-label')===label||n.textContent.trim()===label);
 const click=n=>{assert(n&&!n.disabled,'unavailable '+n?.outerHTML);n.focus({preventScroll:true});n.click();};
 const emit=(stage,data={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...data}});
 const dialog=()=>one('.cover-dialog'), phase=()=>dialog()?.dataset.phase, primary=()=>dialog()?.querySelector('footer .primary');
 const rect=n=>{const r=n.getBoundingClientRect();return{x:r.x,y:r.y,right:r.right,bottom:r.bottom,width:r.width,height:r.height};};
 const hit=n=>{if(!n)return false;const r=rect(n),at=document.elementFromPoint(r.x+r.width/2,r.y+r.height/2);return at===n||n.contains(at);};
 const baseline=!!window.__coverFeedbackBaseline;
 let count=0, originals, lastReport; const checks=[], cases=[];
 async function fixture(kind,value){window.__coverTransferAck=null;await emit('cover-transfer-fixture',{kind,value});await wait(()=>window.__coverTransferAck,'fixture');assert(window.__coverTransferAck.ok,JSON.stringify(window.__coverTransferAck));return lastReport=window.__coverTransferAck.report;}
 async function mode(value){return fixture('mode',value);}
 async function theme(width,height,value){await emit('resize',{width,height});await wait(()=>innerWidth===width&&innerHeight===height,'resize');click(button('外观主题'));await wait(()=>one('.action-menu'),'theme menu');click(button(value==='dark'?'深色':'浅色'));await wait(()=>!one('.action-menu')&&document.documentElement.dataset.theme===value,'theme');}
 async function capture(label){
  await wait(()=>{const d=dialog();return !d||(!d.matches('.modal-enter-active,.modal-leave-active')&&d.getAnimations().every(a=>a.playState!=='running'&&!a.pending)&&getComputedStyle(d).transform==='none');},'dialog animation settled');await sleep(120);
  const d=dialog(),b=d&&rect(d),body=d?.querySelector('.cover-body');
  const state={label,noOverflow:document.documentElement.scrollWidth<=innerWidth,contained:!d||(b.width===442&&b.y>=11&&b.bottom<=innerHeight-11),footerHit:!d||[...d.querySelectorAll('footer button')].every(hit),closeHit:!d||hit(button('关闭封面编辑',d)),bodyNoOverflow:!body||body.scrollWidth<=body.clientWidth};
  assert(state.noOverflow&&state.contained&&state.footerHit&&state.closeHit,JSON.stringify(state));if(!baseline){assert(state.bodyNoOverflow,'long text must wrap');const actions=d&&[...d.querySelectorAll('footer button')];if(actions?.length===2)assert(actions.every(n=>rect(n).width===175),'two equal official 175px actions');}checks.push(state);
  await emit(`ux_${String(++count).padStart(2,'0')}_${label}`,{width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,checks:state,phase:phase(),text:body?.textContent,buttons:d&&[...d.querySelectorAll('button')].map(n=>({text:n.textContent.trim(),disabled:n.disabled}))});await sleep(800);
 }
 function write(n,value){n.value=value;n.dispatchEvent(new Event('input',{bubbles:true}));}
 async function begin(value){
  await mode('success');click(button('我的歌单'));await wait(()=>button('管理歌单：合成歌单 3')||button('管理歌单：其他设备更新后的歌单'),'library');click(button('管理歌单：合成歌单 3')||button('管理歌单：其他设备更新后的歌单'));await wait(()=>one('.action-menu'),'menu');click(button('编辑歌单信息',one('.action-menu')));await wait(()=>one('[aria-label="歌单名称"]')?.value,'editor');
  const name=one('[aria-label="歌单名称"]'),desc=one('[aria-label="歌单简介"]');originals=[name.value,desc.value];write(name,'封面流程中的文字草稿');write(desc,'保存封面时保留这段未提交的简介。');
  await fixture('seed');await mode(value);click(button('替换歌单封面'));await wait(()=>phase()==='draft'&&one('.cover-crop-image')?.complete,'draft');
 }
 async function close(){click(button('关闭封面编辑',dialog()));await wait(()=>!dialog(),'closed');assert(document.activeElement===button('替换歌单封面'),'return focus');}
 async function leave(){if(dialog())await close();assert(one('[aria-label="歌单名称"]').value==='封面流程中的文字草稿'&&one('[aria-label="歌单简介"]').value==='保存封面时保留这段未提交的简介。','text draft preserved');write(one('[aria-label="歌单名称"]'),originals[0]);write(one('[aria-label="歌单简介"]'),originals[1]);await sleep(50);click(button('取消编辑歌单'));await sleep(50);if(button('放弃修改并离开'))click(button('放弃修改并离开'));else if(button('离开并稍后核对'))click(button('离开并稍后核对'));await wait(()=>!one('.playlist-editor-page'),'leave');}
 async function save(){click(primary());}
 async function settled(target){await wait(()=>phase()===target&&!button('关闭封面编辑',dialog())?.disabled,'settled '+target);}
 async function success(label){await wait(()=>baseline?phase()==='confirmed'&&!button('关闭封面编辑',dialog()).disabled:!dialog(),'confirmed');if(!baseline){assert(one('.playlist-editor-page [role="status"]')?.textContent.includes('封面已保存'),'visible saved notice');assert(document.activeElement===button('替换歌单封面'),'success returns focus');}await capture(label);}
 try{
  await wait(()=>one('.track-row'),'startup');
  for(const [w,h,t] of [[1120,760,'light'],[760,540,'light'],[760,540,'dark'],[1120,760,'dark']]){
   await theme(w,h,t);await begin('hold');await save();await wait(()=>button('暂停上传',dialog())&&one('progress')?.max>262144,'multichunk upload');await capture(`uploading_${w}_${t}`);
   click(button('暂停上传',dialog()));await sleep(150);if(!baseline)assert(button('正在暂停…',dialog())?.disabled,'pause remains pending until transfer stops');await capture(`pause_requested_${w}_${t}`);await fixture('release');await wait(()=>!button('关闭封面编辑',dialog()).disabled&&dialog().textContent.includes('上传已暂停'),'paused');await capture(`paused_${w}_${t}`);
   await save();await success(`saved_${w}_${t}`);await leave();cases.push('pause_'+w+'_'+t);
  }
  for(const [w,h,t] of [[760,540,'dark'],[1120,760,'light']]){
   await theme(w,h,t);const suffix=w+'_'+t;
   for(const [kind,target] of [['allocation_unknown','allocating'],['upload_unknown','uploading'],['binding_rejected','rejected']]){
    await begin(kind);await save();await settled(target);if(kind!=='binding_rejected')assert(primary().disabled,'uncertain transfer cannot repeat save');await capture(kind+'_'+suffix);
    if(kind!=='binding_rejected'){click(button('放弃本次换图',dialog()));await settled('abandoned');await capture('abandoned_'+kind+'_'+suffix);}await leave();cases.push(kind+'_'+suffix);
   }
   await begin('auth_error');await save();await wait(()=>button('重新获取上传权限',dialog())&&!button('关闭封面编辑',dialog()).disabled,'upload authentication failed');if(!baseline)assert(!dialog().textContent.includes('正在上传图片'),'error must not claim upload is active');await capture('auth_error_'+suffix);await mode('success');click(button('重新获取上传权限',dialog()));await wait(()=>!button('关闭封面编辑',dialog()).disabled,'renewed');await capture('renewed_'+suffix);await save();await success('renewed_saved_'+suffix);await leave();cases.push('auth_'+suffix);
   await begin('binding_unknown');await save();await settled('binding');await capture('binding_unknown_'+suffix);const report=await fixture('read_ready');await mode('binding_unknown');click(button('核对结果',dialog()));await success('binding_reconciled_'+suffix);const after=await mode('success');assert(after.bindings===report.bindings,'reconcile must not bind twice');await leave();cases.push('binding_'+suffix);
   await begin('conflict');await save();await settled('uploaded');assert(primary().disabled,'conflict requires explicit review');await capture('conflict_'+suffix);click(button('查看最新歌单信息',dialog()));await wait(()=>button('确认将这张封面应用到最新歌单',dialog()),'latest details');if(!baseline)assert(hit(button('确认将这张封面应用到最新歌单',dialog())),'conflict action automatically visible');await capture('latest_'+suffix);click(button('确认将这张封面应用到最新歌单',dialog()));await success('conflict_saved_'+suffix);await leave();cases.push('conflict_'+suffix);
   await begin('cleanup_failure');await save();await settled('confirmed');assert(button('重试清理图片缓存',dialog()),'cleanup warning stays visible');await capture('cleanup_warning_'+suffix);click(button('重试清理图片缓存',dialog()));await wait(()=>!button('关闭封面编辑',dialog()).disabled,'failed cleanup');await capture('cleanup_failed_'+suffix);await fixture('restore_cache');click(button('重试清理图片缓存',dialog()));await success('cleanup_done_'+suffix);await leave();cases.push('cleanup_'+suffix);
  }
  if(!baseline){
   await theme(760,540,'dark');await begin('expired');await save();await wait(()=>!dialog()&&!one('.playlist-editor-page')&&one('.welcome'),'expired account leaves editor');assert(button('重新登录')&&one('.welcome h1').textContent.includes('登录已失效'),'explicit expired-session recovery entry');await capture('session_expired_760_dark');await theme(1120,760,'light');await capture('session_expired_1120_light');cases.push('expired');
  }
  await emit('complete',{ok:true,stageCount:count,checks,cases,realMusicRequests:0,realAccountWrites:0,officialPixelMatchVerified:false});
 }catch(error){await emit('ux_failed',{message:String(error),phase:phase(),text:dialog()?.textContent});await sleep(850);await emit('failed',{message:String(error),stack:error?.stack,stageCount:count,active:document.activeElement?.outerHTML.slice(0,400)});}
})();
