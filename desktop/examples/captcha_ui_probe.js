(async () => {
  const sleep = ms => new Promise(r => setTimeout(r, ms));
  const el = s => document.querySelector(s);
  const button = text => [...document.querySelectorAll('button')].find(n => n.textContent.trim() === text);
  const wait = async (fn, name) => { for (let i=0;i<450;i++) { if(fn())return; await sleep(100); } throw Error(name); };
  const report = (stage, data = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {event:'captcha-proof',payload:{stage,...data}});
  const stage = async name => { await sleep(400); window.__captchaAck=''; await report(name); await wait(()=>window.__captchaAck===name,'ack'); await sleep(1000); };
  const fill = (label, value) => { const n=el(`[aria-label="${label}"]`); n.value=value; n.dispatchEvent(new Event('input',{bubbles:true})); };
  try {
    await wait(()=>el('.account-button')&&!el('.account-button').disabled,'boot');
    el('.account-button').click();
    await wait(()=>el('.login-methods'),'login');
    button('邮箱').click();
    await wait(()=>el('[data-login-status]')?.dataset.loginStatus==='ready','email');
    fill('登录邮箱','blocked@example.test'); fill('登录密码','fixture-password'); el('.login-submit').click();
    await wait(()=>button('打开安全验证')&&!button('打开安全验证').disabled,'challenge');
    await stage('captcha_form'); button('打开安全验证').click();
    await wait(()=>window.__captchaAck==='captcha_window','widget');
    await wait(()=>button('打开安全验证')&&!button('打开安全验证').disabled,'cancel_return');
    if(el('[data-login-status]')?.dataset.loginStatus!=='captcha')throw Error('challenge_lost');
    await stage('captcha_return');
    button('打开安全验证').click();
    await wait(()=>window.__captchaAck==='captcha_window_again','reopened_widget');
    await sleep(1500);
    el('[aria-label="关闭登录"]').click();
    await sleep(500);
    await stage('captcha_closed');
    await report('complete',{ok:true,widgetCancelled:true});
  } catch (e) { await report('failed',{ok:false,error:e.message}); }
})();
