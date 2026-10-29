(async()=>{
  const one=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)],sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const assert=(ok,text)=>{if(!ok)throw Error(text);};
  const wait=async(test,label)=>{for(let i=0;i<200;i++){if(test())return;await sleep(100);}throw Error(label);};
  const emit=(stage,rest={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...rest}});
  const click=n=>{assert(n&&!n.disabled,'control unavailable');n.scrollIntoView({block:'nearest'});n.focus({preventScroll:true});n.click();};
  const section=()=>one('.audio-output'), select=()=>one('#audio-output-device'), refresh=()=>one('[aria-label="刷新音频输出设备"]');
  const status=()=>section()?.querySelector('.output-status')?.textContent??'';
  const ready=()=>section()?.getAttribute('aria-busy')==='false';
  const phase=window.__CUMULUS_OUTPUT_PHASE__;let stageCount=0;
  const player=()=>one('.playerbar');
  async function capture(name){
    section().scrollIntoView({block:'center'});await sleep(300);
    assert(document.documentElement.scrollWidth<=innerWidth,'horizontal overflow');
    const scroll=one('.settings-scroll');assert(scroll.scrollWidth<=scroll.clientWidth+1,'settings content overflow');
    const panel=section().getBoundingClientRect(),box=scroll.getBoundingClientRect();assert(panel.right<=box.right+1,'output panel overflow');
    assert(player().dataset.state==='paused'&&Math.abs(Number(player().dataset.position)-12.5)<.2,'settings changed paused playback');
    await emit(`ux_${String(++stageCount).padStart(2,'0')}_${name}`,{theme:document.documentElement.dataset.theme,width:innerWidth,height:innerHeight,focused:document.activeElement?.id||document.activeElement?.getAttribute('aria-label')||document.activeElement?.textContent,selected:select()?.value,status:status(),error:!!section().querySelector('.catalog-error'),state:player().dataset.state,position:Number(player().dataset.position)});await sleep(500);
  }
  async function theme(name){click(one(`input[name="settings-theme"][value="${name}"]`));await wait(()=>document.documentElement.dataset.theme===name,'theme');}
  async function read(){click(refresh());await sleep(120);await wait(ready,'output refreshed');}
  async function choose(id){const n=select();assert(n&&!n.disabled,'output selector unavailable');n.scrollIntoView({block:'nearest'});n.focus({preventScroll:true});n.value=id;n.dispatchEvent(new Event('change',{bubbles:true}));await wait(()=>ready()&&!select()?.disabled,'output saved');await sleep(150);assert(document.activeElement===select(),'output selection lost focus');}
  try{
    await wait(()=>player()?.dataset.state==='paused'&&one('[aria-label="打开设置"]')&&!one('[aria-label="打开设置"]').disabled,'app ready');
    click(one('[aria-label="打开设置"]'));await wait(()=>ready()&&select()&&!select().disabled,'output settings');
    if(phase==='initial'){
      await theme('light');assert(select().value==='auto','default output');await capture('default_light');
      await choose('fixture/a');await wait(()=>status().includes('已应用'),'selected output applied');await capture('selected_a');
      await emit('resize',{width:760,height:540});await wait(()=>innerWidth===760,'small window');await theme('dark');await capture('small_dark');
      await emit('remove-device');await read();await wait(()=>status().includes('回退'),'unplug fallback');assert(select().value==='fixture/a'&&select().selectedOptions[0].disabled,'preserve missing selection');await capture('missing_fallback');
      await emit('return-device');await read();assert(select().value==='fixture/a'&&status().includes('回退'),'replug unexpectedly switched output');await capture('returned_sticky');
      click(all('.audio-output button').find(n=>n.textContent.trim()==='重试所选输出'));await wait(()=>ready()&&status().includes('已应用'),'retry output');assert(document.activeElement===refresh(),'removed retry lost focus');await capture('retried');
      await emit('reject-write');const n=select();n.focus();n.value='fixture/b';n.dispatchEvent(new Event('change',{bubbles:true}));await wait(()=>ready()&&section().querySelector('.catalog-error'),'save error');assert(select().value==='fixture/a','failed write changed preference');assert(document.activeElement===refresh(),'save error lost recovery focus');await capture('save_failed');
      await emit('allow-write');await read();await capture('read_recovered');
      await emit('corrupt');await read();await wait(()=>section().querySelector('.settings-recovery'),'damage');assert(!select(),'damaged preference shown as valid');await capture('damaged');
      click(all('.audio-output button').find(n=>n.textContent.trim()==='恢复默认输出设置'));await wait(()=>ready()&&select()&&!select().disabled&&status().includes('已应用'),'repair and native application');assert(document.activeElement===refresh(),'repair lost focus');await capture('repaired');
      await emit('raw-damage');await read();assert(section().querySelector('.settings-recovery')&&!select(),'invalid JSON exposed as usable output');await capture('raw_json_damaged');
      click(all('.audio-output button').find(n=>n.textContent.trim()==='恢复默认输出设置'));await wait(()=>ready()&&select()&&!select().disabled&&status().includes('已应用'),'invalid JSON repair and native application');assert(document.activeElement===refresh(),'raw repair lost focus');assert(select().value==='auto','raw repair defaults');await capture('raw_json_repaired');
      await choose('fixture/b');await wait(()=>status().includes('已应用'),'final choice applied');await theme('light');await capture('saved_b');
    }else{assert(select().value==='fixture/b','output selection did not survive process restart');await wait(()=>status().includes('已应用'),'reopened output applied');await capture('reopened_b');}
    await emit('complete',{ok:true,phase,stageCount,realMusicRequests:0,decoder:'libmpv',audioOutput:'null',deviceCatalog:'synthetic',physicalEndpointVerified:false});
  }catch(error){await emit('failed',{ok:false,phase,stageCount,error:String(error),status:status(),width:innerWidth,overflow:all('.settings-scroll, .settings-scroll *').filter(n=>n.scrollWidth>n.clientWidth+1||n.matches('.output-selection *')).map(n=>({tag:n.tagName,class:n.className,scroll:n.scrollWidth,client:n.clientWidth,width:n.getBoundingClientRect().width,left:n.getBoundingClientRect().left,right:n.getBoundingClientRect().right,cssWidth:getComputedStyle(n).width,minWidth:getComputedStyle(n).minWidth,maxWidth:getComputedStyle(n).maxWidth,text:n.textContent.slice(0,100)}))});}
})();
