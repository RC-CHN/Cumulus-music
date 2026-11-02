// User actions traverse real Vue, typed IPC and Rust; service modes are private.
(async()=>{
 const invoke=(command,args={})=>window.__TAURI_INTERNALS__.invoke(command,args);
 const sleep=ms=>new Promise(r=>setTimeout(r,ms));
 const assert=(v,message)=>{if(!v)throw Error(message);};
 const all=s=>[...document.querySelectorAll(s)].filter(n=>!n.closest('.modal-leave-active,.tab-leave-active'));
 const one=s=>all(s)[0];
 const button=t=>all('button').find(n=>n.textContent.trim()===t||n.getAttribute('aria-label')===t);
 const wait=async(fn,name,ms=24000)=>{const end=performance.now()+ms;while(performance.now()<end){if(await fn())return;await sleep(40);}throw Error(name);};
 const emit=(stage,value={})=>invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...value}});
 const settle=async()=>{await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));await wait(()=>!document.getAnimations().some(a=>Number.isFinite(a.effect?.getComputedTiming().endTime)&&(a.pending||a.playState==='running')),'finite animations settled');await sleep(100);};
 async function click(node){await wait(()=>node?.isConnected&&!node.disabled&&!node.closest('[inert]'),'enabled control');node.scrollIntoView({block:'nearest'});await settle();node.focus({preventScroll:true});const r=node.getBoundingClientRect(),hit=document.elementFromPoint(r.x+r.width/2,r.y+r.height/2);assert(hit===node||node.contains(hit),'pointer hit');node.click();await sleep(0);}
 async function fill(label,value){await wait(()=>all(`[aria-label="${label}"]`).some(n=>!n.disabled),'editable '+label);const node=all(`[aria-label="${label}"]`).find(n=>!n.disabled);node.focus();node.value=value;node.dispatchEvent(new Event('input',{bubbles:true}));await sleep(0);return node;}
 let sequence=0,count=0,last='boot';
 async function mode(value){const id=++sequence;await invoke('plugin:event|emit',{event:'question-login-fixture',payload:{id,mode:value}});await wait(()=>window.__questionFixtureAck===id,'fixture acknowledgement');return window.__questionFixtureSnapshot;}
 async function theme(value){localStorage.setItem('cumulus.appearance.v1',value);dispatchEvent(new StorageEvent('storage',{key:'cumulus.appearance.v1',newValue:value}));await wait(()=>document.documentElement.dataset.theme===value,'theme');await settle();}
 async function resize(width,height){await emit('resize',{width,height,scale:devicePixelRatio});await wait(()=>innerWidth===width&&innerHeight===height,'native resize');await settle();}
 const panel=()=>one('.question-verification'),phase=()=>panel()?.dataset.questionPhase;
 const idle=()=>!one('.login-flow[aria-busy="true"]');
 const choices=()=>all('.question-options input');
 async function choose(index){const input=choices()[index];assert(input&&!input.disabled,'editable question choice');input.focus({preventScroll:true});await click(input.closest('label'));assert(input.checked,'native radio selection');}
 async function stage(name,value={}){
  last=name;await settle();const d=one('dialog[open]'),r=d?.getBoundingClientRect();
  assert(document.documentElement.scrollWidth<=innerWidth+1,'page width');
  assert(!d||(r.top>=0&&r.bottom<=innerHeight+1&&d.scrollWidth<=d.clientWidth+1),'dialog bounds');
  if(panel()){
   assert(d.scrollHeight<=d.clientHeight+1,'question fits short window');
   assert(one('#login-title').textContent==='回答与你账号相关的问题','official question title');
   assert(!one('.login-restart')&&!one('.login-methods'),'question owns its actions');
   assert(!one('.login-status').textContent.trim(),'no duplicate generic question hint');
   const options=one('.question-options');if(options){const box=options.getBoundingClientRect();assert(box.height<=294.5&&options.scrollHeight>options.clientHeight,'bounded scrollable options');assert(choices().length===7,'six offered plus none');}
  }
  assert(!d||d.contains(document.activeElement),'focus in active dialog');
  const card=one('.question-picture'),style=card&&getComputedStyle(card);
  await emit(`ux_${String(++count).padStart(2,'0')}_${name}`,{...value,width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,ratio:devicePixelRatio,screenX:window.screenX,screenY:window.screenY,
   dialogs:d?[{open:true,opacity:getComputedStyle(d).opacity,background:getComputedStyle(d).backgroundColor,rect:r.toJSON()}]:[],
   loadingText:(()=>{const n=one('.question-loading-text');return n?{rect:n.getBoundingClientRect().toJSON(),color:getComputedStyle(n).color,background:getComputedStyle(d).backgroundColor}:null;})(),phase:phase()??null,overflow:false,focusValid:!d||d.contains(document.activeElement),focused:document.hasFocus(),choiceCount:choices().length,checked:choices().filter(n=>n.checked).length,card:card?{width:card.getBoundingClientRect().width,radius:style.borderRadius}:null});
 }
 async function openQuestion(){await click(one('.account-button'));if(!one('.login-methods')){await wait(()=>one('.account-add'),'account chooser');await click(one('.account-add'));}await click(button('邮箱'));await fill('登录邮箱','question@example.test');await fill('登录密码','fixture-password');await click(one('.login-submit'));await wait(()=>panel(),'question entered');}
 async function ready(){await wait(()=>phase()==='ready'&&idle(),'question loaded');await settle();}
 async function close(){await click(button('关闭登录'));await wait(()=>!one('dialog[open]'),'dialog closed');await settle();}
 async function logged(){await wait(()=>!one('dialog')&&one('.account-button').textContent.includes('安全问题账号'),'authenticated account');await settle();}
 try{
  await wait(()=>one('.account-button')&&!one('.account-button').disabled&&one('.welcome'),'guest bootstrap');
  await theme('light');await resize(1120,760);await mode('device');await openQuestion();await ready();await stage('device_ready_light',{expectGuest:true,rememberEpoch:true});
  assert(document.activeElement===choices()[0],'loaded question focus');assert(button('下一步').disabled,'empty selection blocked');await choose(0);await stage('device_selected_light',{expectGuest:true});
  await mode('wrong');await click(button('下一步'));await wait(()=>idle()&&one('.login-questions [role="alert"]'),'wrong answer');assert(choices()[0].checked,'known wrong retains selection');assert(document.activeElement===choices()[0],'failed answer returns focus to selected choice');await stage('wrong_light',{expectGuest:true});
  await theme('dark');await choose(1);assert(!one('.login-questions [role="alert"]'),'editing clears answer feedback');await stage('edited_dark',{expectGuest:true});
  await resize(760,540);await mode('business-false');await click(button('下一步'));await wait(()=>idle()&&one('.login-questions [role="alert"]'),'business false');await stage('business_false_small_dark',{expectGuest:true,preserveEpoch:true});
  const old=choices().slice();await close();assert(old.every(n=>!n.checked),'close clears retained answer DOM');await stage('closed_answer_dark',{expectGuest:true,preserveEpoch:true});
  await theme('light');await resize(1120,760);await mode('album');await openQuestion();await ready();assert(one('.question-options').dataset.questionType==='2','album type');await stage('album_ready_light',{expectGuest:true});
  await resize(760,540);await choose(6);await stage('album_none_small_light',{expectGuest:true});await close();
  await theme('dark');await mode('artist');await openQuestion();await ready();assert(one('.question-options').dataset.questionType==='3','artist type');await stage('artist_ready_small_dark',{expectGuest:true});await choose(0);await stage('artist_selected_small_dark',{expectGuest:true});await close();
  await mode('read-failed');await openQuestion();await wait(()=>phase()==='failed'&&idle(),'known read failure');await stage('read_failed_small_dark',{expectGuest:true});
  await mode('device');await click(button('重试读取'));await ready();await stage('read_retried_small_dark',{expectGuest:true});await close();
  await theme('light');await mode('read-lost');await openQuestion();await wait(()=>phase()==='failed'&&idle(),'lost read may retry');await stage('read_lost_small_light',{expectGuest:true});
  await mode('device');await click(button('重试读取'));await ready();await stage('read_recovered_small_light',{expectGuest:true});
  await mode('verify-lost');await choose(6);await click(button('下一步'));await wait(()=>phase()==='recovery_required'&&idle(),'lost write recovery');assert(choices().length===0&&!button('重试读取'),'unknown answer cannot repeat');await stage('verify_unknown_small_light',{expectGuest:true,preserveEpoch:true});
  await mode('hold-identity');await click(button('核对登录状态'));await wait(async()=>Boolean((await mode('read')).pending),'held reconciliation');await stage('reconcile_inflight_small_light',{expectGuest:true,preserveEpoch:true});
  await close();await wait(async()=>(await mode('read')).dropped===1,'reconciliation future dropped');await mode('release');await stage('closed_reconcile_dropped_light',{expectGuest:true,preserveEpoch:true});
  await mode('device');await openQuestion();await ready();await choose(6);await mode('verify-lost');await click(button('下一步'));await wait(()=>phase()==='recovery_required'&&idle(),'second unknown answer');await stage('verify_unknown_again_small_light',{expectGuest:true,preserveEpoch:true});
  await mode('device');await click(button('核对登录状态'));await logged();await stage('reconciled_light',{expectNickname:'安全问题账号',rememberEpoch:true});
  await theme('dark');await resize(1120,760);await mode('hold-read');await openQuestion();await wait(async()=>Boolean((await mode('read')).pending),'held read');await stage('read_inflight_dark',{expectNickname:'安全问题账号',preserveEpoch:true});await close();await mode('release');await stage('closed_read_dropped_dark',{expectNickname:'安全问题账号',preserveEpoch:true});
  await mode('device');await openQuestion();await ready();await choose(0);await mode('hold-verify');await click(button('下一步'));await wait(async()=>Boolean((await mode('read')).pending),'held answer');const held=choices().slice();await stage('verify_inflight_dark',{expectNickname:'安全问题账号',preserveEpoch:true});
  await click(button('返回登录'));await wait(()=>one('.credential-form')&&idle(),'back to login');assert(held.every(n=>!n.checked),'back clears retained selection');await mode('release');await stage('returned_login_dark',{expectNickname:'安全问题账号',preserveEpoch:true});await close();
  await theme('light');await mode('device');await openQuestion();await ready();await choose(1);const draft=choices().slice();await close();assert(draft.every(n=>!n.checked),'draft cleared');await stage('closed_draft_light',{expectNickname:'安全问题账号',preserveEpoch:true});
  await openQuestion();await ready();await choose(0);await click(button('下一步'));await logged();await stage('authorized_light',{expectNickname:'安全问题账号',rememberEpoch:true});
  await emit('complete',{ok:true,stageCount:count,realMusicRequests:0,preserveEpoch:true});
 }catch(e){await emit('failed',{last,error:String(e),stack:e.stack,phase:phase()??null,question:panel()?.outerHTML??null});}
})();
