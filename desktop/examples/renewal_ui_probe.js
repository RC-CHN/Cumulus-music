(async()=>{
 const sleep=ms=>new Promise(r=>setTimeout(r,ms)),el=s=>document.querySelector(s);
 const report=(stage,data={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'auth-proof',payload:{stage,...data}});
 const wait=async(fn,label)=>{const end=Date.now()+30000;while(Date.now()<end){if(fn())return;await sleep(100);}throw Error(label);};
 const text=value=>[...document.querySelectorAll('button')].find(n=>n.textContent.trim()===value);
 const click=async n=>{if(!n)throw Error('missing_control');await wait(()=>n.isConnected&&!n.disabled,'disabled_control');n.click();await sleep(150);};
 const fill=(label,value)=>{const n=el(`[aria-label="${label}"]`);n.value=value;n.dispatchEvent(new Event('input',{bubbles:true}));};
 const state=()=>el('.session-renewal')?.dataset.state;
 async function clock(action){window.__renewalClockAck='';await window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'renewal-clock-proof',payload:action});await wait(()=>window.__renewalClockAck===action,'native_clock_ack');}
 async function stage(name){await sleep(400);const d=el('dialog'),r=d?.getBoundingClientRect();if(d&&(d.scrollWidth>d.clientWidth+1||r.top<0||r.bottom>innerHeight+1))throw Error('layout_overflow');window.__authAck='';await report(name,{state:state(),overflow:false});await wait(()=>window.__authAck===name,'native_ack');await sleep(900);}
 async function open(){await click(el('.account-button'));await wait(()=>state(),'renewal_ready');}
 async function reopen(){await click(el('.account-dialog .dialog-close'));await wait(()=>!el('dialog'),'chooser_closed');await open();}
 async function login(){await click(el('.account-add'));await wait(()=>el('.login-methods'),'login');await click(text('邮箱'));await wait(()=>el('[data-login-status]')?.dataset.loginStatus==='ready','email_ready');fill('登录邮箱','alice@example.test');fill('登录密码','fixture-password');await click(el('[aria-label="同意登录协议"]'));await click(el('.login-submit'));await wait(()=>!el('dialog'),'login_done');await open();}
 try {
  await wait(()=>el('.account-button')?.textContent.includes('邮箱验证账号'),'restored');await open();
  await wait(()=>state()==='missing_credentials','missing_credentials');await stage('renewal_missing');
  await login();await wait(()=>state()==='not_due','fresh_login_daily_gate');await stage('renewal_login');
  await clock('advance');await reopen();await wait(()=>state()==='ready','read_due_after_fresh_login');await stage('renewal_due');
  await click(text('检查登录续期'));await wait(()=>state()==='recovery_required'&&text('核对续期结果'),'paired_identity_pending');
  if(text('重新登录')||!el('.session-renewal').textContent.includes('需要核对账号身份')) throw Error('paired_recovery_controls');
  await stage('renewal_identity_pending');
  await click(text('核对续期结果'));await wait(()=>state()==='refreshed','same_identity_confirmed');await stage('renewal_success');
  await click(text('检查登录续期'));await wait(()=>state()==='not_due','daily_gate');await stage('renewal_daily');
  await clock('advance_again');await reopen();await wait(()=>state()==='ready','read_due_after_rotation');await stage('renewal_due_again');
  await clock('reset');await login();await wait(()=>state()==='not_due','second_fresh_login_gate');await stage('renewal_second_login');
  await clock('advance');const check=text('检查登录续期');check.focus();await click(check);
  await wait(()=>state()==='unconfirmed'&&document.activeElement===text('重新读取续期状态'),'unknown_rotation_focus');
  if(text('核对续期结果')||!el('.session-renewal').textContent.includes('当前仍为登录状态')) throw Error('unknown_rotation_controls');
  await stage('renewal_unconfirmed');
  await click(text('重新读取续期状态'));await wait(()=>state()==='unconfirmed','local_read_only');
  await click(text('重新登录'));await wait(()=>el('.qr-frame')&&!el('.account-dialog'),'optional_login_handoff');
  await click(el('[aria-label="关闭登录"]'));await wait(()=>!el('dialog'),'optional_login_cancelled');
  await open();await wait(()=>state()==='unconfirmed','optional_login_retains_unconfirmed');
  await stage('renewal_relogin_cancelled');
  await report('complete',{ok:true,state:state()});
 }catch(e){await report('failed',{ok:false,error:e instanceof Error?e.message:'unknown',state:state(),message:el('.session-renewal')?.textContent});}
})();
