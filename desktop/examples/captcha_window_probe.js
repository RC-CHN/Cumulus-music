(async () => {
  const sleep = ms => new Promise(r => setTimeout(r, ms));
  const report = (stage, data={}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'captcha-proof',payload:{stage,...data}});
  const violations=[];
  const resources=()=>performance.getEntriesByType('resource').map(r=>{try{const u=new URL(r.name);return {url:u.origin+u.pathname,duration:r.duration};}catch{return {url:'local'};}});
  document.addEventListener('securitypolicyviolation', e => { let host=''; try {host=new URL(e.blockedURI).host;} catch {} violations.push({directive:e.effectiveDirective,host,kind:['inline','eval','wasm-eval'].includes(e.blockedURI)?e.blockedURI:'url'}); });
  try {
    let denied=false;
    try { await window.__TAURI_INTERNALS__.invoke('get_app_info'); } catch { denied=true; }
    if(!denied)throw Error('app_ipc_not_denied');
    for(let i=0;i<300 && document.body.dataset.phase!=='ready';i++)await sleep(100);
    if(document.body.dataset.phase!=='ready')throw Error('sdk_not_ready');
    const visibleWidget=()=>[...document.querySelectorAll('[class*="yidun_popup"]')].some(n=>{const r=n.getBoundingClientRect(),s=getComputedStyle(n);return r.width>200&&r.height>80&&s.display!=='none'&&s.visibility!=='hidden';});
    for(let i=0;i<150&&!visibleWidget();i++)await sleep(100);
    if(!visibleWidget())throw Error('widget_not_visible');
    const imageLoaded=()=>[...document.querySelectorAll('img')].some(n=>n.src.includes('necaptcha.nosdn.127.net/')&&n.naturalWidth>100&&n.getBoundingClientRect().width>100);
    for(let i=0;i<150&&!imageLoaded();i++)await sleep(100);
    if(!imageLoaded())throw Error('puzzle_image_not_loaded');
    await sleep(500);
    const loadedResources=resources();
    window.__captchaAck='';
    const stage=window.__captchaSecond?'captcha_window_again':'captcha_window';
    await report(stage,{ipcDenied:denied,phase:document.body.dataset.phase,officialWidget:visibleWidget(),puzzleImageLoaded:imageLoaded(),resources:loadedResources,violations});
    for(let i=0;i<50&&window.__captchaAck!==stage;i++)await sleep(100);
    await sleep(1400);
    if(!window.__captchaSecond)document.getElementById('cancel').click();
  } catch(e) { await report('captcha_diagnostic',{phase:document.body.dataset.phase,bootstrap:document.body.dataset.bootstrap??null,location:location.href,resources:resources(),text:document.body.textContent.slice(0,400),violations}); await sleep(1500); await report('failed',{ok:false,error:e.message,violations}); }
})();
