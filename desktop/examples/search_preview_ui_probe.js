// Actual production WebView/IPC with synthetic search, music and account state.
(async () => {
  const one=s=>document.querySelector(s), all=s=>[...document.querySelectorAll(s)], sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const assert=(v,m)=>{if(!v)throw Error(m);};
  const wait=async(f,m)=>{for(let i=0;i<220;i++){if(f())return;await sleep(60);}throw Error(m);};
  const invoke=(command,args={})=>window.__TAURI_INTERNALS__.invoke(command,args);
  const emit=(stage,data={})=>invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...data}});
  const button=(name,root=document)=>[...root.querySelectorAll('button')].find(n=>n.getAttribute('aria-label')===name||n.textContent.trim()===name);
  const click=n=>{assert(n&&!n.disabled,'control unavailable');n.focus({preventScroll:true});n.click();};
  const pane=()=>one('.search-result-panel');
  const ready=()=>pane()?.getAttribute('aria-busy')==='false'&&!one('.search-results-enter-active,.search-results-leave-active,.page-enter-active,.page-leave-active');
  const pointer=async(x,y)=>{window.__searchPointerAck=false;await emit('search-pointer',{x,y});await wait(()=>window.__searchPointerAck,'pointer');};
  // A hover match can arrive before WebKit starts painting its opacity transitions.
  // Keep the native pointer and keyboard assertions; wait only for the rendered state.
  async function waitControls(card, state) {
    const mask = card.querySelector('.search-track-mask');
    const actions = card.querySelector('.search-track-actions');
    const deadline = performance.now() + 1600;
    let observed;
    do {
      const style = getComputedStyle(actions);
      observed = {
        hover: card.matches(':hover'),
        focus: card.matches(':focus-within'),
        mask: Number(getComputedStyle(mask).opacity),
        actions: Number(style.opacity),
        hit: style.pointerEvents === 'auto',
      };
      if (state === 'idle' && !observed.hover && !observed.focus && observed.actions === 0 && !observed.hit) return;
      if (state === 'hover' && observed.hover && observed.mask > .99 && observed.actions > .99 && observed.hit) return;
      if (state === 'keyboard' && !observed.hover && observed.focus && observed.actions > .99 && observed.hit) return;
      await sleep(30);
    } while (performance.now() < deadline);
    throw Error(state + ' preview controls did not settle: ' + JSON.stringify(observed));
  }
  const source=Object.getOwnPropertyDescriptor(HTMLImageElement.prototype,'src');let images=0;
  Object.defineProperty(HTMLImageElement.prototype,'src',{...source,set(value){
    if(String(value).includes('/cumulus-preview-')){images++;value=String(value).includes('-bad')?'data:image/png;base64,aW52YWxpZA==':'data:image/svg+xml,'+encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="160" height="160"><rect width="160" height="160" fill="#6c597b"/><circle cx="80" cy="80" r="52" fill="#e0a6b2"/></svg>');}
    source.set.call(this,value);
  }});
  let count=0,step='startup',plays=0,selectionGate,selectionReceived=false;
  const fetch=window.fetch;window.fetch=async function(url,options){
    if(['/play_selection','/prepare_play_selection','/commit_prepared_selection','/player_action'].some(command=>String(url).endsWith(command)))plays++;
    const delay=String(url).endsWith('/play_selection')&&JSON.parse(options.body).selection.kind==='search_result_track'
      ?selectionGate:undefined;
    const response=await fetch.call(window,url,options);
    if(delay){selectionReceived=true;await delay;}
    return response;
  };
  function holdSelection(){
    let release;
    selectionReceived=false;
    selectionGate=new Promise(resolve=>{release=resolve;});
    return ()=>{selectionGate=undefined;release();};
  }
  async function choose(key){click(one(`[data-category="${key}"]`));await sleep(0);await wait(()=>ready()&&one(`[data-category="${key}"]`)?.getAttribute('aria-selected')==='true','category '+key);await sleep(200);}
  async function capture(label){step=label;await wait(()=>all('dialog,.search-result-panel').every(n=>n.getAnimations().every(a=>a.playState!=='running'&&!a.pending)),'animations');await sleep(180);assert(document.documentElement.scrollWidth<=innerWidth&&pane().scrollWidth<=pane().clientWidth+1,'horizontal containment');assert(!one('button button'),'no nested controls');await emit(`ux_${String(++count).padStart(2,'0')}_${label}`,{width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,renderedSongs:all('.search-track-card').length});await sleep(420);}
  function layout(){
    assert(!one('.search-result-panel .track-table'),'comprehensive songs are cards');
    const cards=all('.search-track-card');assert(cards.length===6,'six song previews');assert(all('.search-best-section .search-result-card').length===2,'two best matches');
    const a=cards[0].getBoundingClientRect(),b=cards[1].getBoundingClientRect();assert(Math.abs(a.height-84)<1,'84px song card');assert(innerWidth>1000?Math.abs(a.top-b.top)<1:b.top>=a.bottom-1,'responsive two/one column songs');
    for(const grid of all('[data-preview="true"]')){const items=[...grid.querySelectorAll('.search-grid-cell')],columns=getComputedStyle(grid).gridTemplateColumns.split(' ').length;assert(items.length===Math.min(6,columns),'one full preview row');assert(items.every(n=>Math.abs(n.getBoundingClientRect().top-items[0].getBoundingClientRect().top)<1),'preview must not wrap');}
  }
  try {
    await wait(()=>one('.track-row'),'daily ready');window.__searchFixtureAck=false;await emit('search-fixture',{mode:'preview'});await wait(()=>window.__searchFixtureAck,'fixture');await emit('like-fixture',{healthy:true});
    for(const [w,h,t] of [[1120,760,'light'],[1120,760,'dark'],[760,540,'light'],[760,540,'dark']]){
      await emit('resize',{width:w,height:h});await wait(()=>innerWidth===w&&innerHeight===h,'resize');await pointer(innerWidth-3,75);await sleep(160);click(button('外观主题'));await wait(()=>one('.action-menu'),'theme');click(button(t==='dark'?'深色':'浅色'));await wait(()=>!one('.action-menu'),'theme applied');
      const input=one('[aria-label="搜索音乐"]');input.value='合成预览';input.dispatchEvent(new Event('input',{bubbles:true}));input.form.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));await sleep(0);await wait(()=>ready()&&one('.search-track-card'),'preview ready');await sleep(200);await pointer(innerWidth-3,75);layout();assert(!one('.search-panel'),'submission closes discovery');await capture(`overview_${w}_${t}`);
      const card=one('.search-track-card');card.scrollIntoView({block:'center'});document.activeElement?.blur();await pointer(innerWidth-3,75);await waitControls(card,'idle');
      const r=card.querySelector('.search-track-cover').getBoundingClientRect();await pointer(r.x+r.width/2,r.y+r.height/2);await waitControls(card,'hover');assert(card.querySelector('img')?.naturalWidth>0,'good cover decoded');assert(!all('.search-track-card')[1].querySelector('img'),'bad cover fallback');await capture(`hover_${w}_${t}`);
      await pointer(innerWidth-3,75);const download=card.querySelector('.search-track-actions button');download.focus({preventScroll:true});await waitControls(card,'keyboard');const d=download.getBoundingClientRect();assert(document.elementFromPoint(d.x+d.width/2,d.y+d.height/2)?.closest('button')===download,'keyboard control hit target');await capture(`keyboard_${w}_${t}`);
      const before=plays;download.dispatchEvent(new MouseEvent('dblclick',{bubbles:true}));assert(plays===before,'action double click does not play');click(download);await wait(()=>one('.download-dialog[open]'),'download options');await capture(`download_${w}_${t}`);click(button('关闭下载对话框'));await wait(()=>!one('.download-dialog'),'download closed');assert(document.activeElement===download,'download returns exact focus');
      const more=card.querySelector('.search-track-actions button:last-child');click(more);await wait(()=>one('.song-action-menu'),'song operations');await capture(`more_${w}_${t}`);one('.song-action-menu').dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true}));await wait(()=>!one('.song-action-menu'),'operations closed');assert(document.activeElement===more,'operations return exact focus');
      const link=button('查看全部歌单'),section=link.closest('.search-result-section');section.scrollIntoView({block:'start'});await sleep(200);const saved=pane().scrollTop;await capture(`playlist_preview_${w}_${t}`);click(link);await wait(()=>ready()&&one('[data-category="playlists"]')?.getAttribute('aria-selected')==='true','all playlists');assert(one('.search-result-grid')?.dataset.loadedCount==='10'&&!one('.search-result-pagination'),'full category starts with ten continuous results');await capture(`category_${w}_${t}`);await choose('all');assert(Math.abs(pane().scrollTop-saved)<2,'return restores preview position');layout();await capture(`return_${w}_${t}`);
      button('查看全部MV').closest('.search-result-section').scrollIntoView({block:'start'});await capture(`video_preview_${w}_${t}`);
    }
    await emit('resize',{width:1120,height:760});await wait(()=>innerWidth===1120,'wide preview');await sleep(250);const grid=button('查看全部歌单').closest('.search-result-section').querySelector('[data-preview]');grid.scrollIntoView({block:'center'});const last=[...grid.querySelectorAll('.search-card-open')].at(-1);last.focus({preventScroll:true});await capture('before_resize');await emit('resize',{width:760,height:540});await wait(()=>innerWidth===760,'narrow preview');await sleep(250);assert(!last.isConnected&&grid.contains(document.activeElement),'resize preserves focus in visible preview');const focused=document.activeElement.getBoundingClientRect(),viewport=pane().getBoundingClientRect();assert(focused.top>=viewport.top-1&&focused.bottom<=viewport.bottom+1,'resized preview focus is fully visible');await capture('after_resize');
    const epoch=(await invoke('account_snapshot')).epoch;
    await invoke('play_selection',{epoch,selection:{kind:'songs',ids:['91','92'],index:0,quality:'standard'}});
    const baseline=await invoke('player_queue',{epoch});
    const card=one('.search-track-card');card.scrollIntoView({block:'center'});
    const cover=card.querySelector('.search-track-cover');
    step='preview_play_focus';
    await wait(()=>{
      const rect=cover.getBoundingClientRect();
      return cover.contains(document.elementFromPoint(rect.x+rect.width/2,rect.y+rect.height/2));
    },'cover is visible after scrolling to the preview');
    const focusTrace=[];
    const describeFocus=()=>({
      state:one('.playerbar')?.dataset.state,
      connected:cover.isConnected,
      disabled:cover.disabled,
      coverLabel:cover.getAttribute('aria-label'),
      active:document.activeElement===cover?'cover':document.activeElement?.tagName,
    });
    const record=()=>{if(focusTrace.length<20)focusTrace.push(describeFocus());};
    const focusChanges=new MutationObserver(record);
    focusChanges.observe(cover,{attributes:true,attributeFilter:['disabled','aria-label']});
    record();
    const releasePlay=holdSelection();
    click(cover);
    await wait(()=>selectionReceived&&cover.disabled,'native play result is pending in the WebView');
    // WebKit drops focus on a disabled button; emulate that condition where
    // the platform's WebView keeps focus on it.
    if(document.activeElement===cover)cover.blur();
    assert(document.activeElement===document.body,'play button lost browser focus while busy');
    releasePlay();
    try {
      await wait(()=>card.classList.contains('current')&&!cover.disabled&&
        cover.getAttribute('aria-label').startsWith('暂停')&&document.activeElement===cover,'play restores the exact cover');
    } catch(error) {
      throw Error(`${error.message}: ${JSON.stringify([...focusTrace,describeFocus()])}`);
    } finally {
      focusChanges.disconnect();
    }
    await wait(()=>Number(one('.playerbar').dataset.position)>.15,'preview advances the native playback clock');
    const playing=await invoke('player_queue',{epoch});
    assert(playing.tracks.length===3&&playing.tracks[0].entryId===baseline.tracks[0].entryId&&
      playing.tracks[2].entryId===baseline.tracks[1].entryId&&playing.current===playing.tracks[1].entryId,
    'one preview inserts after current and retains both baseline entries');
    await capture('playing');
    const releasePause=holdSelection();
    click(cover);
    await wait(()=>selectionReceived&&cover.disabled,'native pause result is pending in the WebView');
    const other=button('外观主题');
    other.focus({preventScroll:true});
    releasePause();
    await wait(()=>!cover.disabled&&cover.getAttribute('aria-label').startsWith('播放'),'pause preview');
    assert(document.activeElement===other,'completion must not steal newly chosen focus');
    assert((await invoke('player_queue',{epoch})).revision===playing.revision,'pause leaves queue intact');
    await capture('paused');
    const like=all('.search-track-card')[1].querySelector('.like-button');like.scrollIntoView({block:'nearest'});click(like);await wait(()=>like.getAttribute('aria-pressed')==='true'&&!like.disabled,'liked feedback');await capture('liked');click(like);await wait(()=>like.getAttribute('aria-pressed')==='false'&&!like.disabled,'unliked feedback');await capture('unliked');
    click(button('播放全部搜索单曲'));await wait(()=>one('.search-track-card.current .search-track-cover')?.getAttribute('aria-label').startsWith('暂停'),'play all preview');
    const allQueue=await invoke('player_queue',{epoch});
    // M3001 replaces the ordinary block with the six rechecked privileges;
    // M3095 passes that block, in order, to the explicit Play All action.
    assert(allQueue.tracks.map(row=>row.track.id).join(',')==='1,2,3,4,5,6'&&
      allQueue.current===allQueue.tracks[0].entryId&&
      !allQueue.tracks.some(row=>baseline.tracks.some(old=>old.entryId===row.entryId)),
      'explicit Play All replaces the baseline with the six privilege-checked songs in order');
    await capture('play_all');
    assert(images>0,'image outcome injection used');await emit('complete',{ok:true,stageCount:count,previewLayouts:true,nativePointer:true,songControls:true,playbackFocusVerified:true,coverOutcomeInjected:true,realMusicRequests:0,realAccountWrites:0});
  }catch(e){await emit('failed',{step,stageCount:count,message:String(e),stack:e?.stack,body:document.body.innerText.slice(0,2600)});}
})();
