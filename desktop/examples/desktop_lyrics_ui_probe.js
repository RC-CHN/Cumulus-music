(async () => {
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const assert = (ok, message) => { if (!ok) throw Error(message); };
  const wait = async (fn, message) => { for (let i = 0; i < 150; i++) { if (await fn()) return; await sleep(100); } throw Error(message); };
  const emit = payload => invoke('plugin:event|emit', {event:'desktop-lyric-proof', payload});
  const one = selector => document.querySelector(selector);
  const button = text => [...document.querySelectorAll('.desktop-lyric-settings button')].find(n => n.textContent.trim() === text);
  const click = node => { assert(node && !node.disabled, 'available control'); node.click(); };
  window.__lyricAck = {};
  let serial = 0, stageCount = 0, epoch = '';
  const checks = [];
  const fixture = async (kind, payload = {}) => {
    const id = ++serial; await emit({kind, id, ...payload});
    await wait(() => id in window.__lyricAck, `fixture ${kind}`);
    const result = window.__lyricAck[id]; delete window.__lyricAck[id];
    if (result.error) throw Error(result.error);
    return result.value ?? result;
  };
  const lyric = source => fixture('eval', {source});
  const read = () => lyric("return await window.__TAURI_INTERNALS__.invoke('desktop_lyric_read',{generation:location.hash.split('=')[1]});");
  const settings = () => invoke('desktop_lyric_settings', {epoch});
  const ready = () => wait(async () => (await settings()).visible && (await read()).state === 'ready', 'lyric ready');
  const facts = `const rect=n=>{const r=n.getBoundingClientRect();return {x:r.x,y:r.y,right:r.right,bottom:r.bottom,width:r.width,height:r.height}};
    const controls=[...document.querySelectorAll('.desktop-lyric-toolbar button,.desktop-lyric-toolbar span')].filter(n=>getComputedStyle(n).display!=='none');
    return {width:innerWidth,height:innerHeight,overflow:document.documentElement.scrollWidth>innerWidth,
      background:getComputedStyle(document.body).backgroundColor,rows:document.querySelectorAll('.desktop-lyric-row').length,
      words:document.querySelectorAll('.lyric-ink').length,locked:document.querySelector('main').classList.contains('locked'),
      vertical:document.querySelector('main').classList.contains('vertical'),body:document.body.textContent,
      controls:controls.map(n=>({label:n.getAttribute('aria-label')||n.textContent.trim(),...rect(n)})),
      lines:[...document.querySelectorAll('.desktop-lyric-row')].map(rect)};`;
  const capture = async (name, main = false) => {
    await sleep(550);
    const data = main ? {width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,
      overflow:document.documentElement.scrollWidth>innerWidth,body:document.body.textContent} : await lyric(facts);
    assert(!data.overflow, `${name} horizontal overflow`);
    if (data.controls) assert(data.controls.every(r=>r.x>=-1 && r.right<=data.width+1 && r.bottom<=data.height+1), `${name} controls clipped: ${JSON.stringify(data.controls)}`);
    await emit({kind:'snapshot',stage:`ux_${String(++stageCount).padStart(2,'0')}_${name}`,surface:main?'main':'lyric',...data});
    await sleep(1000);
  };
  const clickLyric = label => lyric(`const n=document.querySelector('[aria-label=${JSON.stringify(label)}]');if(!n||n.disabled)throw Error('lyric control: '+${JSON.stringify(label)});${label==='关闭桌面歌词'?'setTimeout(()=>n.click(),70)':'n.click()'};return true;`);
  const field = (label,value) => { const input=one(`[aria-label="${label}"]`); assert(input, label); input.value=value; input.dispatchEvent(new Event(input.tagName==='SELECT'?'change':'input',{bubbles:true})); };
  const apply = async () => { await sleep(50); const before=(await settings()).preferences.current.revision; click(button('应用样式')); await wait(async()=> (await settings()).preferences.current.revision!==before,'style saved'); await wait(()=>!button('应用样式').disabled || !one('.desktop-lyric-settings[aria-busy="true"]'),'style completed'); await sleep(500); };
  const openSettings = async () => { if(!one('.desktop-lyric-settings')) click(one('[aria-label="打开设置"]')); await wait(()=>one('[aria-label="显示桌面歌词"]'),'settings mounted'); one('.desktop-lyric-settings').scrollIntoView({block:'start',behavior:'instant'}); await sleep(300); };
  const close = async () => { await clickLyric('关闭桌面歌词'); await wait(async()=>!(await settings()).visible,'lyric closed'); assert(!(await settings()).preferences.current.enabled,'close not persisted'); };
  const open = async () => { await openSettings(); await wait(()=>!one('[aria-label="显示桌面歌词"]').disabled && !one('[aria-label="显示桌面歌词"]').checked,'enable available'); click(one('[aria-label="显示桌面歌词"]')); await ready(); };
  const player = action => invoke('player_action',{epoch,action});
  try {
    await wait(()=>one('.track-row'),'main startup');
    epoch=(await invoke('account_snapshot')).epoch;
    await invoke('play_selection',{epoch,selection:{kind:'songs',ids:['1','2','3','4','5','6','7','8'],index:0,quality:'standard'}});
    await sleep(350); await player({kind:'pause',paused:true}); await player({kind:'seek',seconds:1.25});
    await openSettings();
    if(!window.__lyricProofReopen) { localStorage.setItem('cumulus.appearance.v1','light'); window.dispatchEvent(new StorageEvent('storage',{key:'cumulus.appearance.v1',newValue:'light'})); }
    if(window.__lyricProofReopen) {
      await ready(); const state=await read();
      assert(!state.locked && state.preferences.current.appearance.fontSize===30,'restart preferences/unlock');
      await capture('restart_unlocked'); await capture('restart_settings',true);
      const old=await read(); await fixture('account');
      await wait(async()=>!(await read()).media.item,'account clears lyric');
      assert(await lyric(`try{await window.__TAURI_INTERNALS__.invoke('desktop_lyric_action',{generation:${JSON.stringify(old.generation)},ticket:${JSON.stringify(old.ticket)},action:'toggle'});return false}catch{return true}`),'old account action accepted');
      await capture('account_isolated'); checks.push('restart persistence and runtime unlock','account isolation');
    } else {
      await capture('settings_initial',true); await open(); await capture('single_translation');
      const before=await read(); assert(before.lyrics.frame.timedWords && before.lyrics.frame.lines[0].words.length>0,'YRC words');
      assert(await lyric("try{await window.__TAURI_INTERNALS__.invoke('account_snapshot');return false}catch{return true}"),'main IPC exposed');
      assert(await invoke('desktop_lyric_read',{generation:before.generation}).then(()=>false,()=>true),'lyric IPC exposed to main');
      await clickLyric('播放'); await wait(async()=>(await read()).media.playing,'lyric play');
      await sleep(300); await clickLyric('暂停'); await wait(async()=>!(await read()).media.playing,'lyric pause');
      assert((await read()).media.selection.audioPlayId===before.media.selection.audioPlayId,'second player');
      await ready(); const offset=(await read()).lyrics.frame.offset; await clickLyric('歌词延后半秒');
      await wait(async()=>(await read()).lyrics?.frame.offset!==undefined && (await read()).lyrics?.frame.offset!==offset,'offset update');
      assert(Math.abs((await read()).lyrics.frame.offset-offset+0.5)<1e-9,'delay offset sign'); await capture('paused_offset'); checks.push('shared native audio and YRC','IPC isolation','offset control');
      field('桌面歌词行数','false'); await sleep(50); field('桌面歌词双行对齐','split'); field('桌面歌词字号','30'); await apply();
      await wait(async()=>(await lyric(facts)).rows===2,'two lines'); await capture('double_split');
      field('桌面歌词辅助语言','roman'); field('桌面歌词配色','blue'); await apply(); await capture('blue_roman');
      field('桌面歌词配色','custom'); await sleep(80); field('已唱上色','FFCCEEFF'); await apply(); await capture('custom_colors');
      localStorage.setItem('cumulus.appearance.v1','dark'); window.dispatchEvent(new StorageEvent('storage',{key:'cumulus.appearance.v1',newValue:'dark'}));
      await capture('settings_dark',true);
      await fixture('resize_main',{width:800,height:650}); await sleep(400); one('.desktop-lyric-settings').scrollIntoView({block:'start',behavior:'instant'});
      await capture('settings_narrow',true); button('应用样式').scrollIntoView({block:'center',behavior:'instant'}); await capture('settings_narrow_actions',true);
      await fixture('resize_main',{width:1120,height:760}); await sleep(400); one('.desktop-lyric-settings').scrollIntoView({block:'start',behavior:'instant'});
      field('桌面歌词方向','vertical'); await apply(); click(button('重置窗口位置')); await sleep(650); await capture('vertical');
      field('桌面歌词方向','horizontal'); field('桌面歌词行数','true'); await apply(); click(button('重置窗口位置')); await sleep(650);
      const saved=(await settings()).preferences.current;
      await invoke('desktop_lyric_change',{epoch,command:{kind:'change',expected_revision:saved.revision,edit:{kind:'geometry',value:{x:450,y:300,width:240,height:220}}}});
      await capture('compact_transparent');
      await fixture('focus_lyrics');
      await lyric("document.querySelector('[aria-label=\"桌面歌词设置\"]').focus();return true;"); await capture('compact_controls');
      assert(await lyric("const n=document.querySelector('.lyric-channel.auxiliary .desktop-lyric-text');return !!n && getComputedStyle(n).transform==='matrix(1, 0, 0, 1, 0, 0)';"),'short auxiliary line scrolled away');
      click(button('重置窗口位置')); await sleep(650);
      // A dirty appearance draft survives passive window-status updates.
      field('桌面歌词字号','32'); await clickLyric('锁定桌面歌词');
      await wait(()=>button('解锁桌面歌词'),'passive unlock entry'); assert(one('[aria-label="桌面歌词字号"]').value==='32','passive refresh lost draft');
      await capture('locked'); click(button('解锁桌面歌词')); await wait(async()=>!(await read()).locked,'unlock'); click(button('还原草稿'));
      await capture('unlocked');
      await lyric("document.querySelector('.lyric-details').click();return true;"); await wait(()=>one('.lyrics-panel .lyric-line'),'detail event opens shared lyrics');
      const shared=(await read()).lyrics.frame.offset; click(one('.lyrics-panel [aria-label="歌词提前半秒"]'));
      await wait(async()=> (await read()).lyrics?.frame.offset===shared+0.5,'main offset reaches desktop lyrics'); await capture('main_detail',true);
      await clickLyric('桌面歌词设置'); await wait(()=>!one('.lyrics-panel') && one('.desktop-lyric-settings'),'settings event');
      checks.push('main details and shared offset','main settings event');
      checks.push('style controls and custom palette','vertical layout','passive unlock and draft retention');
      const old=await read(); await player({kind:'next'}); await player({kind:'pause',paused:true});
      assert(await lyric(`try{await window.__TAURI_INTERNALS__.invoke('desktop_lyric_action',{generation:${JSON.stringify(old.generation)},ticket:${JSON.stringify(old.ticket)},action:'toggle'});return false}catch{return true}`),'stale track action');
      await ready(); checks.push('stale track rejection');
      await fixture('offline',{enabled:true}); await player({kind:'next'}); await player({kind:'pause',paused:true});
      await wait(async()=>(await read()).state==='failed','failure state'); const count=(await fixture('stats')).requests;
      await sleep(700); assert((await fixture('stats')).requests===count,'unbounded fetch retry'); await capture('fetch_failed');
      await fixture('offline',{enabled:false}); await lyric("document.querySelector('.desktop-lyric-error button').click();return true;"); await ready();
      await capture('fetch_recovered'); checks.push('bounded fetch failure and explicit retry');
      await fixture('hold',{enabled:true}); await player({kind:'next'}); await player({kind:'pause',paused:true});
      await wait(async()=>(await fixture('stats')).pending===1,'held fetch started'); await capture('loading');
      await close(); await wait(async()=>(await fixture('stats')).pending===0,'close cancels fetch'); await fixture('hold',{enabled:false});
      await fixture('block',{enabled:true}); await wait(()=>!one('[aria-label="显示桌面歌词"]').checked,'closed checkbox updated'); click(one('[aria-label="显示桌面歌词"]'));
      await wait(async()=>(await settings()).opening,'opening state'); assert((await fixture('windows')).main,'main hidden');
      await wait(async()=>!(await settings()).opening && !!(await settings()).error,'bounded startup timeout');
      one('.desktop-lyric-settings [role="alert"]').scrollIntoView({block:'center',behavior:'instant'}); await capture('startup_failed',true); await fixture('block',{enabled:false}); click(button('重试打开')); await ready();
      await lyric('setTimeout(()=>location.reload(),60);return true;'); await wait(async()=>!(await settings()).visible && !!(await settings()).error,'reload retires window');
      await wait(()=>button('重试打开'),'retry after reload'); click(button('重试打开')); await ready(); await capture('reloaded');
      checks.push('fetch cancellation','startup timeout and explicit retry','reload retires generation');
      await fixture('key',{key:'d'}); await wait(async()=>!(await settings()).visible,'global shortcut closes lyrics');
      await fixture('key',{key:'d'}); await ready(); checks.push('independent global shortcut');
      await lyric("document.body.dispatchEvent(new KeyboardEvent('keydown',{key:'p',code:'KeyP',ctrlKey:true,bubbles:true,cancelable:true}));return true;");
      await wait(async()=>(await read()).media.playing,'local lyric shortcut'); await player({kind:'pause',paused:true}); await ready();
      checks.push('lyric window local shortcut');
      await fixture('storage',{fail:true}); field('桌面歌词字号','34'); await sleep(50); click(button('应用样式'));
      await wait(()=>one('.desktop-lyric-settings [role="alert"]'),'save failure'); assert((await read()).preferences.current.appearance.fontSize===30,'failed write changed style');
      one('.desktop-lyric-settings [role="alert"]').scrollIntoView({block:'center',behavior:'instant'}); await capture('save_failed',true); await fixture('storage',{fail:false}); click(button('还原草稿'));
      await fixture('storage',{corrupt:true}); await wait(async()=>!(await settings()).visible,'damaged prefs close window');
      await wait(()=>button('恢复默认桌面歌词设置'),'repair entry'); button('恢复默认桌面歌词设置').scrollIntoView({block:'center',behavior:'instant'}); await capture('damaged_settings',true); click(button('恢复默认桌面歌词设置'));
      await wait(()=>one('[aria-label="显示桌面歌词"]'),'repaired'); await open(); field('桌面歌词字号','30'); await apply();
      await clickLyric('锁定桌面歌词'); await wait(async()=>(await read()).locked,'final lock');
      checks.push('failed save preserves appearance','damaged storage explicit repair');
    }
    const stats=await fixture('stats'); assert(stats.factoryCount===2,'lyric surface created an additional audio engine');
    await emit({kind:'complete',stage:'complete',ok:true,stageCount,checks,realMusicRequests:0,reopened:!!window.__lyricProofReopen});
  } catch(e) { await emit({kind:'failed',stage:'failed',message:String(e)+'\n'+String(e?.stack||''),body:document.body.textContent,stageCount,checks}); }
})();
