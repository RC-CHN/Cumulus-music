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
    await wait(()=>all('.device-verification,.device-loading,.device-confirm-content,.device-qr-mask,.verification-choice,.verification-handoff').every(n=>Number(getComputedStyle(n).opacity)>=0.999),'device steps fully visible');
    await sleep(100);
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
  const pins=()=>all('.login-pin input');
  const pinLength=()=>pins().reduce((n,input)=>n+input.value.length,0);
  async function pinDigit(index,value) {await fill(`确认码第 ${index+1} 位`,value);await sleep(0);}
  async function pastePin(value) {
    const input=pins()[0];await wait(()=>input&&!input.disabled,'editable PIN');input.focus();
    const event=new Event('paste',{bubbles:true,cancelable:true});Object.defineProperty(event,'clipboardData',{value:{getData:()=>value}});input.dispatchEvent(event);await sleep(0);
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
    if(one('.login-pin')||one('.device-qr-frame'))assert(d.scrollHeight<=d.clientHeight+1,'standard device step fits short window without vertical scrolling');
    assert(!d||d.contains(document.activeElement),'focus within active dialog');
    const root=one('.device-qr-frame svg[role="img"]');
    if(phase()==='scanned'||['expired','rejected','blocked','recovery_required'].includes(phase()))assert(!root,'inactive QR withdrawn');
    const paths=one('.verification-methods');
    if(paths)assert(paths.getBoundingClientRect().height<=227,'bounded official method list');
    const device=one('.device-verification'),loading=one('.device-loading-text');
    if(device)assert(one('#login-title')?.textContent===(device.dataset.deviceKind==='scan'?'身份验证':'登录安全确认'),'official device title');
    assert(!device?.textContent.includes('fixture-device-token'),'no private polling token in UI');
    await emit(`ux_${String(++count).padStart(2,'0')}_${name}`,{...extra,width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,ratio:devicePixelRatio,
      screenX:window.screenX,screenY:window.screenY,phase:phase()??null,loadingText:loading?{rect:loading.getBoundingClientRect().toJSON(),color:getComputedStyle(loading).color,background:getComputedStyle(d).backgroundColor}:null,stepOpacity:device?getComputedStyle(device).opacity:null,loadingOpacity:one('.device-loading')?getComputedStyle(one('.device-loading')).opacity:null,overflow:false,focusValid:!d||d.contains(document.activeElement),
      dialogs:d?[{open:true,opacity:getComputedStyle(d).opacity,background:getComputedStyle(d).backgroundColor,rect:r.toJSON()}]:[],
      focused:document.hasFocus(),focusedMethod:document.activeElement?.dataset?.verificationMethod??null,methodCount:paths?.querySelectorAll('button').length??null,qrVisible:!!root,codeLength:pinLength(),pinSlots:pins().length});
  }
  async function openChallenge() {
    await click(one('.account-button'));
    if(!one('.login-methods')){await wait(()=>one('.account-add'),'account chooser');await click(one('.account-add'));}
    await enterDeviceChallenge();
  }
  async function enterDeviceChallenge() {
    await wait(()=>one('.login-methods'),'login dialog');await click(button('邮箱'));
    await fill('登录邮箱','device@example.test');await fill('登录密码','fixture-password');
    const consent=all('[aria-label="同意登录协议"]').find(n=>!n.disabled);
    if(!consent.checked)await click(consent);
    await click(one('.login-submit'));
    await wait(()=>button('获取验证方式'),'two factor challenge');await click(button('获取验证方式'));
    await wait(()=>one('[data-verification-method="DEVICE_SCAN"]'),'offered device methods');await settle();
  }
  async function choose(value) {selectedMethod=value;await click(one(`[data-verification-method="${value}"]`));if(value.startsWith('DEVICE'))await wait(()=>one('[data-device-phase]')&&!one('.login-flow[aria-busy="true"]'),'native device start');}
  async function back() {await click(button(selectedMethod==='SECURITY_EMAIL'?'收不到邮箱验证码？尝试其他验证方式':'选择其他验证方式'));await wait(()=>one('.verification-methods'),'verification method list');await settle();assert(document.activeElement.dataset.verificationMethod===selectedMethod,'restore selected method focus');}
  async function logged() {await wait(()=>!one('dialog')&&one('.account-button').textContent.includes('邮箱验证账号'),'authenticated account');await settle();}
  try {
    await wait(()=>one('.account-button')&&!one('.account-button').disabled&&one('.welcome'),'guest bootstrap');
    await theme('light');await resize(1120,760);await openChallenge();
    assert(one('.verification-identity-title')?.textContent==='用户昵称'&&one('.verification-identity-name')?.textContent.trim()==='设备验证账号','method list shows candidate identity');
    await stage('methods_light',{expectGuest:true});
    await theme('dark');await resize(760,540);await choose('BINDING_CELLPHONE');
    const handoff=one('.verification-handoff'),bindingIdentity=one('.binding-handoff-identity');
    assert(handoff?.textContent.includes('请前往手机端操作')&&!handoff.querySelector('button,a,input'),'official mobile-only handoff');
    assert(bindingIdentity?.querySelector('.verification-identity-title')?.textContent==='用户昵称'&&bindingIdentity.querySelector('.verification-identity-name')?.textContent.trim()==='设备验证账号','handoff preserves candidate identity');
    assert(one('#login-title')?.textContent==='登录安全验证'&&document.activeElement===one('.verification-back'),'handoff title and return focus');
    const handoffDialog=one('dialog[open]');assert(handoffDialog.scrollHeight<=handoffDialog.clientHeight+1,'handoff fits short window');
    await stage('binding_mobile_handoff',{expectGuest:true});await back();assert(document.activeElement.dataset.verificationMethod==='BINDING_CELLPHONE','return to method focus');
    await click(button('选择其他登录方式'));
    await wait(()=>one('#login-title')?.textContent==='扫码登录'&&one('.login-methods')&&!all('.verification-methods').length,'other login mode returns to normal login');
    await enterDeviceChallenge();
    await theme('light');await resize(1120,760);
    await choose('WEICHAT_AUTH');await stage('wechat_boundary',{expectGuest:true});await back();
    await mode('normal');await choose('DEVICE_CONFIRM');await stage('confirm_waiting_light',{expectGuest:true});
    await theme('dark');await resize(760,540);await stage('confirm_waiting_small_dark',{expectGuest:true});
    await pinDigit(0,'1');assert(document.activeElement===pins()[1],'typing advances PIN focus');await pinDigit(1,'2');
    pins()[2].dispatchEvent(new KeyboardEvent('keydown',{key:'Backspace',bubbles:true,cancelable:true}));await sleep(0);assert(pinLength()===1&&document.activeElement===pins()[1],'backspace deletes previous empty slot');
    pins()[1].dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowLeft',bubbles:true,cancelable:true}));assert(document.activeElement===pins()[0],'PIN arrow navigation');
    await pinDigit(0,'x');assert(pinLength()===0,'non-digit rejected');for(let i=0;i<3;i++)await pinDigit(i,'0');await pastePin('12x456');assert(pins().map(n=>n.value).join('')==='000','invalid paste does not submit or replace');
    await pastePin('12');assert(pinLength()===3&&pins()[2].value==='0'&&document.activeElement===pins()[2],'short paste replaces only prefix and keeps remaining digits');
    await stage('pin_partial_keyboard',{expectGuest:true});
    await mode('poll_slow');await wait(()=>one('.device-verification[aria-busy="true"]'),'background read pending');await pastePin('000000');assert(pinLength()===0&&pins().every(n=>n.disabled),'completed code cleared while waiting for read');await stage('pin_waits_for_read',{expectGuest:true});
    await wait(()=>one('.verification-error')?.textContent.trim(),'incorrect confirmation code');assert(pinLength()===0,'submitted secret cleared');await stage('confirm_rejected_code',{expectGuest:true});
    const retiredPin=pins()[0];await pinDigit(0,'2');assert(!one('.verification-error')?.textContent.trim(),'editing clears wrong-code message');await stage('pin_error_edit',{expectGuest:true});
    await mode('expired');await wait(()=>phase()==='expired','confirmation expired');await stage('confirm_expired',{expectGuest:true});assert(retiredPin.value==='','expired PIN DOM cleared');await mode('normal');await click(button('重新发送'));await wait(()=>phase()==='waiting'&&!one('.login-flow[aria-busy="true"]'),'confirmation resent');assert(pinLength()===0&&document.activeElement===pins()[0],'fresh confirmation starts empty and focused');await stage('confirm_resent',{expectGuest:true});await back();
    await mode('normal');await theme('light');await choose('DEVICE_SCAN');await stage('scan_waiting_small_light',{expectGuest:true});assert(one('.device-qr-frame svg[role="img"]').getBoundingClientRect().width===122,'official device QR width');
    const oldQr=one('.device-qr-frame svg path').getAttribute('d');
    await mode('scanned');await wait(()=>phase()==='scanned','scan confirmed pending phone');await stage('scan_scanned',{expectGuest:true});
    await mode('expired');await wait(()=>phase()==='expired','scan expired');await stage('scan_expired',{expectGuest:true});await mode('normal');await click(button('刷新原设备二维码'));await wait(()=>phase()==='waiting'&&one('.device-qr-frame svg[role="img"]'),'inline QR refresh');assert(one('.device-qr-frame svg path').getAttribute('d')!==oldQr,'refreshed QR belongs to new application');await stage('scan_refreshed',{expectGuest:true});await back();
    await mode('blocked');selectedMethod='DEVICE_SCAN';await click(one('[data-verification-method="DEVICE_SCAN"]'));await wait(()=>one('.verification-methods')&&one('.verification-blocked'),'blocked device returns to methods with feedback');assert(document.activeElement.dataset.verificationMethod==='DEVICE_SCAN','blocked return restores method focus');await stage('scan_blocked',{expectGuest:true});await choose('SECURITY_EMAIL');await wait(()=>one('[data-email-phase="code_required"]')&&one('[aria-label="安全邮箱验证码"]')?.querySelectorAll('input').length===6&&!one('.login-flow[aria-busy="true"]'),'blocked branch opens native email verification with six PIN slots');await stage('blocked_alternative',{expectGuest:true});await back();
    await mode('rejected');await theme('dark');await resize(1120,760);await choose('DEVICE_SCAN');await wait(()=>phase()==='rejected','original device rejected');assert(button('刷新原设备二维码')&&!document.body.innerText.includes('错误次数'),'scan rejection uses QR expiry instead of PIN error');await stage('scan_rejected_dark',{expectGuest:true});await back();
    await mode('confirm_status');await choose('DEVICE_CONFIRM');await wait(()=>phase()==='code_required','status alone requires code');await stage('confirm_status_is_not_login',{expectGuest:true});
    await mode('business_false');await pastePin('123456');await wait(()=>one('.verification-error')?.textContent.trim(),'business rejection inside HTTP200');await stage('confirm_business_rejected',{expectGuest:true});
    await mode('normal');for(let i=3;i<6;i++)await pinDigit(i,String(i+1));await pastePin('123');
    assert(pinLength()===0,'short paste completing the current code scrubs the fields');
    await logged();await stage('confirmed_account',{expectNickname:'邮箱验证账号',rememberEpoch:true});
    await openChallenge();await mode('start_lost');await choose('DEVICE_SCAN');await wait(()=>phase()==='recovery_required','unknown start');await stage('start_unknown',{expectNickname:'邮箱验证账号'});await back();await stage('start_unknown_stopped',{expectNickname:'邮箱验证账号'});
    await mode('poll_error');await choose('DEVICE_SCAN');await wait(()=>phase()==='error','poll read failed');await stage('poll_read_failed',{expectNickname:'邮箱验证账号'});
    await mode('normal');await click(button('重新检查验证状态'));await wait(()=>phase()==='waiting','explicit status retry');await stage('poll_read_recovered',{expectNickname:'邮箱验证账号'});
    await mode('cancel_lost');await back();await stage('cancel_unknown',{expectNickname:'邮箱验证账号'});
    await mode('verify_lost');await choose('DEVICE_CONFIRM');await pastePin('123456');await wait(()=>phase()==='recovery_required','unknown code result');await stage('confirmation_unknown',{expectNickname:'邮箱验证账号'});assert(!one('.login-pin'),'unknown write has no resubmit form');
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
    await openChallenge();await mode('normal');await choose('DEVICE_CONFIRM');await pinDigit(0,'1');await pinDigit(1,'2');await pinDigit(2,'3');const leavingPins=pins();await stage('partial_pin_before_close',{expectNickname:'邮箱验证账号',preserveEpoch:true});
    await click(button('关闭登录'));assert(leavingPins.every(n=>n.value===''),'all six retained PIN DOM fields cleared on close');await wait(()=>!one('dialog'),'closed partial PIN');await stage('closed_partial_pin',{expectNickname:'邮箱验证账号',preserveEpoch:true});
    await openChallenge();await mode('start_hold');selectedMethod='DEVICE_SCAN';await click(one('[data-verification-method="DEVICE_SCAN"]'));
    await wait(()=>one('.device-loading')&&one('.login-flow[aria-busy="true"]'),'visible pending application');await stage('application_inflight',{expectNickname:'邮箱验证账号',preserveEpoch:true});
    await click(button('关闭登录'));await wait(()=>!one('dialog'),'close pending application');await stage('closed_application_inflight',{expectNickname:'邮箱验证账号',preserveEpoch:true});
    await mode('release_start');await sleep(300);await stage('late_application_ignored',{expectNickname:'邮箱验证账号',preserveEpoch:true});
    await emit('complete',{ok:true,stageCount:count,preserveEpoch:true});
  } catch(error) {await emit('failed',{ok:false,last,stageCount:count,error:error?.stack??String(error),phase:phase()??null,focusedMethod:document.activeElement?.dataset?.verificationMethod??null,body:document.body.innerText.slice(-1600)});}
})();
