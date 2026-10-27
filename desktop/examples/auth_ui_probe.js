(async()=>{
 const sleep=ms=>new Promise(r=>setTimeout(r,ms)),el=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)];
 const report=(stage,data={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'auth-proof',payload:{stage,...data}});
 const wait=async(fn,label,ms=30000)=>{const end=Date.now()+ms;while(Date.now()<end){if(await fn())return;await sleep(100);}throw Error(label);};
 const click=async node=>{if(!node)throw Error('missing_control');await wait(()=>node.isConnected&&!node.disabled,'disabled_control',70000);node.scrollIntoView({block:'nearest'});node.click();await sleep(100);};
 const text=(value,root=document)=>[...root.querySelectorAll('button')].find(n=>n.textContent.trim()===value);
 const fill=(label,value)=>{const n=el(`[aria-label="${label}"]`);if(!n)throw Error(`missing_${label}`);n.value=value;n.dispatchEvent(new Event('input',{bubbles:true}));};
 const status=()=>el('[data-login-status]')?.dataset.loginStatus;
 const ready=()=>el('.account-button')&&!el('.account-button').disabled;
 const logged=async name=>wait(()=>ready()&&el('.account-button').textContent.includes(name)&&!el('dialog'),'account_'+name);
 async function stage(name,expected={}){await sleep(400);const d=el('dialog'),r=d?.getBoundingClientRect();const overflow=document.documentElement.scrollWidth>innerWidth||d&&(d.scrollWidth>d.clientWidth+1||r.top<0||r.bottom>innerHeight+1);if(overflow)throw Error('auth_layout_overflow');window.__authAck='';await report(name,{...expected,overflow:false,width:innerWidth,height:innerHeight,dialog:d?.getAttribute('aria-label')??d?.getAttribute('aria-labelledby')??null});await wait(()=>window.__authAck===name,'native_ack');await sleep(900);}
 async function chooser(){await click(el('.account-button'));await wait(()=>el('.account-add')&&!el('.account-dialog [role="status"]'),'account_list');}
 async function add(){await chooser();await click(el('.account-add'));await wait(()=>el('.login-methods'),'login_dialog');}
 async function email(address,password='fixture-password'){await click(text('邮箱',el('.login-methods')));await wait(()=>status()==='ready','email_ready');fill('登录邮箱',address);fill('登录密码',password);await click(el('.login-submit'));}
 async function choose(name){const row=all('.saved-account').find(n=>n.textContent.includes(name));await click(row);}
 let step='boot';
 try {
  await wait(()=>ready()&&el('.welcome'),'guest_bootstrap');
  if(window.__authProof.reopen){
   await stage('auth_reopen_guest',{expectGuest:true});await click(el('.account-button'));await wait(()=>el('.login-accounts'),'login');await click(el('.login-accounts'));await wait(()=>all('.saved-account').length===4,'saved_accounts_restored');await stage('auth_reopen_accounts',{expectGuest:true});await choose('短信验证账号');await logged('短信验证账号');await stage('auth_reopen_selected',{expectNickname:'短信验证账号'});
  }else{
   await click(el('.account-button'));await wait(()=>el('.qr-frame svg'),'qr');await stage('auth_qr',{expectGuest:true});
   step='email_rejected';await email('alice@example.test','wrong-password');await wait(()=>status()==='rejected','wrong_password');await stage('auth_rejected',{expectGuest:true});
   fill('登录密码','fixture-password');await click(el('.login-submit'));await logged('邮箱验证账号');await stage('auth_email',{expectNickname:'邮箱验证账号'});
   step='two_factor';await add();await email('verify@example.test');await wait(()=>status()==='two_factor','two_factor');await click(text('获取验证方式'));await wait(()=>el('[aria-label="安全验证方式"]'),'verification_methods');await click(text('发送验证码'));await wait(()=>!el('[aria-label="安全验证码"]').disabled,'verification_sent');await stage('auth_two_factor',{expectNickname:'邮箱验证账号'});fill('安全验证码','123456');await click(text('确认验证'));await logged('二次验证账号');
   step='switch';await chooser();await stage('auth_accounts',{expectNickname:'二次验证账号'});await choose('邮箱验证账号');await logged('邮箱验证账号');await stage('auth_switched',{expectNickname:'邮箱验证账号'});
   step='question';await add();await email('question@example.test');await wait(()=>status()==='security_question','question_challenge');await click(text('读取安全问题'));await wait(()=>el('.login-questions'),'question');await stage('auth_question',{expectNickname:'邮箱验证账号'});await click(el('input[name="login-answer"][value="设备甲"]'));await click(text('确认答案'));await logged('安全问题账号');
   step='captcha_boundary';await add();await email('blocked@example.test');await wait(()=>status()==='captcha','captcha_boundary');await stage('auth_captcha_boundary',{expectNickname:'安全问题账号'});await click(el('[aria-label="关闭登录"]'));await logged('安全问题账号');
   step='logout';await chooser();await choose('邮箱验证账号');await logged('邮箱验证账号');await chooser();await click(text('退出登录'));await stage('auth_logout_confirm',{expectNickname:'邮箱验证账号'});await click(text('确认退出'));await wait(()=>ready()&&el('.welcome')&&!el('dialog'),'logout');await stage('auth_logged_out',{expectGuest:true});
   step='expired_saved';await click(el('.account-button'));await click(el('.login-accounts'));await wait(()=>all('.saved-account').length===3,'saved_after_logout');await choose('邮箱验证账号');await wait(()=>el('.account-dialog [role="alert"]'),'expired_saved_error');await stage('auth_saved_expired',{expectGuest:true});await choose('二次验证账号');await logged('二次验证账号');
   step='sms';await add();await click(text('手机',el('.login-methods')));await wait(()=>status()==='ready','sms_ready');fill('登录手机号','13800138000');await wait(()=>text('获取验证码')&&!text('获取验证码').disabled,'shared_cooldown',70000);await click(text('获取验证码'));await wait(()=>status()==='code_sent','sms_sent');await stage('auth_sms',{expectNickname:'二次验证账号'});fill('短信验证码','123456');await click(el('.login-options input[type="checkbox"]'));await click(el('.login-submit'));await logged('短信验证账号');await stage('auth_sms_done',{expectNickname:'短信验证账号'});
  }
  await report('complete',{ok:true,reopened:window.__authProof.reopen,expectNickname:'短信验证账号',networkRequests:0,realAccountMutations:0});
 }catch(error){await report('failed',{ok:false,step,error:error instanceof Error?error.message:'unknown',status:status()??null,dialogError:el('.login-status')?.textContent??null});}
})();
