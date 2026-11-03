// Real Tauri/WebView with synthetic QR and identity replies, no login service calls.
(async()=>{
  const one=s=>document.querySelector(s);
  const wait=async(f,m)=>{for(let i=0;i<100;i++){if(f())return;await new Promise(r=>setTimeout(r,60));}throw Error(m);};
  const assert=(v,m)=>{if(!v)throw Error(m);};
  const emit=(stage,data={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...data}});
  try{
    await wait(()=>one('.account-button')&&!one('.account-button').disabled&&one('#app').__vue_app__,'guest or fixture ready');
    const provides=one('#app').__vue_app__._context.provides;
    const core=provides[Reflect.ownKeys(provides).find(k=>typeof k==='symbol'&&k.description==='core-client')];

    const counts={begins:0,polls:0,reads:0,reconciles:0};
    const id='synthetic-recovery';
    core.beginQr=async()=>{counts.begins++;return {operationId:id,status:'waiting_scan',modules:[[true]],pollAfterMs:80};};
    core.pollQr=async op=>{assert(op===id,'poll current candidate');counts.polls++;throw {message:'synthetic lost account read',code:'network'};};
    core.cancelQr=async()=>{};
    core.qrIdentityPending=async op=>{assert(op===id,'read current candidate');if(++counts.reads===1)throw Error('unavailable');return true;};
    core.reconcileQr=async op=>{assert(op===id,'reconcile current candidate');if(++counts.reconciles===1)throw {message:'合成身份暂时未核对',code:'network'};};
    one('.account-button').click();
    if(one('.account-button').title!=='点击登录'){
      await wait(()=>one('.account-add')&&!one('.account-add').disabled,'add synthetic account');
      one('.account-add').click();
    }
    await wait(()=>one('[data-qr-recovery="read_failed"]'),'failed local status shows read-only recovery');
    const action=()=>one('[data-qr-recovery] button');
    assert(action().textContent.trim()==='核对二维码状态'&&counts.reconciles===0,'no auto identity read');
    action().click();
    await wait(()=>one('[data-qr-recovery="identity"]'),'native acknowledged approval renders identity recovery');
    await new Promise(r=>setTimeout(r,220));
    assert(counts.begins===1&&counts.polls===1&&counts.reads===2&&counts.reconciles===0,'no QR or identity replay');
    assert(action().textContent.trim()==='核对登录状态'&&!one('.qr-frame svg'),'no scannable ticket during identity recovery');
    action().click();
    await wait(()=>one('.login-status[role="alert"]')?.textContent.includes('合成身份暂时未核对'),'identity read failure keeps recovery');
    action().click();
    await wait(()=>!one('.login-dialog[open]'),'native identity success closes the flow');
    assert(counts.begins===1&&counts.polls===1&&counts.reconciles===2,'only explicit identity retries');
    await emit('complete',{ok:true,qrIdentityRecoveryVerified:true,injectedCoreClient:true,counts});
  }catch(e){await emit('failed',{message:String(e)});}
})();
