// DOM-only business actions. Synthetic mode control is private to this host.
(async () => {
  const invoke = (command,args={}) => window.__TAURI_INTERNALS__.invoke(command,args);
  const sleep = ms => new Promise(resolve=>setTimeout(resolve,ms));
  const assert = (v,name) => {if(!v)throw Error(name);};
  const one = s => document.querySelector(s);
  const all = s => [...document.querySelectorAll(s)].filter(n=>!n.closest('.modal-leave-active,.tab-leave-active'));
  const button = t => all('button').find(n=>n.textContent.trim()===t||n.getAttribute('aria-label')===t);
  const wait = async (fn,name) => {const end=performance.now()+24000;while(performance.now()<end){if(await fn())return;await sleep(40);}throw Error(name);};
  const emit = (stage,payload={}) => invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...payload}});
  const settle = async () => {
    await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));
    await wait(()=>!document.getAnimations().some(a=>Number.isFinite(a.effect?.getComputedTiming().endTime)&&(a.pending||a.playState==='running')),'finite animations settled');
  };
  async function click(node) {
    await wait(()=>node?.isConnected&&!node.disabled&&!node.closest('[inert]'),'enabled visible control');
    node.scrollIntoView({block:'nearest'});await settle();node.focus({preventScroll:true});
    const r=node.getBoundingClientRect(),hit=document.elementFromPoint(r.x+r.width/2,r.y+r.height/2);
    assert(hit===node||node.contains(hit),'pointer hit');node.click();await sleep(0);
  }
  async function fill(label,value) {
    await wait(()=>all(`[aria-label="${label}"]`).some(n=>!n.disabled),'editable '+label);
    const node=all(`[aria-label="${label}"]`).find(n=>!n.disabled);node.focus();node.value=value;node.dispatchEvent(new Event('input',{bubbles:true}));
  }
  let serial=0,count=0,last='boot',selectedMethod='';
  async function mode(value) {const id=++serial;await invoke('plugin:event|emit',{event:'device-login-fixture',payload:{id,mode:value}});await wait(()=>window.__deviceFixtureAck===id,'fixture acknowledgement');}
  async function theme(value) {localStorage.setItem('cumulus.appearance.v1',value);dispatchEvent(new StorageEvent('storage',{key:'cumulus.appearance.v1',newValue:value}));await wait(()=>document.documentElement.dataset.theme===value,'theme');await settle();}
  async function resize(width,height) {await emit('resize',{width,height,scale:devicePixelRatio});await wait(()=>innerWidth===width&&innerHeight===height,'native resize');await settle();}
  const phase = () => one('[data-device-phase]')?.dataset.devicePhase;
  async function stage(name,extra={}) {
    last=name;await settle();
    const d=one('dialog[open]'),r=d?.getBoundingClientRect();
    const overflow=document.documentElement.scrollWidth>innerWidth+1||!!d&&(r.top<0||r.bottom>innerHeight+1||d.scrollWidth>d.clientWidth+1);
    assert(!overflow,'viewport/dialog overflow');
    assert(!d||d.contains(document.activeElement),'focus within active dialog');
    const root=one('.device-qr-frame svg[role="img"]');
    if(phase()==='scanned'||['expired','rejected','blocked','recovery_required'].includes(phase()))assert(!root,'inactive QR withdrawn');
    const paths=one('.verification-methods');
    if(paths)assert(paths.getBoundingClientRect().height<=227,'bounded official method list');
    const device=one('.device-verification');
    if(device)assert(one('#login-title')?.textContent===(device.dataset.deviceKind==='scan'?'身份验证':'登录安全确认'),'official device title');
    assert(!device?.textContent.includes('fixture-device-token'),'no private polling token in UI');
    await emit(`ux_${String(++count).padStart(2,'0')}_${name}`,{...extra,width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,ratio:devicePixelRatio,
      screenX:window.screenX,screenY:window.screenY,phase:phase()??null,overflow:false,focusValid:!d||d.contains(document.activeElement),
      dialogs:d?[{open:true,opacity:getComputedStyle(d).opacity,background:getComputedStyle(d).backgroundColor,rect:r.toJSON()}]:[],
      focused:document.hasFocus(),focusedMethod:document.activeElement?.dataset?.verificationMethod??null,methodCount:paths?.querySelectorAll('button').length??null,qrVisible:!!root,codeLength:one('[aria-label="原设备确认码"]')?.value.length??0});
  }
  async function openChallenge() {
    await click(one('.account-button'));
    if(!one('.login-methods')){await wait(()=>one('.account-add'),'account chooser');await click(one('.account-add'));}
    await wait(()=>one('.login-methods'),'login dialog');await click(button('邮箱'));
    await fill('登录邮箱','device@example.test');await fill('登录密码','fixture-password');await click(one('.login-submit'));
    await wait(()=>button('获取验证方式'),'two factor challenge');await click(button('获取验证方式'));
    await wait(()=>one('[data-verification-method="DEVICE_SCAN"]'),'offered device methods');await settle();
  }
  async function choose(value) {selectedMethod=value;await click(one(`[data-verification-method="${value}"]`));if(value.startsWith('DEVICE'))await wait(()=>one('[data-device-phase]')&&!one('.login-flow[aria-busy="true"]'),'native device start');}
  async function back() {await click(button('选择其他验证方式'));await wait(()=>one('.verification-methods'),'verification method list');await settle();assert(document.activeElement.dataset.verificationMethod===selectedMethod,'restore selected method focus');}
  async function logged() {await wait(()=>!one('dialog')&&one('.account-button').textContent.includes('邮箱验证账号'),'authenticated account');await settle();}
  try {
    await wait(()=>one('.account-button')&&!one('.account-button').disabled&&one('.welcome'),'guest bootstrap');
    await theme('light');await resize(1120,760);await openChallenge();await stage('methods_light',{expectGuest:true});
    await choose('BINDING_CELLPHONE');assert(one('.verification-handoff').textContent.includes('请前往手机端操作'),'official PC handoff');await stage('binding_mobile_handoff',{expectGuest:true});await back();assert(document.activeElement.dataset.verificationMethod==='BINDING_CELLPHONE','return to method focus');
    await choose('WEICHAT_AUTH');await stage('wechat_boundary',{expectGuest:true});await back();
    await mode('normal');await choose('DEVICE_CONFIRM');await stage('confirm_waiting_light',{expectGuest:true});
    await theme('dark');await resize(760,540);await stage('confirm_waiting_small_dark',{expectGuest:true});
    await fill('原设备确认码','000000');await click(button('确认验证'));await wait(()=>one('.verification-error'),'incorrect confirmation code');assert(one('[aria-label="原设备确认码"]').value==='','submitted secret cleared');await stage('confirm_rejected_code',{expectGuest:true});
    await mode('expired');await wait(()=>phase()==='expired','confirmation expired');await stage('confirm_expired',{expectGuest:true});await back();
    await mode('normal');await theme('light');await choose('DEVICE_SCAN');await stage('scan_waiting_small_light',{expectGuest:true});assert(one('.device-qr-frame svg[role="img"]').getBoundingClientRect().width===122,'official device QR width');
    await mode('scanned');await wait(()=>phase()==='scanned','scan confirmed pending phone');await stage('scan_scanned',{expectGuest:true});
    await mode('expired');await wait(()=>phase()==='expired','scan expired');await stage('scan_expired',{expectGuest:true});await back();
    await mode('blocked');await choose('DEVICE_SCAN');await stage('scan_blocked',{expectGuest:true});await back();await stage('blocked_return',{expectGuest:true});
    await mode('rejected');await theme('dark');await resize(1120,760);await choose('DEVICE_SCAN');await wait(()=>phase()==='rejected','original device rejected');await stage('scan_rejected_dark',{expectGuest:true});await back();
    await mode('confirm_status');await choose('DEVICE_CONFIRM');await wait(()=>phase()==='code_required','status alone requires code');await stage('confirm_status_is_not_login',{expectGuest:true});
    await mode('business_false');await fill('原设备确认码','123456');await click(button('确认验证'));await wait(()=>one('.verification-error'),'business rejection inside HTTP200');await stage('confirm_business_rejected',{expectGuest:true});
    await mode('normal');await fill('原设备确认码','123456');await click(button('确认验证'));await logged();await stage('confirmed_account',{expectNickname:'邮箱验证账号',rememberEpoch:true});
    await openChallenge();await mode('start_lost');await choose('DEVICE_SCAN');await wait(()=>phase()==='recovery_required','unknown start');await stage('start_unknown',{expectNickname:'邮箱验证账号'});await back();await stage('start_unknown_stopped',{expectNickname:'邮箱验证账号'});
    await mode('poll_error');await choose('DEVICE_SCAN');await wait(()=>phase()==='error','poll read failed');await stage('poll_read_failed',{expectNickname:'邮箱验证账号'});
    await mode('normal');await click(button('重新检查验证状态'));await wait(()=>phase()==='waiting','explicit status retry');await stage('poll_read_recovered',{expectNickname:'邮箱验证账号'});
    await mode('cancel_lost');await back();await stage('cancel_unknown',{expectNickname:'邮箱验证账号'});
    await mode('verify_lost');await choose('DEVICE_CONFIRM');await fill('原设备确认码','123456');await click(button('确认验证'));await wait(()=>phase()==='recovery_required','unknown code result');await stage('confirmation_unknown',{expectNickname:'邮箱验证账号'});assert(!one('[aria-label="原设备确认码"]'),'unknown write has no resubmit form');
    await click(button('核对登录状态'));await logged();await stage('confirmation_reconciled',{expectNickname:'邮箱验证账号',rememberEpoch:true});
    await openChallenge();await theme('light');await resize(760,540);await mode('poll_hold');await choose('DEVICE_SCAN');await wait(()=>one('.device-verification[aria-busy="true"]'),'pending status request');await stage('scan_inflight',{expectNickname:'邮箱验证账号',preserveEpoch:true});
    await click(button('选择其他验证方式'));
    if(window.__deviceLegacy) await mode('release_poll');
    await wait(()=>one('.verification-methods'),'stopped device branch');await settle();assert(document.activeElement.dataset.verificationMethod==='DEVICE_SCAN','inflight stop restores selected method focus');await stage('stopped_inflight',{expectNickname:'邮箱验证账号',preserveEpoch:true});
    if(!window.__deviceLegacy) await mode('release_poll');
    await sleep(300);await stage('late_response_after_stop',{expectNickname:'邮箱验证账号',preserveEpoch:true});
    await mode('poll_hold');await choose('DEVICE_SCAN');await wait(()=>one('.device-verification[aria-busy="true"]'),'second pending status request');await stage('second_scan_inflight',{expectNickname:'邮箱验证账号',preserveEpoch:true});
    await click(button('关闭登录'));await wait(()=>!one('dialog'),'closed pending device login');await stage('closed_inflight',{expectNickname:'邮箱验证账号',preserveEpoch:true});
    await mode('release_poll');await sleep(300);await stage('late_authorization_ignored',{expectNickname:'邮箱验证账号',preserveEpoch:true});
    await emit('complete',{ok:true,stageCount:count,preserveEpoch:true});
  } catch(error) {await emit('failed',{ok:false,last,stageCount:count,error:error?.stack??String(error),phase:phase()??null,focusedMethod:document.activeElement?.dataset?.verificationMethod??null,body:document.body.innerText.slice(-1600)});}
})();
