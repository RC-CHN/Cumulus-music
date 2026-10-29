(async () => {
  const native = window.__TAURI_INTERNALS__.invoke.bind(window.__TAURI_INTERNALS__);
  const invoke = (command, args = {}) => native(command, args);
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const assert = (value, message) => { if (!value) throw Error(message); };
  const wait = async (fn, message) => { for(let i=0;i<150;i++){if(await fn())return;await sleep(100);}throw Error(message); };
  const emit = payload => invoke('plugin:event|emit',{event:'desktop-lyric-proof',payload});
  const one = selector => document.querySelector(selector);
  const button = text => [...document.querySelectorAll('button')].find(n=>n.textContent.trim()===text);
  const click = n => {assert(n && !n.disabled,'available control');n.click();};
  const field = (label,value) => {const n=one(`[aria-label="${label}"]`);assert(n,label);n.value=value;n.dispatchEvent(new Event('input',{bubbles:true}));};
  window.__lyricAck={};
  let serial=0,stageCount=0,epoch='',fontReads=0,mode='native',nativeCatalog;
  const checks=[];
  // Tauri's invoke property is immutable. Intercept only this command's fetch
  // transport in the test WebView; all ordinary font reads reach the real host.
  const nativeFetch = window.fetch.bind(window);
  window.fetch = (url, options) => {
    if(!String(url).endsWith('/system_font_catalog'))return nativeFetch(url,options);
    fontReads++;
    if(mode==='native')return nativeFetch(url,options);
    const args=JSON.parse(options.body);
    const value=mode==='failure'?{code:'state',message:'synthetic font enumeration failure'}:
      {epoch:args.epoch,fallbacks:nativeCatalog.fallbacks,families:Array.from({length:12005},(_,i)=>({name:`Synthetic Font ${String(i).padStart(5,'0')} with a deliberately long family name`,aliases:[`合成字体 ${i}`]}))};
    return Promise.resolve(new Response(JSON.stringify(value),{headers:{'Content-Type':'application/json','Tauri-Response':mode==='failure'?'error':'ok'}}));
  };
  const fixture = async(kind,payload={}) => {const id=++serial;await emit({kind,id,...payload});await wait(()=>id in window.__lyricAck,kind);const value=window.__lyricAck[id];delete window.__lyricAck[id];if(value.error)throw Error(value.error);return value.value??value;};
  const lyric = source => fixture('eval',{source});
  const settings = () => invoke('desktop_lyric_settings',{epoch});
  const read = () => lyric("return await window.__TAURI_INTERNALS__.invoke('desktop_lyric_read',{generation:location.hash.split('=')[1]});");
  const theme = value => {localStorage.setItem('cumulus.appearance.v1',value);window.dispatchEvent(new StorageEvent('storage',{key:'cumulus.appearance.v1',newValue:value}));};
  const capture = async(name) => {
    await sleep(350);
    const dialog=one('.font-dialog');
    const controls=[...(dialog??one('.desktop-lyric-settings')).querySelectorAll('button,input')].filter(n=>n.getBoundingClientRect().width>0);
    const rect=n=>{const r=n.getBoundingClientRect();return {x:r.x,y:r.y,right:r.right,bottom:r.bottom,width:r.width,height:r.height};};
    const overflow=document.documentElement.scrollWidth>innerWidth || !!(dialog && dialog.scrollWidth>dialog.clientWidth+1);
    assert(!overflow,name+' horizontal overflow');
    if(dialog){const r=rect(dialog);assert(r.x>=0 && r.y>=0 && r.right<=innerWidth+1 && r.bottom<=innerHeight+1,name+' dialog outside window');}
    if(dialog && !one('.font-dialog [role="alert"]')) {
      const footer=rect(one('.font-dialog footer')),preview=rect(one('.font-preview'));
      assert(footer.y>=preview.bottom+8 && footer.bottom<=rect(dialog).bottom-8,name+' footer overlaps preview or falls outside dialog');
    }
    const rows=document.querySelectorAll('.font-list .font-option').length;
    assert(rows<=20,'font rows unbounded');
    for(const row of document.querySelectorAll('.font-list .font-option')) {
      const radio=rect(row.querySelector('input')),name=rect(row.querySelector('span'));
      assert(rect(row).height<=44 && radio.right<=name.x && Math.abs(radio.y+radio.height/2-name.y-name.height/2)<5,'font radio and name are not aligned');
    }
    await emit({kind:'snapshot',stage:`ux_${String(++stageCount).padStart(2,'0')}_${name}`,surface:'main',overflow,width:innerWidth,height:innerHeight,rows,controls:controls.map(rect),dialog:dialog?rect(dialog):null,fontMode:mode,fontReads,body:document.body.textContent});
    await sleep(1000);
  };
  const open = async() => {const n=button('选择字体');n.scrollIntoView({block:'center',behavior:'instant'});n.focus();click(n);await Promise.resolve();const animated=one('.font-dialog');if(animated && document.documentElement.dataset.motion==='full')assert(getComputedStyle(animated).transitionDuration.split(',').some(v=>parseFloat(v)>0),'modal enter animation missing');await wait(()=>one('.font-dialog[open]'),'font dialog opened');await wait(()=>!one('.font-list[aria-busy="true"]'),'font directory loaded');};
  const close = async() => {click(one('[aria-label="关闭字体选择"]'));await wait(()=>!one('.font-dialog'),'font dialog closed');assert(document.activeElement===button('选择字体'),'focus restored to font button');};
  const apply = async() => {const revision=(await settings()).preferences.current.revision;await sleep(50);click(button('应用样式'));await wait(async()=>(await settings()).preferences.current.revision!==revision,'appearance saved');await wait(()=>!one('.desktop-lyric-settings[aria-busy="true"]'),'saved UI');};
  try {
    await wait(()=>one('.track-row'),'main ready');epoch=(await invoke('account_snapshot')).epoch;
    await invoke('play_selection',{epoch,selection:{kind:'songs',ids:['1','2'],index:0,quality:'standard'}});
    await sleep(250);await invoke('player_action',{epoch,action:{kind:'pause',paused:true}});
    click(one('[aria-label="打开设置"]'));await wait(()=>one('[aria-label="显示桌面歌词"]'),'settings');
    const start=performance.now();nativeCatalog=await invoke('system_font_catalog',{epoch});const enumerationMs=performance.now()-start;
    assert(nativeCatalog.epoch===epoch && nativeCatalog.families.length>0,'native system fonts empty');
    assert(await invoke('system_font_catalog',{epoch:'stale-account'}).then(()=>false,()=>true),'stale account font request');
    if(window.__lyricProofReopen){
      assert((await settings()).preferences.current.appearance.font==='Cumulus Missing Font 19317','missing saved name lost on restart');
      await open();assert(one('.font-fallback-note'),'missing font fallback notice after restart');await capture('restart_missing');await close();
      await wait(async()=>(await settings()).visible,'persisted lyric window');const view=await read();assert(view.fontFamilies[0]==='Cumulus Missing Font 19317','missing name not passed to renderer');
      const css=await lyric("return getComputedStyle(document.querySelector('.desktop-lyric-row')).fontFamily;");assert(css.includes('Cumulus Missing Font 19317') && css.includes('宋体') && css.includes('sans-serif'),'lyric CSS fallback chain');
      checks.push('missing font preserved across process restart','native lyric CSS fallback chain');
    } else {
      theme('light');await open();assert(document.activeElement===one('[aria-label="搜索本机字体"]'),'search initial focus');await capture('native_light');
      const count=fontReads;assert(count>=3,'font transport observer inactive');await sleep(1000);assert(fontReads===count,'settings poll enumerates fonts');
      await fixture('show');one('[aria-label="搜索本机字体"]').focus();await fixture('key',{key:'Tab'});assert(one('.font-dialog').contains(document.activeElement),'Tab escaped modal');
      await fixture('key',{key:'Escape'});await wait(()=>!one('.font-dialog'),'native Escape closes dialog');assert(document.activeElement===button('选择字体'),'Escape lost trigger focus');
      checks.push('native font enumeration and no polling','independent Tab and Escape with focus restoration');
      await open();
      const chosen=nativeCatalog.families.find(f=>f.name.includes('Noto Sans CJK'))?.name??nativeCatalog.families[0].name;
      field('搜索本机字体',chosen);await sleep(150);const option=[...document.querySelectorAll('.font-list input')].find(n=>n.value===chosen);click(option);await capture('native_selected');
      click(button('使用此字体'));await wait(()=>!one('.font-dialog'),'selected');assert(one('[aria-label="桌面歌词字体"]').value===chosen,'selection not draft');await apply();
      click(one('[aria-label="显示桌面歌词"]'));await wait(async()=>(await settings()).visible,'lyric enabled');
      await wait(async()=>(await read()).state==='ready','lyric ready');assert((await read()).fontFamilies[0]===chosen,'font renderer does not match saved selection');
      assert(await lyric("try{await window.__TAURI_INTERNALS__.invoke('system_font_catalog',{epoch:"+JSON.stringify(epoch)+"});return false}catch{return true}"),'lyric window exposed native catalog');
      checks.push('selected native font applied to shared lyric window','main-only IPC and stale account rejection');
      await open();field('搜索本机字体','Cumulus No Such Search 59490');await sleep(120);await capture('no_match');await close();
      field('桌面歌词字体','Cumulus Missing Font 19317');await apply();await open();assert(one('.font-fallback-note'),'missing font message');theme('dark');await capture('missing_dark');
      await fixture('resize_main',{width:760,height:540});await capture('missing_narrow');await close();
      mode='failure';await open();assert(one('.font-dialog [role="alert"]'),'font error absent');await capture('read_failed');const failedCount=fontReads;await sleep(1000);assert(fontReads===failedCount,'font failure auto retry');mode='native';click([...document.querySelectorAll('.font-dialog button')].find(n=>n.textContent.trim()==='刷新'));await wait(()=>!one('.font-dialog [role="alert"]') && document.querySelectorAll('.font-list .font-option').length>0,'explicit native retry');await capture('retry_native');await close();checks.push('visible failure and explicit read-only retry');
      mode='large';await open();assert(document.querySelectorAll('.font-list .font-option').length===20,'large page rows');click(one('[aria-label="下一页字体"]'));await sleep(100);assert(one('.font-list input').value.includes('00020'),'second bounded page');await capture('large_directory');field('搜索本机字体','合成字体 12004');await sleep(100);assert(document.querySelectorAll('.font-list input').length===1,'alias search');await capture('alias_search');await close();checks.push('12005 synthetic fonts bounded to 20 DOM rows','localized alias search and page reset');
      mode='native';await fixture('resize_main',{width:1120,height:760});await open();click(one('.font-default input'));click(button('使用此字体'));await wait(()=>!one('.font-dialog'),'default selected');assert(one('[aria-label="桌面歌词字体"]').value==='','default draft');click(button('还原草稿'));await wait(()=>one('[aria-label="桌面歌词字体"]').value==='Cumulus Missing Font 19317','cancel font draft');checks.push('default draft and unsaved changes restored');
    }
    await emit({kind:'complete',stage:'complete',ok:true,stageCount,checks,realMusicRequests:0,enumerationMs,nativeFamilies:nativeCatalog.families.length,fontReads,reopened:!!window.__lyricProofReopen});
  } catch(e) {await emit({kind:'failed',stage:'failed',message:String(e)+'\n'+String(e?.stack||''),stageCount,checks,body:document.body.textContent});}
})();
