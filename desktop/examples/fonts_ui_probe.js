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
  let serial=0,stageCount=0,epoch='',mode='native',nativeCatalog,forcedFallback=false;
  const checks=[];
  let retry;
  const fixture = async(kind,payload={}) => {const id=++serial;await emit({kind,id,...payload});await wait(()=>id in window.__lyricAck,kind);const value=window.__lyricAck[id];delete window.__lyricAck[id];if(value.error)throw Error(value.error);return value.value??value;};
  const fontStats = () => fixture('font_catalog');
  const expectNativeReads = async (before, count, label) => {
    const after=await fontStats();
    const nativeReads=(after.sources.native??0)-(before.sources.native??0);
    assert(after.total-before.total===count && nativeReads===count,
      `${label}: expected ${count} native reads; observed ${nativeReads} native, ${after.total-before.total} total`);
    return after;
  };
  const fontMode = async value => {
    await fixture('font_catalog',{mode:value});
    mode=value;
  };
  const rows = dialog => [...dialog.querySelectorAll('.font-list .font-option input')].map(n=>n.value);
  const synthetic = index => `Synthetic Font ${String(index).padStart(5,'0')} with a deliberately long family name`;
  const syntheticPage = start => Array.from({length:20},(_,i)=>synthetic(start+i));
  const sameRows = (actual, expected) => actual.length===expected.length &&
    actual.every((name,index)=>name===expected[index]);
  async function forceFallback() {
    // Tauri 2.11.5 falls back to postMessage after a fetch failure. Exercise
    // that exact transition using a read; never fail or repeat a mutation.
    const original=window.fetch;
    window.fetch=(url,...args)=>{
      if(!forcedFallback && String(url).endsWith('/account_snapshot')){
        forcedFallback=true;
        return Promise.reject(Error('sealed IPC custom-protocol failure'));
      }
      return original.call(window,url,...args);
    };
    try {
      assert((await invoke('account_snapshot')).epoch===epoch && forcedFallback,
        'forced fetch failure must recover the same account via native IPC');
    } finally {
      window.fetch=original;
    }
  }
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
    const catalog=mode==='large' && dialog ? {
      page:dialog.querySelector('.font-pagination span')?.textContent.trim(),
      count:dialog.querySelector('.font-pagination small')?.textContent.trim(),
      names:[...dialog.querySelectorAll('.font-list .font-option input')].map(node=>node.value),
    } : null;
    await emit({kind:'snapshot',stage:`ux_${String(++stageCount).padStart(2,'0')}_${name}`,surface:'main',overflow,width:innerWidth,height:innerHeight,rows,controls:controls.map(rect),dialog:dialog?rect(dialog):null,fontMode:mode,retry:retry??null,forcedFallback,catalog,body:document.body.textContent});
    await sleep(1000);
  };
  const open = async() => {const n=button('选择字体');n.scrollIntoView({block:'center',behavior:'instant'});n.focus();click(n);await Promise.resolve();const animated=one('.font-dialog');if(animated && document.documentElement.dataset.motion==='full')assert(getComputedStyle(animated).transitionDuration.split(',').some(v=>parseFloat(v)>0),'modal enter animation missing');await wait(()=>one('.font-dialog[open]'),'font dialog opened');await wait(()=>one('.font-dialog[open] .font-list[aria-busy="false"]'),'font directory loaded');};
  const close = async() => {click(one('[aria-label="关闭字体选择"]'));await wait(()=>!one('.font-dialog'),'font dialog closed');assert(document.activeElement===button('选择字体'),'focus restored to font button');};
  const apply = async() => {const revision=(await settings()).preferences.current.revision;await sleep(50);click(button('应用样式'));await wait(async()=>(await settings()).preferences.current.revision!==revision,'appearance saved');await wait(()=>!one('.desktop-lyric-settings[aria-busy="true"]'),'saved UI');};
  try {
    await wait(()=>one('.track-row'),'main ready');epoch=(await invoke('account_snapshot')).epoch;
    await invoke('play_selection',{epoch,selection:{kind:'songs',ids:['1','2'],index:0,quality:'standard'}});
    await sleep(250);await invoke('player_action',{epoch,action:{kind:'pause',paused:true}});
    const beforeSettings=await fontStats();
    click(one('[aria-label="打开设置"]'));await wait(()=>one('[aria-label="显示桌面歌词"]'),'settings');
    // The interface and lyric catalogues share the native enumeration lock
    // and fixture counter. Settle the settings read before the lyric proof.
    await wait(()=>one('select[data-interface-font][aria-busy="false"]'),'interface font catalog settled');
    const afterSettings=await expectNativeReads(beforeSettings,1,'interface font setting');
    const start=performance.now();nativeCatalog=await invoke('system_font_catalog',{epoch});const enumerationMs=performance.now()-start;
    assert(nativeCatalog.epoch===epoch && nativeCatalog.families.length>0,'native system fonts empty');
    const afterDirect=await expectNativeReads(afterSettings,1,'explicit system font catalog');
    assert(await invoke('system_font_catalog',{epoch:'stale-account'}).then(()=>false,()=>true),'stale account font request');
    const beforeDialog=await expectNativeReads(afterDirect,0,'stale account font request');
    if(window.__lyricProofReopen){
      assert((await settings()).preferences.current.appearance.font==='Cumulus Missing Font 19317','missing saved name lost on restart');
      await open();assert(one('.font-fallback-note'),'missing font fallback notice after restart');
      await expectNativeReads(beforeDialog,1,'reopened lyric font picker');
      await capture('restart_missing');await close();
      await wait(async()=>(await settings()).visible,'persisted lyric window');const view=await read();assert(view.fontFamilies[0]==='Cumulus Missing Font 19317','missing name not passed to renderer');
      const css=await lyric("return getComputedStyle(document.querySelector('.desktop-lyric-row')).fontFamily;");assert(css.includes('Cumulus Missing Font 19317') && css.includes('宋体') && css.includes('sans-serif'),'lyric CSS fallback chain');
      checks.push('missing font preserved across process restart','native lyric CSS fallback chain');
    } else {
      theme('light');await open();assert(document.activeElement===one('[aria-label="搜索本机字体"]'),'search initial focus');
      const afterDialog=await expectNativeReads(beforeDialog,1,'lyric font picker');
      await capture('native_light');await sleep(1000);
      await expectNativeReads(afterDialog,0,'settings poll must not enumerate fonts');
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
      const beforeFailure=(await fontStats()).total;
      await fontMode('failure');await open();
      const failed=await fontStats();
      assert(failed.total===beforeFailure+1 && failed.sources.failure===1 &&
        one('.font-dialog [role="alert"]'),'one injected font error expected');
      await capture('read_failed');
      await sleep(1000);
      assert((await fontStats()).total===failed.total,'font failure auto retry');

      // Activate the exact current modal button through the same independent
      // native keyboard driver as Tab/Escape. Neither a stale selector nor a
      // direct IPC call can stand in for the user's retry.
      const dialog=one('.font-dialog[open]');
      const refresh=dialog?.querySelector('.font-search > button');
      retry={clicks:0,trusted:false,reads:0};
      await fixture('show');
      await wait(()=>document.hasFocus() && dialog?.isConnected && dialog.open &&
        refresh?.isConnected && !refresh.matches(':disabled') &&
        dialog.querySelector('.font-list')?.getAttribute('aria-busy')==='false',
        'foreground retry button not ready');
      const rect=refresh.getBoundingClientRect();
      const hit=document.elementFromPoint(rect.x+rect.width/2,rect.y+rect.height/2);
      assert(rect.width>0 && rect.height>0 && refresh.contains(hit),'visible retry hit target');
      refresh.focus({preventScroll:true});
      assert(document.activeElement===refresh,'retry button did not take focus');
      refresh.addEventListener('click',event=>{retry.clicks++;retry.trusted=event.isTrusted;});
      await fontMode('native');
      await fixture('key',{key:'Return'});
      await wait(()=>retry.clicks>0,'native retry activation did not reach the button');
      await wait(async()=>{
        retry.reads=(await fontStats()).sources.native-failed.sources.native;
        return retry.reads>0;
      },'retry button did not dispatch the native font read');
      await wait(()=>dialog.isConnected && dialog.open && !dialog.querySelector('[role="alert"]') &&
        dialog.querySelectorAll('.font-list .font-option').length>0,'explicit native retry');
      const expected=nativeCatalog.families.slice(0,20).map(f=>f.name);
      assert(sameRows(rows(dialog),expected),'retry did not restore native font rows');
      assert(retry.clicks===1 && retry.trusted && retry.reads===1 && (await fontStats()).total===failed.total+1,
        'explicit retry must produce one trusted activation and one font read');
      await capture('retry_native');await close();checks.push('visible failure and explicit read-only retry');
      if(window.__fontForceIpcFallback)await forceFallback();
      const beforeLarge=await fontStats();
      await fontMode('large');await open();
      const large=one('.font-dialog[open]');
      const page = () => large.querySelector('.font-pagination span')?.textContent.trim();
      const countText = () => large.querySelector('.font-pagination small')?.textContent.trim();
      await wait(()=>sameRows(rows(large),syntheticPage(0)) && page()==='1 / 601' &&
        countText()==='12005 种字体','first exact synthetic directory page');
      click(large.querySelector('[aria-label="下一页字体"]'));
      await wait(()=>sameRows(rows(large),syntheticPage(20)) && page()==='2 / 601',
        'second exact bounded synthetic directory page');
      await capture('large_directory');
      field('搜索本机字体','合成字体 12004');
      await wait(()=>sameRows(rows(large),[synthetic(12004)]) && page()==='1 / 1',
        'exact alias row and first page after search input');
      await capture('alias_search');
      const afterLarge=await fontStats();
      assert(afterLarge.total===beforeLarge.total+1 && afterLarge.sources.large===1,
        'pagination and alias filtering use one synthetic catalog read');
      await close();checks.push('12005 synthetic fonts bounded to 20 DOM rows','localized alias search and page reset');
      await fontMode('native');await fixture('resize_main',{width:1120,height:760});await open();click(one('.font-default input'));click(button('使用此字体'));await wait(()=>!one('.font-dialog'),'default selected');assert(one('[aria-label="桌面歌词字体"]').value==='','default draft');click(button('还原草稿'));await wait(()=>one('[aria-label="桌面歌词字体"]').value==='Cumulus Missing Font 19317','cancel font draft');checks.push('default draft and unsaved changes restored');
    }
    await emit({kind:'complete',stage:'complete',ok:true,stageCount,checks,realMusicRequests:0,enumerationMs,nativeFamilies:nativeCatalog.families.length,forcedFallback,reopened:!!window.__lyricProofReopen});
  } catch(e) {await emit({kind:'failed',stage:'failed',message:String(e)+'\n'+String(e?.stack||''),stageCount,checks,fontMode:mode,retry:retry??null,forcedFallback,body:document.body.textContent});}
})();
