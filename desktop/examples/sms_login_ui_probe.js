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
 async function mode(value){const id=++sequence;await invoke('plugin:event|emit',{event:'sms-login-fixture',payload:{id,mode:value}});await wait(()=>window.__smsFixtureAck===id,'fixture acknowledgement');}
 async function theme(value){localStorage.setItem('cumulus.appearance.v1',value);dispatchEvent(new StorageEvent('storage',{key:'cumulus.appearance.v1',newValue:value}));await wait(()=>document.documentElement.dataset.theme===value,'theme');await settle();}
 async function resize(width,height){await emit('resize',{width,height,scale:devicePixelRatio});await wait(()=>innerWidth===width&&innerHeight===height,'native resize');await settle();}
 const sms=()=>one('.sms-verification');
 const phase=()=>sms()?.dataset.smsPhase;
 const idle=()=>!one('.login-flow[aria-busy="true"]');
 async function stage(name,value={}){
  last=name;await settle();const d=one('dialog[open]'),r=d?.getBoundingClientRect();
  assert(document.documentElement.scrollWidth<=innerWidth+1,'page width');
  assert(!d||(r.top>=0&&r.bottom<=innerHeight+1&&d.scrollWidth<=d.clientWidth+1),'dialog bounds');
  if(sms())assert(d.scrollHeight<=d.clientHeight+1,'SMS step fits short window without vertical scrolling');
  assert(!d||d.contains(document.activeElement),'focus in active dialog');
  if(sms())assert(one('#login-title').textContent==='登录安全确认','official SMS title');
  const input=one('[aria-label="短信安全验证码"]');const cooldown=all('button').find(n=>/^\d{2}:\d{2}$/.test(n.textContent.trim()))?.textContent.trim()??null;if(cooldown)assert(/^\d{2}:[0-5]\d$/.test(cooldown),'valid minute/second countdown');
  await emit(`ux_${String(++count).padStart(2,'0')}_${name}`,{...value,width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,ratio:devicePixelRatio,screenX:window.screenX,screenY:window.screenY,
   dialogs:d?[{open:true,opacity:getComputedStyle(d).opacity,background:getComputedStyle(d).backgroundColor,rect:r.toJSON()}]:[],
   cooldown,phase:phase()??null,direction:sms()?.dataset.smsDirection??null,overflow:false,focusValid:!d||d.contains(document.activeElement),focused:document.hasFocus(),codeLength:input?.value.length??0,
   phone:one('.sms-phone')?.textContent??null,smsInfo:one('.sms-instructions')?.textContent??null});
 }
 async function openChallenge(){await click(one('.account-button'));if(!one('.login-methods')){await wait(()=>one('.account-add'),'account chooser');await click(one('.account-add'));}await wait(()=>one('.login-methods'),'login methods');await click(button('邮箱'));await fill('登录邮箱','sms@example.test');await fill('登录密码','fixture-password');await click(one('[aria-label="同意登录协议"]'));await click(one('.login-submit'));await wait(()=>button('获取验证方式'),'challenge');await click(button('获取验证方式'));await wait(()=>one('[data-verification-method="UP_SMS"]'),'SMS offered');await settle();}
 async function choose(){await click(one('[data-verification-method="UP_SMS"]'));await wait(()=>sms()&&idle(),'SMS ready');await settle();}
 async function back(){await click(button('选择其他验证方式'));await wait(()=>one('.verification-methods'),'verification methods');await settle();assert(document.activeElement.dataset.verificationMethod==='UP_SMS','return focus to SMS choice');}
 async function close(){await click(button('关闭登录'));await wait(()=>!one('dialog[open]'),'dialog closed');await settle();}
 async function logged(){await wait(()=>!one('dialog')&&one('.account-button').textContent.includes('邮箱验证账号'),'native authenticated account');await settle();}
 try{
  await wait(()=>one('.account-button')&&!one('.account-button').disabled&&one('.welcome'),'guest bootstrap');
  if(window.__smsSendFailure){
   await theme('light');await resize(760,540);await mode('non86');await openChallenge();await choose();await fill('短信安全验证码','123');await stage('downlink_edited_before_send',{expectGuest:true,rememberEpoch:true});
   await mode('send_failed');await click(button('获取验证码'));await wait(()=>idle()&&one('.verification-error')?.textContent.includes('发送未成功'),'fresh send failure after editing');assert(one('[aria-label="短信安全验证码"]').value==='','new send clears obsolete OTP');assert(document.activeElement===one('[aria-label="短信安全验证码"]'),'send result returns focus to OTP');await stage('downlink_send_failed_light',{expectGuest:true,preserveEpoch:true});
   await theme('dark');await stage('downlink_send_failed_dark',{expectGuest:true,preserveEpoch:true});await back();await choose();assert(all('button').some(n=>/^\d{2}:[0-5]\d$/.test(n.textContent.trim())&&n.disabled),'cooldown survives changing verification method');await stage('send_failure_cooldown_persists',{expectGuest:true,preserveEpoch:true});
   await emit('complete',{ok:true,stageCount:count,realMusicRequests:0,preserveEpoch:true});return;
  }
  await theme('light');await resize(1120,760);await mode('normal');await openChallenge();await stage('methods_light',{expectGuest:true});
  await choose();assert(sms().dataset.smsDirection==='uplink','86 defaults uplink');assert(sms().textContent.includes('138****8000')&&!sms().textContent.includes('139****1111'),'official authDetail mask');await stage('uplink_ready_light',{expectGuest:true});
  await resize(760,540);await theme('dark');await stage('uplink_small_dark',{expectGuest:true});
  await mode('check_hold');await click(button('已发送，立即登录'));await wait(()=>button('验证中…')?.disabled,'native check pending');await stage('checking_inflight',{expectGuest:true,rememberEpoch:true});
  await back();await stage('stopped_inflight',{expectGuest:true,preserveEpoch:true});await mode('release');await stage('late_check_dropped',{expectGuest:true,preserveEpoch:true});
  await mode('business_false');await choose();await click(button('已发送，立即登录'));await wait(()=>phase()==='failed','business failure');await stage('uplink_business_failed',{expectGuest:true});
  await click(button('重新发送'));await wait(()=>phase()==='ready'&&idle(),'fresh SMS info');assert(sms().textContent.includes('1069000002'),'backup destination on explicit retry');await stage('backup_destination',{expectGuest:true});
  await click(button('已发送，立即登录'));await wait(()=>phase()==='failed','second business failure');await click(button('无法发送？获取短信验证码'));await wait(()=>one('[aria-label="短信安全验证码"]')&&!one('[aria-label="短信安全验证码"]').disabled,'explicit downlink alternative');await stage('downlink_alternative',{expectGuest:true});
  await mode('normal');await click(button('获取验证码'));await wait(()=>all('button').some(n=>/^\d{2}:[0-5]\d$/.test(n.textContent.trim())),'native send cooldown');await stage('downlink_sent',{expectGuest:true});
  const retired=await fill('短信安全验证码','000000');await click(button('确认验证'));await wait(()=>one('.verification-error')?.textContent.trim(),'wrong OTP');assert(retired.value==='','wrong OTP cleared');await stage('downlink_wrong_code',{expectGuest:true});
  await fill('短信安全验证码','123');assert(!one('.verification-error').textContent.trim(),'editing clears OTP feedback');await stage('downlink_editing',{expectGuest:true});
  await mode('down_lost');await fill('短信安全验证码','123456');await click(button('确认验证'));await wait(()=>phase()==='recovery_required','unknown OTP result');assert(retired.value==='','lost OTP cleared');assert(!button('确认验证'),'unknown OTP cannot resubmit');assert(!one('.verification-error').textContent.trim(),'unknown new result has no stale wrong-code feedback');await stage('downlink_unknown',{expectGuest:true});
  await click(button('核对登录状态'));await logged();await stage('downlink_reconciled',{expectNickname:'邮箱验证账号',rememberEpoch:true});
  await mode('non86');await openChallenge();await choose();assert(sms().dataset.smsDirection==='downlink','non86 defaults downlink');assert(one('.sms-phone').textContent.includes('+1'),'country label');await stage('non86_downlink',{expectNickname:'邮箱验证账号',preserveEpoch:true});await back();await close();
  await mode('no_country');await openChallenge();await choose();assert(sms().dataset.smsDirection==='downlink'&&!one('.sms-phone').textContent.includes('+'),'missing country is not fabricated');await stage('missing_country_downlink',{expectNickname:'邮箱验证账号',preserveEpoch:true});await back();await close();
  await mode('info_lost');await openChallenge();await choose();assert(phase()==='recovery_required','lost info exposed');await stage('info_unknown',{expectNickname:'邮箱验证账号',preserveEpoch:true});await mode('normal');await click(button('重新获取短信信息'));await wait(()=>phase()==='ready'&&idle(),'manual info recovery');await stage('info_recovered',{expectNickname:'邮箱验证账号',preserveEpoch:true});await back();
  await mode('timeout');await choose();await click(button('已发送，立即登录'));await wait(()=>phase()==='timed_out','real thirty second native deadline',40000);await stage('uplink_timed_out',{expectNickname:'邮箱验证账号',preserveEpoch:true});await mode('normal');await click(button('重新发送'));await wait(()=>phase()==='ready'&&idle(),'manual timeout restart');await stage('timeout_restarted',{expectNickname:'邮箱验证账号',preserveEpoch:true});
  await click(button('已发送，立即登录'));await logged();await stage('uplink_authorized',{expectNickname:'邮箱验证账号',rememberEpoch:true});
  await mode('normal');await openChallenge();await mode('get_hold');await click(one('[data-verification-method="UP_SMS"]'));await wait(()=>one('.sms-loading'),'pending SMS info');await stage('info_inflight',{expectNickname:'邮箱验证账号',preserveEpoch:true});await close();await mode('release');await stage('closed_info_dropped',{expectNickname:'邮箱验证账号',preserveEpoch:true});
  await mode('non86');await openChallenge();await choose();await mode('check_hold');const detached=await fill('短信安全验证码','123456');await click(button('确认验证'));await wait(()=>one('.login-flow[aria-busy="true"]'),'downlink answer in flight');await stage('downlink_inflight',{expectNickname:'邮箱验证账号',preserveEpoch:true});await close();assert(detached.value==='','closing clears retained OTP DOM');await mode('release');await stage('closed_answer_dropped',{expectNickname:'邮箱验证账号',preserveEpoch:true});
  await emit('complete',{ok:true,stageCount:count,realMusicRequests:0,preserveEpoch:true});
 }catch(e){await emit('failed',{last,error:String(e),stack:e.stack,status:one('[data-login-status]')?.dataset.loginStatus??null,sms:sms()?.outerHTML??null});}
})();
