(async()=>{
 const one=s=>document.querySelector(s), all=s=>[...document.querySelectorAll(s)], sleep=ms=>new Promise(r=>setTimeout(r,ms));
 const assert=(ok,message)=>{if(!ok)throw Error(message);};
 const invoke=window.__TAURI_INTERNALS__.invoke.bind(window.__TAURI_INTERNALS__);
 const emit=(stage,data={})=>invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...data}});
 const wait=async(test,label)=>{for(let i=0;i<180;i++){if(test())return;await sleep(80);}throw Error(label);};
 const button=text=>all('button').find(b=>b.textContent.trim()===text);
 const click=b=>{assert(b&&!b.disabled,'unavailable button');b.scrollIntoView({block:'nearest'});b.focus({preventScroll:true});b.click();};
 const dialog=()=>one('.like-recovery-dialog[open]');
 const row=id=>one(`.like-recovery-list [data-song-id="${id}"]`);
 const ready=()=>dialog()&&!dialog().querySelector('button:disabled');
 const heart=id=>all('.track-row .like-button').find(b=>b.getAttribute('aria-label').includes(`合成歌曲 ${id} ·`));
 const operation=id=>row(id)?.querySelector('.secondary');
 const theme=async value=>{click(one('[aria-label="外观主题"]'));await wait(()=>one('.action-menu'),'theme menu');click(button(value==='dark'?'深色':'浅色'));await wait(()=>document.documentElement.dataset.theme===value,'theme changed');};
 let count=0,loseAck=false,recoveryReplies=0;
 async function capture(name){
   await sleep(300);assert(scrollY===0&&scrollX===0,'desktop frame scrolled');assert(document.documentElement.scrollWidth<=innerWidth,'horizontal overflow');
   for(const node of all('dialog[open],.like-recovery-notice')){const r=node.getBoundingClientRect();assert(r.top>=0&&r.bottom<=innerHeight&&r.left>=0&&r.right<=innerWidth,'recovery outside window');}
   if(dialog()){assert(dialog().contains(document.activeElement),'recovery lost keyboard focus');assert(all('.like-recovery-list li').length<=20,'unbounded rows');}
   await emit(`ux_${String(++count).padStart(2,'0')}_${name}`,{width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,dialog:!!dialog(),pendingRows:all('.like-recovery-list li').length});await sleep(700);
 }
 async function open(){click(button('查看并核对'));await wait(()=>dialog()&&row('2')||dialog()&&row('4')||dialog()&&row('5'),'recovery dialog');}
 async function close(){click(one('[aria-label="关闭喜欢状态核对"]'));await wait(()=>!dialog(),'closed');await sleep(250);}
 try{
   await wait(()=>one('#app')?.__vue_app__?._context?.provides,'typed client provider');
   const provides=one('#app').__vue_app__._context.provides;
   const core=provides[Reflect.ownKeys(provides).find(key=>typeof key==='symbol'&&key.description==='core-client')];
   assert(core?.recoverLike,'typed like recovery adapter');
   const nativeCommand=core.recoverLike;
   const intercept=async(...args)=>{
     let result;try{result=await nativeCommand(...args);}finally{recoveryReplies++;}
     if(args[1].kind==='accept_current'&&loseAck){loseAck=false;throw {code:'network',message:'合成确认回包丢失'};}
     return result;
   };
   core.recoverLike=intercept;assert(core.recoverLike===intercept,'adapter fault injection unavailable');
   await wait(()=>heart(2)&&one('.account-button')&&!one('.account-button').disabled,'signed in');
   if(localStorage.getItem('cumulus.like-recovery-proof.done')){
     await wait(()=>one('.like-recovery-notice'),'pending restored');await open();assert(row('5'),'restored wrong resource');await capture('reopened_pending');
     await emit('like-fixture',{id:'5',remote:true});click(operation('5'));await wait(()=>!row('5')&&ready(),'restored reconciliation');await capture('reopened_confirmed');
     await close();assert(document.activeElement===one('.settings-button'),'restored focus fallback');await capture('reopened_closed');
     await emit('complete',{ok:true,stageCount:count,reopened:true,realMusicRequests:0});return;
   }
   await theme('light');click(heart(2));await wait(()=>one('.like-recovery-notice'),'lost applied like');await open();await capture('applied_unknown_light');
   assert(!button('保留当前状态'),'accept before observation');click(operation('2'));await wait(()=>!row('2')&&ready(),'read-only applied recovery');await capture('applied_confirmed');await close();assert(!one('.error-state')&&!one('.main-content .notice:not(.like-recovery-notice)'),'stale like failure after recovery');
   await wait(()=>heart(2).getAttribute('aria-pressed')==='true','heart readback');assert(document.activeElement===one('.settings-button'),'close fallback focus');
   await theme('dark');await emit('resize',{width:760,height:540});await wait(()=>innerWidth===760,'small window');
   click(heart(4));await wait(()=>one('.like-recovery-notice'),'unapplied lost like');await open();assert(!dialog().textContent.includes('已确认喜欢状态'),'stale success notice on a new intent');await capture('unapplied_unknown_dark');
   await emit('like-fixture',{failRead:true});click(operation('4'));await wait(()=>ready()&&one('.like-recovery-dialog .catalog-error'),'read failure');await capture('read_failed');
   click(operation('4'));await wait(()=>ready()&&button('保留当前状态'),'negative observation');await capture('unapplied_checked');
   click(button('保留当前状态'));await wait(()=>button('确认保留当前状态'),'confirmation');assert(document.activeElement===button('取消'),'confirmation must default to cancel');await capture('accept_confirmation');click(button('取消'));await wait(()=>row('4'),'cancelled');await capture('accept_cancelled');
   click(button('保留当前状态'));await wait(()=>button('确认保留当前状态'),'confirm again');await emit('like-fixture',{id:'4',remote:true});click(button('确认保留当前状态'));await wait(()=>one('.like-recovery-dialog .catalog-error')&&ready(),'changed server rejected');assert(one('.like-recovery-dialog .catalog-error').textContent.includes('请取消本次确认并重新核对'),'wrong state-change guidance');await capture('changed_remote');click(button('取消'));
   await emit('like-fixture',{id:'4',remote:false});const before=recoveryReplies;click(operation('4'));await wait(()=>recoveryReplies>before&&ready()&&button('保留当前状态'),'check again');click(button('保留当前状态'));await wait(()=>button('确认保留当前状态'),'lost acknowledgement setup');loseAck=true;click(button('确认保留当前状态'));await wait(()=>!row('4')&&!button('确认保留当前状态')&&ready(),'lost acknowledgement inspected');assert(!loseAck&&one('.like-recovery-dialog .catalog-error')?.textContent.includes('合成确认回包丢失'),'lost acknowledgement fault was not injected');await capture('accept_lost_ack');
   click(button('刷新待核对记录'));await wait(()=>ready()&&!one('.like-recovery-dialog .catalog-error'),'read refresh cleared error');await close();
   await emit('like-fixture',{healthy:true});click(heart(4));await wait(()=>heart(4).getAttribute('aria-pressed')==='true'&&!heart(4).disabled,'explicit new like after local resolution');await capture('new_like_allowed');
   await emit('like-fixture',{healthy:false});click(heart(5));await wait(()=>one('.like-recovery-notice'),'pending for restart');await open();await capture('pending_before_restart');
   localStorage.setItem('cumulus.like-recovery-proof.done','1');await emit('complete',{ok:true,stageCount:count,realMusicRequests:0});
 }catch(error){await emit('failed',{ok:false,error:String(error),stageCount:count,body:document.body.innerText.slice(-2200)});}
})();
