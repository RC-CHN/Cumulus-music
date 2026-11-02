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
 async function mode(value){const id=++sequence;await invoke('plugin:event|emit',{event:'email-login-fixture',payload:{id,mode:value}});await wait(()=>window.__emailFixtureAck===id,'fixture acknowledgement');return window.__emailFixtureSnapshot;}
 async function theme(value){localStorage.setItem('cumulus.appearance.v1',value);dispatchEvent(new StorageEvent('storage',{key:'cumulus.appearance.v1',newValue:value}));await wait(()=>document.documentElement.dataset.theme===value,'theme');await settle();}
 async function resize(width,height){await emit('resize',{width,height,scale:devicePixelRatio});await wait(()=>innerWidth===width&&innerHeight===height,'native resize');await settle();}
 const panel=()=>one('.email-verification'),phase=()=>panel()?.dataset.emailPhase;
 const idle=()=>!one('.login-flow[aria-busy="true"]');
 const pins=()=>all('.email-verification .login-pin input');
 async function pin(value){await wait(()=>pins().length===6&&!pins()[0].disabled,'editable PIN');const n=pins()[0];n.focus();n.value=value;n.dispatchEvent(new Event('input',{bubbles:true}));await sleep(0);return n;}
 async function stage(name,value={}){
  last=name;await settle();const d=one('dialog[open]'),r=d?.getBoundingClientRect();
  assert(document.documentElement.scrollWidth<=innerWidth+1,'page width');
  assert(!d||(r.top>=0&&r.bottom<=innerHeight+1&&d.scrollWidth<=d.clientWidth+1),'dialog bounds');
  if(panel()){assert(d.scrollHeight<=d.clientHeight+1,'email step fits short window');assert(one('#login-title').textContent==='安全邮箱验证','official email title');assert(one('.verification-identity-title')?.textContent==='用户昵称','official identity row');assert(!one('.login-challenge>p.muted'),'no unrelated phone hint');const address=one('.email-description strong');if(address)assert(address.textContent.includes('@example.test'),'mail domain remains visible');}
  assert(!d||d.contains(document.activeElement),'focus in active dialog');
  await emit(`ux_${String(++count).padStart(2,'0')}_${name}`,{...value,width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,ratio:devicePixelRatio,screenX:window.screenX,screenY:window.screenY,
   dialogs:d?[{open:true,opacity:getComputedStyle(d).opacity,background:getComputedStyle(d).backgroundColor,rect:r.toJSON()}]:[],
   loadingText:(()=>{const n=one('.email-loading-text');return n?{rect:n.getBoundingClientRect().toJSON(),color:getComputedStyle(n).color,background:getComputedStyle(one('dialog[open]')).backgroundColor}:null;})(),phase:phase()??null,overflow:false,focusValid:!d||d.contains(document.activeElement),focused:document.hasFocus(),codeLength:pins().reduce((n,p)=>n+p.value.length,0),pinCount:pins().length});
 }
 async function openChallenge(){await click(one('.account-button'));if(!one('.login-methods')){await wait(()=>one('.account-add'),'account chooser');await click(one('.account-add'));}await click(button('邮箱'));await fill('登录邮箱','verify@example.test');await fill('登录密码','fixture-password');await click(one('[aria-label="同意登录协议"]'));await click(one('.login-submit'));await wait(()=>button('获取验证方式'),'challenge');await click(button('获取验证方式'));await wait(()=>one('[data-verification-method="SECURITY_EMAIL"]'),'email offered');await settle();}
 async function choose(){await click(one('[data-verification-method="SECURITY_EMAIL"]'));await wait(()=>phase()&&phase()!=='sending'&&idle(),'email response');await settle();}
 async function back(){await click(one('.verification-back'));await wait(()=>one('.verification-methods'),'method list');await settle();assert(document.activeElement.dataset.verificationMethod==='SECURITY_EMAIL','return focus to email choice');}
 async function close(){await click(button('关闭登录'));await wait(()=>!one('dialog[open]'),'dialog closed');await settle();}
 async function logged(){await wait(()=>!one('dialog')&&one('.account-button').textContent.includes('二次验证账号'),'authenticated native account');await settle();}
 try{
  await wait(()=>one('.account-button')&&!one('.account-button').disabled&&one('.welcome'),'guest bootstrap');
  await theme('light');await resize(1120,760);await mode('normal');await openChallenge();await choose();assert(phase()==='code_required','initial auto send acknowledged');assert(panel().textContent.includes('10分钟'),'official validity hint');await stage('ready_light',{expectGuest:true,rememberEpoch:true});
  await pin('123');await stage('editing_light',{expectGuest:true});
  await mode('wrong');const wrong=await pin('000000');await wait(()=>idle()&&one('.email-pin .verification-error'),'wrong code');assert(wrong.value==='','wrong OTP cleared');assert(document.activeElement===pins()[0],'wrong PIN focus');await stage('wrong_light',{expectGuest:true});
  await theme('dark');await pin('1');assert(!one('.email-pin .verification-error'),'editing clears feedback');await stage('editing_dark',{expectGuest:true});
  await mode('business-false');await pin('123456');await wait(()=>idle()&&one('.email-pin .verification-error'),'business failure');await stage('business_false_dark',{expectGuest:true,preserveEpoch:true});
  await resize(760,540);await mode('verify-limit');await pin('222222');await wait(()=>phase()==='verify_limit'&&idle(),'verify limit');assert(pins().length===0,'limited code entry removed');await stage('verify_limit_small_dark',{expectGuest:true});
  await theme('light');await mode('normal');await click(button('重新发送'));await wait(()=>phase()==='code_required'&&idle(),'resend ready');await stage('resent_small_light',{expectGuest:true});
  await back();await mode('send-limit');await choose();assert(phase()==='send_limit'&&!one('.verification-back'),'send limit has only acknowledge action');await stage('send_limit_small_light',{expectGuest:true});
  await click(button('我知道了'));await wait(()=>one('.verification-methods'),'limit return');await settle();assert(document.activeElement.dataset.verificationMethod==='SECURITY_EMAIL','limit return focus');await stage('limit_returned_light',{expectGuest:true});
  await theme('dark');await mode('send-failed');await choose();assert(phase()==='failed','failed send feedback');await stage('send_failed_small_dark',{expectGuest:true});
  await mode('normal');await click(button('重试发送'));await wait(()=>phase()==='code_required'&&idle(),'explicit send retry');await stage('retry_ready_small_dark',{expectGuest:true});
  await back();await mode('send-lost');await choose();assert(phase()==='send_unconfirmed'&&!button('重新发送'),'unknown send not replayed');assert(!panel().textContent.includes('已发送安全验证码'),'no false acknowledgement');await stage('send_unknown_small_dark',{expectGuest:true});
  await mode('normal');await pin('123456');await logged();await stage('unknown_send_authorized',{expectNickname:'二次验证账号',rememberEpoch:true});
  await resize(1120,760);await theme('light');await openChallenge();await choose();await mode('verify-lost');const lost=await pin('123456');await wait(()=>phase()==='recovery_required'&&idle(),'unknown answer');assert(pins().length===0&&lost.value==='','unknown answer removed');await stage('verify_unknown_light',{expectNickname:'二次验证账号',preserveEpoch:true});
  await click(button('核对登录状态'));await logged();await stage('reconciled_light',{expectNickname:'二次验证账号',rememberEpoch:true});
  await theme('dark');await mode('normal');await openChallenge();await mode('hold-send');await click(one('[data-verification-method="SECURITY_EMAIL"]'));await wait(async()=>Boolean((await mode('read')).pending),'held send');await stage('send_inflight_dark',{expectNickname:'二次验证账号',preserveEpoch:true});await close();await mode('release');await stage('closed_send_dropped_dark',{expectNickname:'二次验证账号',preserveEpoch:true});
  await openChallenge();await choose();await mode('hold-verify');const held=await pin('123456');await wait(async()=>Boolean((await mode('read')).pending),'held verification');await stage('verify_inflight_dark',{expectNickname:'二次验证账号',preserveEpoch:true});await back();assert(held.value==='','stopped OTP cleared');await mode('release');await stage('stopped_verify_dark',{expectNickname:'二次验证账号',preserveEpoch:true});
  await theme('light');await choose();await pin('123');await stage('draft_light',{expectNickname:'二次验证账号',preserveEpoch:true});const detached=pins().slice();await close();assert(detached.every(p=>p.value===''),'closing clears retained PIN DOM');await stage('closed_draft_light',{expectNickname:'二次验证账号',preserveEpoch:true});
  await openChallenge();await choose();await pin('123456');await logged();await stage('authorized_light',{expectNickname:'二次验证账号',rememberEpoch:true});
  await emit('complete',{ok:true,stageCount:count,realMusicRequests:0,preserveEpoch:true});
 }catch(e){await emit('failed',{last,error:String(e),stack:e.stack,phase:phase()??null,email:panel()?.outerHTML??null});}
})();
