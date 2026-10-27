(async () => {
  const invoke=(name,args={})=>window.__TAURI_INTERNALS__.invoke(name,args);
  const report=(stage,data={})=>invoke('plugin:event|emit',{event:'watchman-events-proof',payload:{stage,...data}});
  const wait=ms=>new Promise(r=>setTimeout(r,ms));
  await wait(800);
  const pane=document.createElement('section');pane.id='watchman-proof-pane';
  pane.style.cssText='position:fixed;inset:0;z-index:999999;background:#fff;color:#222;padding:40px;font:18px sans-serif';
  pane.innerHTML='<h1>本地事件采集验证</h1><p id="watchman-proof-status">等待原生采集器</p><button id="watchman-proof-button" style="position:absolute;left:50px;top:140px;width:300px;height:50px">点击验证</button><input id="watchman-proof-input" type="password" autocomplete="off" style="position:absolute;left:50px;top:220px;width:300px;height:50px"><button id="watchman-proof-finish" style="position:absolute;left:50px;top:310px;width:300px;height:50px">完成并检查</button>';
  document.body.append(pane);
  const status=document.querySelector('#watchman-proof-status');
  window.__watchmanEventsProof={
    begin(){status.textContent='原生采集已开始，等待实际鼠标和键盘输入';},
    async restart(oldGeneration){
      try {
        let rejected=false;try {await invoke('watchman_observation',{frame:{generation:oldGeneration,sequence:999,observed_ms:Date.now(),payload:{kind:'event',sample:{dom_type:'click',target_id:'stale'}}}});}catch{rejected=true;}
        document.querySelector('#watchman-proof-button').click();
        await window.__cumulusWatchmanEvents.flushed();
        status.textContent='原生事件编码、重新绑定和旧会话拒绝已检查';
        await report('restarted',{staleRejected:rejected});
      }catch {await report('failed',{reason:'restart'});}
    }
  };
  document.querySelector('#watchman-proof-finish').addEventListener('click',async()=>{
    try {
      const input=document.querySelector('#watchman-proof-input'),length=input.value.length;
      input.value=''; // Only the count is reported; the typed text never crosses IPC.
      await window.__cumulusWatchmanEvents.flushed();
      await report('captured',{typedLength:length});
    }catch {await report('failed',{reason:'capture'});}
  });
  await report('ready',{cors:'withCredentials' in new XMLHttpRequest(),width:innerWidth,height:innerHeight,screenX,screenY,outerWidth,outerHeight});
})()
