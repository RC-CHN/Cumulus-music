// Real main/mini WebViews, MusicClient, SQLite and libmpv/null. Synthetic FM only.
(async () => {
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const assert = (ok, text) => { if (!ok) throw Error(text); };
  const wait = async (check, text) => { for (let i = 0; i < 200; i++) { if (await check()) return; await sleep(60); } throw Error(text); };
  const emit = payload => invoke('plugin:event|emit', {event:'mini-proof', payload});
  window.__miniAck = {};
  let sequence = 0, count = 0, last = 'initial', epoch, playId;
  let source = {kind:'personal'};
  async function fixture(kind, payload = {}) {
    const id = ++sequence;
    await emit({kind, id, ...payload});
    await wait(() => id in window.__miniAck, 'fixture ' + kind);
    const value = window.__miniAck[id]; delete window.__miniAck[id];
    if (value.error) { const error = Error(value.error); error.code = value.code; throw error; }
    return value.value ?? value;
  }
  const mini = code => fixture('eval', {source:code});
  async function read() {
    for (let i = 0; i < 5; i++) {
      try { return await mini("return await window.__TAURI_INTERNALS__.invoke('mini_read',{generation:location.hash.slice(6)});"); }
      catch (error) { if (!['stale','snapshot_changed'].includes(error.code) || i === 4) throw error; await sleep(25); }
    }
  }
  const click = label => mini(`const b=[...document.querySelectorAll('button')].find(n=>n.getAttribute('aria-label')===${JSON.stringify(label)}||n.textContent.trim()===${JSON.stringify(label)});if(!b||b.disabled)throw Error('unavailable '+${JSON.stringify(label)});b.focus({preventScroll:true});${label==='返回主窗口'?'setTimeout(()=>b.click(),60)':'b.click()'};return true;`);
  async function theme(value) {
    await mini(`localStorage.setItem('cumulus.appearance.v1',${JSON.stringify(value)});window.dispatchEvent(new StorageEvent('storage',{key:'cumulus.appearance.v1',newValue:${JSON.stringify(value)}}));return true;`);
    await wait(() => mini(`return document.documentElement.dataset.theme===${JSON.stringify(value)};`), 'theme');
  }
  async function settled(mode, family = 'fm') {
    await wait(async () => {
      const value = await read();
      return value.media.mode === mode && value.media.modeFamily === family && await mini(`const b=document.querySelector('[data-mini-mode]');return b.getAttribute('aria-busy')==='false'&&b.closest('.mini-shell').dataset.controlTicket===${JSON.stringify(value.ticket)};`);
    }, 'settled ' + family + '/' + mode);
  }
  async function open() {
    await invoke('mini_enter');
    await wait(async () => !(await fixture('windows')).main, 'main handover');
    await settled((await read()).media.mode);
  }
  async function pauseSeek() {
    await wait(() => ['playing','paused'].includes(document.querySelector('.playerbar')?.dataset.state), 'audio ready');
    await invoke('player_action', {epoch, action:{kind:'pause', paused:true}});
    await invoke('player_action', {epoch, action:{kind:'seek', seconds:10}});
    await wait(() => Math.abs(Number(document.querySelector('.playerbar')?.dataset.position) - 10) < .2, 'paused position');
    playId = document.querySelector('.playerbar').dataset.playId;
  }
  const facts = `
    const rect=n=>{const r=n.getBoundingClientRect();return {left:r.left,top:r.top,right:r.right,bottom:r.bottom,width:r.width,height:r.height}};
    const hit=n=>{const r=rect(n),h=document.elementFromPoint(r.left+r.width/2,r.top+r.height/2);return n===h||n.contains(h)};
    const mode=document.querySelector('[data-mini-mode]'),feedback=document.querySelector('.mini-mode-feedback');
    return {windowKind:'mini',width:innerWidth,height:innerHeight,ratio:devicePixelRatio,visible:document.visibilityState==='visible',focused:document.hasFocus(),theme:document.documentElement.dataset.theme,
      mode:mode.dataset.mode,family:mode.dataset.family,label:mode.getAttribute('aria-label'),pending:mode.getAttribute('aria-busy')==='true',problem:feedback?.textContent??null,
      genericError:document.querySelector('.mini-message[role="alert"]')?.textContent??null,rows:document.querySelectorAll('.mini-queue-rows li').length,queueText:document.querySelector('.mini-queue')?.textContent??null,
      queueEditable:!!document.querySelector('.mini-queue-rows button:not(:disabled)'),queueTrackOpacities:[...document.querySelectorAll('.mini-queue-track')].map(n=>getComputedStyle(n).opacity),title:document.querySelector('.mini-track h1').textContent,subtitle:document.querySelector('.mini-track p').textContent,
      overflow:document.documentElement.scrollWidth>innerWidth,body:document.body.textContent,
      feedback:feedback?rect(feedback):null,timeline:rect(document.querySelector('.mini-timeline')),
      controls:[...document.querySelectorAll('.mini-transport button,.mini-footer button,.mini-mode-feedback button')].filter(n=>!n.disabled).map(n=>({label:n.getAttribute('aria-label')||n.textContent.trim(),rect:rect(n),hit:hit(n)})),
      paintSamples:[{x:3,y:3,color:getComputedStyle(document.querySelector('.mini-shell')).backgroundColor}],
      transportDisabled:[...document.querySelectorAll('.mini-play,[aria-label="上一首"],[aria-label="下一首"],[aria-label="停止"]')].every(n=>n.disabled)};`;
  async function capture(name, {problem = false, empty = false, ordinary = false} = {}) {
    last = name; await sleep(250);
    const data = await mini(facts);
    assert(!data.overflow && !data.genericError && !data.pending, 'contained settled mini');
    assert(data.controls.every(control => control.hit && control.rect.left >= 0 && control.rect.right <= data.width + 1 && control.rect.top >= 0 && control.rect.bottom <= data.height + 1), 'enabled controls hit inside window');
    assert(!!data.problem === problem, 'local mode feedback');
    if (problem) assert(data.feedback.top<=data.timeline.top&&data.feedback.bottom>=data.timeline.bottom,'mode feedback fully covers metadata without clipping visible timeline fragments');
    if (!ordinary) {
      assert(data.family === 'fm' && !data.queueEditable, 'FM retains its readonly queue');
      assert(data.queueTrackOpacities.every(value=>value==='1'), 'readonly FM metadata remains legible');
    }
    if (empty) assert(data.title === '暂时没有漫游推荐' && data.transportDisabled, 'honest empty FM controls');
    const stage = `ux_${String(++count).padStart(2,'0')}_${name}`;
    window.__miniRecorded = null;
    await emit({kind:'snapshot', stage, fmMini:true, fmSource:source, expectedPlayId:empty?null:playId, empty, ordinary, ...data});
    await wait(() => window.__miniRecorded === stage, 'native observation');
  }
  async function returnMain(name, empty = false) {
    await click('返回主窗口');
    await wait(async () => { const windows = await fixture('windows'); return windows.main && windows.mini.length === 0; }, 'return main');
    await wait(() => document.querySelector('.playerbar')?.dataset.state === (empty?'idle':'paused'), 'main actual transport');
    const bar = document.querySelector('.playerbar');
    assert((bar.dataset.playId || null) === (empty?null:playId), 'same main play identity');
    assert(!![...bar.querySelectorAll('button')].find(n=>['无限漫游','单曲循环'].includes(n.getAttribute('aria-label'))), 'main FM mode remains available');
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const stage = `ux_${String(++count).padStart(2,'0')}_${name}`;
    window.__miniRecorded = null;
    await emit({kind:'snapshot', stage, fmMini:true, fmSource:source, windowKind:'main', empty, expectedPlayId:empty?null:playId,
      width:innerWidth,height:innerHeight,ratio:devicePixelRatio,visible:document.visibilityState==='visible',focused:document.hasFocus(),mainPlayerDom:{state:bar.dataset.state,playId:bar.dataset.playId,position:bar.dataset.position}});
    await wait(() => window.__miniRecorded === stage, 'returned main recorded');
  }
  async function start() { await invoke('fm_command', {epoch, source, command:{kind:'start', quality:'standard'}}); }
  try {
    await wait(() => document.querySelector('.track-row'), 'main ready');
    epoch = (await invoke('account_snapshot')).epoch;
    await start(); await pauseSeek(); await open(); await theme('light'); await settled('sequential'); await capture('personal_infinite_light');
    await click('无限漫游'); await settled('single_loop'); await capture('personal_single_light');
    await theme('dark'); await click('单曲循环'); await settled('sequential'); await capture('personal_infinite_dark');
    await fixture('storage', {reject:true}); await click('无限漫游');
    await wait(() => mini("return !!document.querySelector('.mini-mode-feedback');"), 'mode commit denied');
    await capture('failed_dark', {problem:true}); await theme('light'); await capture('failed_light', {problem:true});
    await click('关闭播放模式提示'); await wait(() => mini("return !document.querySelector('.mini-mode-feedback');"), 'dismiss mode feedback'); await capture('dismissed_light');
    await fixture('storage', {reject:false}); await click('无限漫游'); await settled('single_loop'); await capture('retried_light');
    await click('播放队列'); await wait(() => mini("return document.querySelectorAll('.mini-queue-rows li').length===30;"), 'FM first page'); await capture('expanded_light');
    await theme('dark'); await click('下一页'); await wait(() => mini("return document.querySelectorAll('.mini-queue-rows li').length===10;"), 'FM second page'); await capture('second_page_dark');
    await click('单曲循环'); await settled('sequential'); await wait(() => mini("return document.querySelectorAll('.mini-queue-rows li').length===10;"), 'same second page after mode'); await capture('mode_on_second_page_dark');
    await returnMain('returned_main'); await open(); await theme('dark'); await click('无限漫游'); await settled('single_loop'); await capture('reopened_single_dark');
    await invoke('fm_command', {epoch, source, command:{kind:'leave'}});
    source = {kind:'zone', rule_id:'27001', trial_mode:null}; await start();
    await settled('sequential'); await theme('light'); await capture('source_replaced_empty_light', {empty:true});
    await click('无限漫游'); await settled('single_loop'); await capture('empty_single_light', {empty:true});
    await theme('dark'); await click('播放队列'); await wait(() => mini("return document.querySelector('.mini-queue-empty')?.textContent.includes('暂无漫游推荐');"), 'empty FM queue'); await capture('empty_expanded_dark', {empty:true});
    await fixture('storage', {reject:true}); await click('单曲循环');
    await wait(() => mini("return !!document.querySelector('.mini-mode-feedback');"), 'empty mode commit denied'); await capture('empty_failed_dark', {problem:true, empty:true});
    await returnMain('failed_empty_returned_main', true);
    await open(); await theme('dark'); await click('播放队列');
    await wait(() => mini("return document.querySelector('.mini-queue-empty')?.textContent.includes('暂无漫游推荐')&&!!document.querySelector('.mini-mode-feedback');"), 'durable mode recovery after reopening');
    await capture('reopened_empty_failure_dark', {problem:true, empty:true});
    await fixture('storage', {reject:false}); await click('重试'); await settled('sequential'); await capture('empty_retried_dark', {empty:true});
    await returnMain('returned_empty_main', true); await open(); await theme('light'); await capture('reopened_empty_light', {empty:true});
    const old = await read();
    await invoke('fm_command', {epoch, source, command:{kind:'leave'}});
    await invoke('play_selection', {epoch, selection:{kind:'songs', ids:['1','2','3'], index:0, quality:'standard'}}); await pauseSeek();
    await settled('order_loop','ordinary');
    const rejected = await mini(`try{await window.__TAURI_INTERNALS__.invoke('mini_mode',{generation:location.hash.slice(6),ticket:${JSON.stringify(old.ticket)},mode:'single_loop'});return false;}catch(e){return ['stale','snapshot_changed'].includes(e.code);}`);
    assert(rejected, 'old empty FM ticket cannot change ordinary media'); await capture('ordinary_replaced_light', {ordinary:true});
    assert(await mini("try{await window.__TAURI_INTERNALS__.invoke('account_snapshot');return false;}catch{return true;}"), 'mini account boundary');
    await emit({kind:'complete',stage:'complete',ok:true,fmMini:true,stageCount:count,fmSource:source,staleEmptyRejected:rejected,realMusicRequests:0,realAccountWrites:0});
  } catch (error) {
    await emit({kind:'failed',stage:'failed',ok:false,last,count,fmSource:source,error:String(error),body:document.body.textContent.slice(-2000)});
  }
})();
