(async()=>{
 const sleep=ms=>new Promise(r=>setTimeout(r,ms)),el=s=>document.querySelector(s);
 const report=(stage,data={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'auth-proof',payload:{stage,...data}});
 const wait=async(fn,label)=>{const end=Date.now()+30000;while(Date.now()<end){if(fn())return;await sleep(100);}throw Error(label);};
 const text=value=>[...document.querySelectorAll('button')].find(n=>n.textContent.trim()===value);
 const click=async n=>{if(!n)throw Error('missing_control');await wait(()=>n.isConnected&&!n.disabled,'disabled_control');n.click();await sleep(150);};
 const fill=(label,value)=>{const n=el(`[aria-label="${label}"]`);n.value=value;n.dispatchEvent(new Event('input',{bubbles:true}));};
 const state=()=>el('.session-renewal')?.dataset.state;
 async function stage(name){await sleep(400);const d=el('dialog'),r=d?.getBoundingClientRect();if(d&&(d.scrollWidth>d.clientWidth+1||r.top<0||r.bottom>innerHeight+1))throw Error('layout_overflow');window.__authAck='';await report(name,{state:state(),overflow:false});await wait(()=>window.__authAck===name,'native_ack');await sleep(900);}
 async function open(){await click(el('.account-button'));await wait(()=>state(),'renewal_ready');}
 async function login(){await click(el('.account-add'));await wait(()=>el('.login-methods'),'login');await click(text('邮箱'));await wait(()=>el('[data-login-status]')?.dataset.loginStatus==='ready','email_ready');fill('登录邮箱','alice@example.test');fill('登录密码','fixture-password');await click(el('.login-submit'));await wait(()=>!el('dialog'),'login_done');await open();}
 try {
  await wait(()=>el('.account-button')?.textContent.includes('邮箱验证账号'),'restored');await open();
  await wait(()=>state()==='missing_credentials','missing_credentials');await stage('renewal_missing');
  await login();await wait(()=>['refreshed','not_due'].includes(state()),'automatic_refresh');await stage('renewal_success');
  await click(text('检查登录续期'));await wait(()=>state()==='not_due','daily_gate');await stage('renewal_daily');
  await login();await wait(()=>['recovery_required','reauthentication_required'].includes(state()),'unknown_rotation');await stage('renewal_unknown');
  await click(text('核对续期结果'));await wait(()=>state()==='reauthentication_required','reconcile_only');await stage('renewal_relogin');
  await report('complete',{ok:true,state:state()});
 }catch(e){await report('failed',{ok:false,error:e instanceof Error?e.message:'unknown',state:state(),message:el('.session-renewal')?.textContent});}
})();
