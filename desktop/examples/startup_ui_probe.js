(async()=>{
  const one=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)];
  const sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const assert=(ok,text)=>{if(!ok)throw Error(text);};
  const emit=(stage,rest={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...rest}});
  const wait=async(test,label)=>{for(let i=0;i<200;i++){if(test())return;await sleep(100);}throw Error(label);};
  const click=n=>{assert(n&&!n.disabled,'control unavailable');n.scrollIntoView({block:'nearest'});n.focus({preventScroll:true});n.click();};
  const field=key=>one(`input[name="startup-${key}"]`);
  const state=()=>one('.playerbar')?.dataset.state;
  const position=()=>Number(one('.playerbar')?.dataset.position??0);
  const ready=()=>one('.playback-startup')?.getAttribute('aria-busy')==='false';
  const phase=window.__CUMULUS_STARTUP_PHASE__;
  const resumed=phase==='autoplay-resume'&&sessionStorage.getItem('startup-reload-stage');
  let stageCount=resumed?Number(resumed):0;
  async function capture(name){
    await sleep(250);assert(document.documentElement.scrollWidth<=innerWidth,'horizontal overflow');
    const panel=one('.playback-startup');
    if(panel){const bounds=one('.settings-scroll').getBoundingClientRect();assert(bounds.bottom<=one('.playerbar').getBoundingClientRect().top+1,'settings overlap player');}
    await emit(`ux_${String(++stageCount).padStart(2,'0')}_${name}`,{state:state(),position:position(),theme:document.documentElement.dataset.theme,width:innerWidth,height:innerHeight,autoPlay:field('auto-play')?.checked,rememberPosition:field('remember-position')?.checked,focused:document.activeElement?.getAttribute('aria-label'),damaged:!!panel?.querySelector('.settings-recovery'),error:!!panel?.querySelector('[role="alert"]')});
    await sleep(500);
  }
  async function settings(){click(one('[aria-label="打开设置"]'));await wait(()=>ready()&&field('auto-play'),'startup settings');}
  async function choose(key,checked,other){const node=field(key);assert(node.checked!==checked,'setting should change');click(node);other?.focus({preventScroll:true});await wait(()=>ready()&&field(key)?.checked===checked,'setting persisted');assert(document.activeElement===(other??field(key)),'saved setting lost or stole keyboard focus');}
  async function theme(name){click(one(`input[name="settings-theme"][value="${name}"]`));await wait(()=>document.documentElement.dataset.theme===name,'theme');}
  async function seek(seconds){const slider=one('[aria-label="播放进度"]');slider.value=String(seconds);slider.dispatchEvent(new Event('input',{bubbles:true}));slider.dispatchEvent(new Event('change',{bubbles:true}));await wait(()=>Math.abs(position()-seconds)<0.2,'native seek');}
  async function refresh(){click(one('[aria-label="重新读取播放设置"]'));await sleep(150);await wait(ready,'preferences read');}
  try{
    await wait(()=>one('.track-row')&&one('[aria-label="打开设置"]')&&!one('[aria-label="打开设置"]').disabled,'app ready');
    if(resumed){
      await wait(()=>state()==='idle','reconnected stopped player');await sleep(800);assert(state()==='idle','reconnection restarted audio');
      await settings();assert(field('auto-play').checked&&field('remember-position').checked,'preference persisted before reload');await capture('reloaded_stopped');
      await choose('auto-play',false);await capture('saved_paused_mode');sessionStorage.removeItem('startup-reload-stage');
    }else if(phase==='initial'){
      await wait(()=>state()==='paused'&&Math.abs(position()-12.5)<0.2,'default paused resume');await capture('startup_paused');
      await settings();await theme('light');assert(!field('auto-play').checked&&field('remember-position').checked,'official defaults');await capture('options_default_light');
      await choose('auto-play',true,one('input[name="settings-theme"][value="light"]'));await choose('remember-position',false);assert(state()==='paused'&&Math.abs(position()-12.5)<0.2,'settings immediately changed transport');await capture('saved_start_zero');
      await emit('resize',{width:760,height:540});await wait(()=>innerWidth===760,'small window');await theme('dark');one('.playback-startup').scrollIntoView({block:'center'});await capture('small_dark');
      await emit('reject-write');click(field('auto-play'));await wait(()=>ready()&&one('.playback-startup [role="alert"]'),'write failure');assert(field('auto-play').checked,'failed save changed checkbox');assert(document.activeElement===one('[aria-label="重新读取播放设置"]'),'failed save lost recovery focus');await capture('save_failed');
      await emit('allow-write');await refresh();assert(!one('.playback-startup [role="alert"]'),'read recovery');await theme('light');one('.playback-startup').scrollIntoView({block:'center'});await capture('read_recovered');await seek(17);
    }else if(phase==='autoplay-zero'){
      await wait(()=>state()==='playing'&&position()>0.2,'autoplay from zero');assert(position()<8,'progress memory disabled but restored position');await capture('playing_zero');
      click(one('[aria-label="暂停"]'));await wait(()=>state()==='paused','pause');await settings();assert(field('auto-play').checked&&!field('remember-position').checked,'saved start settings');await choose('remember-position',true);await seek(17);assert(state()==='paused','editing preferences unpaused audio');await capture('saved_resume');
    }else if(phase==='autoplay-resume'){
      await wait(()=>state()==='playing'&&position()>=17,'autoplay remembered progress');assert(position()<25,'unexpected resume clock');await capture('playing_remembered');
      click(one('[aria-label="停止播放"]'));await wait(()=>state()==='idle','explicit stop');await capture('explicit_stop');
      sessionStorage.setItem('startup-reload-stage',String(stageCount));location.reload();return;
    }else if(phase==='repair'){
      await wait(()=>state()==='paused'&&position()>=17,'paused remembered progress');await capture('remembered_paused');
      await settings();await emit('corrupt');await refresh();assert(one('.playback-startup .settings-recovery')&&!field('auto-play'),'damaged preferences exposed as valid');
      await emit('resize',{width:760,height:540});await wait(()=>innerWidth===760,'small window');await theme('dark');one('.playback-startup').scrollIntoView({block:'center'});await capture('damaged_dark');
      click(all('.playback-startup button').find(n=>n.textContent.trim()==='恢复默认播放设置'));await wait(()=>ready()&&field('auto-play'),'explicit repair');assert(!field('auto-play').checked&&field('remember-position').checked,'repair defaults');assert(document.activeElement===one('[aria-label="重新读取播放设置"]'),'repair focus lost');assert(state()==='paused','repair changed transport');await capture('repaired');
      for(const [damage,name] of [['raw-damage','raw_json'],['lost-record','missing_record']]){
        await emit(damage);await refresh();assert(one('.playback-startup .settings-recovery')&&!field('auto-play'),'damage exposed as defaults');await capture(name+'_damaged');
        click(all('.playback-startup button').find(n=>n.textContent.trim()==='恢复默认播放设置'));await wait(()=>ready()&&field('auto-play'),'explicit text/missing repair');assert(!field('auto-play').checked&&field('remember-position').checked,'repair defaults');assert(document.activeElement===one('[aria-label="重新读取播放设置"]'),'repair focus lost');assert(state()==='paused','repair changed transport');await capture(name+'_repaired');
      }
      await theme('light');one('.playback-startup').scrollIntoView({block:'center'});await capture('repaired_light');
    }else{
      await wait(()=>state()==='paused'&&position()>=17,'repaired restart');await capture('default_paused');await settings();assert(!field('auto-play').checked&&field('remember-position').checked,'repair not durable');await theme('light');one('.playback-startup').scrollIntoView({block:'center'});await capture('reopened_settings');
    }
    await emit('complete',{ok:true,phase,stageCount,realMusicRequests:0,decoder:'libmpv',audioOutput:'null'});
  }catch(error){await emit('failed',{ok:false,phase,stageCount,error:String(error),state:state(),position:position()});}
})();
