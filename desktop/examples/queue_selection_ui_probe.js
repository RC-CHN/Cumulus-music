// Real Vue/WebView + Rust queue/SQLite/libmpv; isolated synthetic account, null audio.
(async () => {
 const one=s=>document.querySelector(s), all=s=>[...document.querySelectorAll(s)], sleep=ms=>new Promise(r=>setTimeout(r,ms));
 const invoke=(...a)=>(window.__CUMULUS_PROOF_INVOKE__??window.__TAURI_INTERNALS__.invoke)(...a);
 const emit=(stage,value={})=>invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...value}});
 const assert=(ok,m)=>{if(!ok)throw Error(m);};
 const wait=async(f,m)=>{for(let i=0;i<300;i++){if(f())return;await sleep(50);}throw Error(m);};
 const button=(label,root=document)=>[...root.querySelectorAll('button')].find(n=>n.getAttribute('aria-label')===label||n.textContent.trim()===label);
 const click=n=>{assert(n&&!n.disabled,'enabled '+n?.outerHTML.slice(0,120));n.focus({preventScroll:true});n.click();};
 const bar=()=>one('.playerbar'),panel=()=>one('.queue-panel'),body=()=>one('.queue-scroll');
 const rows=()=>all('.queue-row'),selected=()=>rows().filter(n=>n.classList.contains('selected')).map(n=>n.dataset.entryId);
 const row=id=>rows().find(n=>n.dataset.entryId===id), song=id=>row(id)?.querySelector('.queue-song');
 const rect=n=>{const r=n.getBoundingClientRect();return {left:r.left,top:r.top,right:r.right,bottom:r.bottom,width:r.width,height:r.height};};
 const hit=n=>{const r=rect(n),top=document.elementFromPoint((r.left+r.right)/2,(r.top+r.bottom)/2);return n===top||n.contains(top);};
 const key=(n,k,mod={})=>n.dispatchEvent(new KeyboardEvent('keydown',{key:k,bubbles:true,cancelable:true,...mod}));
 const pointer=(n,type,x,y,mod={})=>n.dispatchEvent(new PointerEvent(type,{pointerId:42,pointerType:'mouse',button:0,buttons:type==='pointerup'?0:1,clientX:x,clientY:y,bubbles:true,cancelable:true,...mod}));
 let client,epoch,playId,count=0,last='bootstrap',phase='initial',editTrace=[];
 function traceRejectedFocus(original) {
  const describe=n=>n instanceof Element?{tag:n.tagName,class:n.className,entry:n.closest('.queue-row')?.dataset.entryId}:null;
  const record=(event,extra={})=>{if(editTrace.length<60)editTrace.push({event,at:performance.now(),active:describe(document.activeElement),originalConnected:original.isConnected,originalDisabled:original.matches(':disabled'),containerConnected:!!body(),...extra});};
  const focus=e=>record(e.type,{target:describe(e.target),related:describe(e.relatedTarget)});
  document.addEventListener('focusin',focus,true);document.addEventListener('focusout',focus,true);
  const observer=new MutationObserver(()=>record('mutation'));
  observer.observe(panel(),{subtree:true,childList:true,attributes:true,attributeFilter:['disabled']});
  record('capture');
  return ()=>{record('stop');observer.disconnect();document.removeEventListener('focusin',focus,true);document.removeEventListener('focusout',focus,true);};
 }
 async function ready(){await wait(()=>panel()&&!button('清空',panel()).disabled,'queue editable');}
 async function scrollTop(){body().scrollTop=0;body().dispatchEvent(new Event('scroll'));await wait(()=>row((window.__queueIds??[])[0]),'first virtual row');await sleep(60);}
 async function choose(id,mod={}){const n=song(id);assert(n,'visible selection row');n.focus({preventScroll:true});const r=rect(n);pointer(n,'pointerdown',r.left+8,r.top+12,mod);pointer(n,'pointerup',r.left+8,r.top+12,mod);await sleep(35);}
 async function prepare(){await wait(()=>bar().dataset.state==='playing'&&Number(bar().dataset.position)>.1,'native playing');click(button('暂停',bar()));await wait(()=>bar().dataset.state==='paused','pause');const seek=one('[aria-label="播放进度"]');seek.value='10';seek.dispatchEvent(new Event('change',{bubbles:true}));await wait(()=>Math.abs(Number(bar().dataset.position)-10)<.2&&!seek.disabled,'seek');playId=bar().dataset.playId;}
 async function capture(label,extra={}){
  last=label;await sleep(220);const p=rect(panel());
  assert(document.documentElement.scrollWidth<=innerWidth&&p.right<=innerWidth&&p.bottom<=rect(bar()).top+1,'contained');
  assert(rows().length<40,'virtual rendering bounded');
  const activeRemoval=label==='paused_current_removed';
  const position=Number(bar().dataset.position);
  assert(bar().dataset.playId===playId&&(activeRemoval
    ? bar().dataset.state==='playing'&&position>.15
    : bar().dataset.state==='paused'&&Math.abs(position-10)<.2),'queue edit uses the expected native transport');
  assert(!one('.playback-message'),'edit error isolated from audio');
  const controls=all('.queue-selection button');assert(controls.every(hit),'selected controls hit');
  const stage=`ux_${String(++count).padStart(2,'0')}_${label}`;
  window.__queueSelectionRecorded=null;
  await emit(stage,{queueSelectionUi:true,phase,width:innerWidth,height:innerHeight,ratio:devicePixelRatio,screenX:window.screenX,screenY:window.screenY,visible:document.visibilityState==='visible',focused:document.hasFocus(),theme:document.documentElement.dataset.theme,rendered:rows().length,selection:selected(),selectionText:one('.queue-selection')?.textContent.trim()??'',dragging:all('.queue-row.dragging').length,marker:one('.queue-drop-marker')&&rect(one('.queue-drop-marker')),scrollTop:body()?.scrollTop,playId,position:Number(bar().dataset.position),active:document.activeElement?.outerHTML.slice(0,350),panel:p,paintSamples:[{x:p.left+4,y:p.top+4,color:getComputedStyle(panel()).backgroundColor}],...extra});
  await wait(()=>window.__queueSelectionRecorded===stage,'native observation saved');
 }
 async function fixture(kind){window.__modeFixtureAck=null;await emit('mode-fixture',{kind});await wait(()=>window.__modeFixtureAck===kind,'fixture');}
 async function appearance(theme){click(button('外观主题'));await wait(()=>one('.action-menu'),'theme menu');click(button(theme==='dark'?'深色':'浅色',one('.action-menu')));await wait(()=>document.documentElement.dataset.theme===theme&&!one('.action-menu'),'theme changed');}
 async function resize(width,height){await emit('resize',{width,height});await sleep(180);if(innerWidth!==width||innerHeight!==height)await emit('resize',{width:width*width/innerWidth,height:height*height/innerHeight});await wait(()=>innerWidth===width&&innerHeight===height,'resize');}
 async function begin(id,targetY){const n=song(id),r=rect(n);n.focus({preventScroll:true});pointer(n,'pointerdown',r.left+8,r.top+12);pointer(document,'pointermove',r.left+15,targetY);await wait(()=>one('.queue-drag-hint'),'drag preview');return {x:r.left+15,y:targetY};}
 async function end(point){pointer(document,'pointerup',point.x,point.y);await wait(()=>!one('.queue-drag-hint'),'drag ended');await ready();}
 try {
  await wait(()=>button('列表循环')&&!button('列表循环').disabled&&one('.track-row'),'ready');
  const provides=one('#app').__vue_app__._context.provides;client=provides[Reflect.ownKeys(provides).find(k=>typeof k==='symbol'&&k.description==='core-client')];epoch=(await client.accountSnapshot()).epoch;
  await resize(1120,760);await client.playSelection(epoch,{kind:'songs',ids:Array.from({length:40},(_,i)=>String(i+1)),index:34,quality:'standard'});await prepare();click(button('播放队列'));await ready();
  let q=await client.playerQueue(epoch),ids=q.tracks.map(t=>t.entryId);window.__queueIds=ids;await scrollTop();
  const initialRevision=q.revision;
  const firstRect=rect(song(ids[0]));await choose(ids[0]);assert(rect(song(ids[0])).top===firstRect.top,'selection toolbar never shifts the click target');assert(selected().join()===ids[0],'single selects without playing');await capture('single_selected');
  await choose(ids[2],{ctrlKey:true});assert(selected().join()===[ids[0],ids[2]].join(),'control toggles');await capture('control_selected');
  await choose(ids[5],{shiftKey:true});assert(selected().join()===ids.slice(2,6).join(),'shift range replaces');await capture('shift_range');
  await choose(ids[4],{ctrlKey:true,shiftKey:true});assert(selected().join()===[ids[5]].join(),'control shift subtracts anchored interval');await capture('subtract_range');
  assert((await client.playerQueue(epoch)).revision===initialRevision,'selection never writes queue');
  await choose(ids[0]);await choose(ids[2],{ctrlKey:true});await choose(ids[0]);assert(selected().join()===ids[0],'release collapses a selected group');await capture('collapsed_selection');
  await choose(ids[2],{ctrlKey:true});let target=rect(row(ids[4]));let point=await begin(ids[0],target.top+6);assert(selected().length===2&&all('.queue-row.dragging').length===2,'drag keeps group');await capture('group_preview');await end(point);
  q=await client.playerQueue(epoch);assert(q.tracks.slice(0,5).map(t=>t.entryId).join()===[ids[1],ids[3],ids[0],ids[2],ids[4]].join(),'native stable block reorder');await capture('group_reordered');
  ids=q.tracks.map(t=>t.entryId);window.__queueIds=ids;
  // Cancellation paths never issue an edit; previews are local state only.
  const beforeCancel=q.revision;point=await begin(ids[0],rect(row(ids[4])).top+5);key(document,'Escape');await wait(()=>!one('.queue-drag-hint'),'escape drag cancel');assert(panel()&&(await client.playerQueue(epoch)).revision===beforeCancel,'escape keeps panel and queue');await capture('escape_cancelled');
  point=await begin(ids[0],rect(row(ids[4])).top+5);pointer(document,'pointercancel',point.x,point.y);await wait(()=>!one('.queue-drag-hint'),'pointer cancellation');assert((await client.playerQueue(epoch)).revision===beforeCancel,'pointer cancel no write');await capture('pointer_cancelled');
  point=await begin(ids[0],rect(row(ids[4])).top+5);pointer(document,'pointermove',rect(panel()).left-20,point.y);await sleep(30);assert(!one('.queue-drop-marker'),'outside target hidden');await end({x:rect(panel()).left-20,y:point.y});assert((await client.playerQueue(epoch)).revision===beforeCancel,'outside drop no write');await capture('outside_cancelled');
  // A changed displayed revision cancels the gesture; pointer-up cannot reuse it.
  point=await begin(ids[0],rect(row(ids[4])).top+5);await client.playerAction(epoch,{kind:'mode',mode:'single_loop'});await wait(()=>!one('.queue-drag-hint'),'revision cancels drag');await ready();const changed=(await client.playerQueue(epoch)).revision;await end(point);assert((await client.playerQueue(epoch)).revision===changed,'old drag not retried');await capture('revision_cancelled');
  await choose(ids[0]);await choose(ids[2],{ctrlKey:true});await fixture('reject');song(ids[2]).focus();const stopFocusTrace=traceRejectedFocus(song(ids[2]));key(document.activeElement,'Delete');assert(document.activeElement===body(),'pending edit parks focus before disabling the original button');await wait(()=>panel().querySelector('[role="alert"]'),'batch failure');await ready();assert((await client.playerQueue(epoch)).tracks.length===40,'failed batch atomic');assert(selected().length===2,'failed batch retains selection');await wait(()=>document.activeElement===song(ids[2]),'failed edit returns enabled original control');stopFocusTrace();await capture('remove_failed',{editTrace});await fixture('allow');
  const expected=ids.filter(id=>![ids[0],ids[2]].includes(id));song(ids[2]).focus();key(document.activeElement,'Delete');await wait(()=>one('.queue-panel h2 small').textContent.trim()==='38','batch removed');await ready();q=await client.playerQueue(epoch);assert(q.tracks.map(t=>t.entryId).join()===expected.join(),'native batch removes only selected');await wait(()=>document.activeElement.matches('.queue-song'),'batch focus successor');await capture('batch_removed');ids=expected;window.__queueIds=ids;
  // Selection is complete, including offscreen entries, rather than just DOM rows.
  key(document.activeElement,'a',{ctrlKey:true});await wait(()=>one('.queue-selection')?.textContent.includes('38'),'select entire queue');await capture('all_selected');
  click(button('取消选择',panel()));await wait(()=>!one('.queue-selection button')&&document.activeElement.matches('.queue-song'),'clear selection restores row focus');await capture('selection_cleared');
  await scrollTop();const beforeScroll=(await client.playerQueue(epoch)).revision;point=await begin(ids[0],rect(body()).bottom-8);const originalTop=body().scrollTop;await wait(()=>body().scrollTop>originalTop+500,'edge auto scroll');assert(rows().length<40,'virtual auto scroll');await capture('autoscroll_preview');pointer(document,'pointercancel',point.x,point.y);await wait(()=>!one('.queue-drag-hint'),'auto scroll cancel');assert((await client.playerQueue(epoch)).revision===beforeScroll,'auto scroll alone never writes');await capture('autoscroll_cancelled');
  await scrollTop();await choose(ids[0]);
  for(const [width,height,theme] of [[1120,760,'light'],[760,540,'light'],[760,540,'dark'],[1120,760,'dark']]) {
    click(button('关闭播放队列'));await wait(()=>!panel(),'close for theme');await resize(width,height);await appearance(theme);click(button('播放队列'));await ready();await scrollTop();await choose(ids[0]);await choose(ids[2],{ctrlKey:true});await capture(`selection_${width}_${theme}`);
  }
  // Double-click and explicit play remain actual native playback operations.
  await choose(ids[0]);song(ids[0]).dispatchEvent(new MouseEvent('dblclick',{bubbles:true,cancelable:true}));await wait(()=>bar().dataset.playId!==playId&&bar().dataset.state==='playing','double click plays');await prepare();phase='double_click';await capture('double_click_played');
  const current=await client.playerSnapshot(epoch);
  const doubleCurrent=()=>{const control=song(current.entryId);assert(control&&!control.disabled,'current row ready');control.focus({preventScroll:true});control.dispatchEvent(new MouseEvent('dblclick',{bubbles:true,cancelable:true}));};
  assert(song(current.entryId).title.includes('Enter 暂停或继续'),'current row describes both activation methods');
  doubleCurrent();
  await wait(()=>bar().dataset.state==='playing'&&Number(bar().dataset.position)>10.2,'current row resumes and advances');
  let same=await client.playerSnapshot(epoch);
  assert(same.entryId===current.entryId&&same.playId===current.playId&&same.queueRevision===current.queueRevision,'resume keeps native identity');
  await wait(()=>document.activeElement===song(current.entryId),'focus returns after resume');
  doubleCurrent();
  await wait(()=>bar().dataset.state==='paused','current row pauses');
  same=await client.playerSnapshot(epoch);
  assert(same.entryId===current.entryId&&same.playId===current.playId&&same.queueRevision===current.queueRevision&&same.position>10.2,
    'pause keeps native identity and progressed position');
  await wait(()=>document.activeElement===song(current.entryId),'focus returns after pause');
  const seek=one('[aria-label="播放进度"]');seek.value='10';seek.dispatchEvent(new Event('change',{bubbles:true}));
  await wait(()=>Math.abs(Number(bar().dataset.position)-10)<.2&&!seek.disabled,'restore checkpoint position');
  await capture('current_double_click_toggled',{currentTransportPreserved:true});
  const currentSong=()=>song(current.entryId);
  const currentPlayButton=()=>row(current.entryId)?.querySelector('.queue-play');
  const currentLabel=()=>currentPlayButton()?.getAttribute('aria-label')??'';
  const sameCurrent=async ()=>{
    const state=await client.playerSnapshot(epoch);
    assert(state.entryId===current.entryId&&state.playId===current.playId&&
      state.queueRevision===current.queueRevision&&state.position>10.2,
      'row keyboard and inline controls preserve original playback and progress');
  };
  assert(currentLabel().startsWith('继续播放 ')&&hit(currentPlayButton()),'paused current inline control offers visible resume');
  currentSong().focus({preventScroll:true});key(currentSong(),'Enter');
  await wait(()=>bar().dataset.state==='playing'&&Number(bar().dataset.position)>10.45&&
    currentLabel().startsWith('暂停 ')&&document.activeElement===currentSong(),'Enter resumes and keeps the song focus');
  await sameCurrent();
  click(currentPlayButton());
  await wait(()=>bar().dataset.state==='paused'&&currentLabel().startsWith('继续播放 ')&&
    document.activeElement===currentPlayButton(),'inline Pause preserves its focus');
  await sameCurrent();
  click(currentPlayButton());
  await wait(()=>bar().dataset.state==='playing'&&currentLabel().startsWith('暂停 ')&&
    document.activeElement===currentPlayButton(),'inline Resume preserves its focus');
  await sameCurrent();
  currentSong().focus({preventScroll:true});key(currentSong(),'Enter');
  await wait(()=>bar().dataset.state==='paused'&&currentLabel().startsWith('继续播放 ')&&
    document.activeElement===currentSong(),'Enter pauses without selecting again');
  await sameCurrent();
  seek.value='10';seek.dispatchEvent(new Event('change',{bubbles:true}));
  await wait(()=>Math.abs(Number(bar().dataset.position)-10)<.2&&!seek.disabled,'restore checkpoint after explicit controls');
  await capture('current_explicit_transport',{currentTransportPreserved:true,inlineLabel:currentLabel()});
  const playTarget=row(ids[2]).querySelector('.queue-play');click(playTarget);await wait(()=>bar().dataset.playId!==playId&&bar().dataset.state==='playing','explicit cover control plays');await prepare();phase='explicit_play';await capture('explicit_played');
  // A new queue has new entry identities; no selection leaks to replacement.
  await client.playSelection(epoch,{kind:'songs',ids:['1','2','3'],index:1,quality:'standard'});await prepare();phase='replacement';await ready();await wait(()=>!one('.queue-selection button'),'replacement selection cleared');await capture('queue_replaced');
  const replacement=await client.playerQueue(epoch), removed=replacement.current;
  const successor=replacement.tracks[2].entryId;
  await wait(()=>song(removed)&&hit(song(removed)),'paused current row is visible');
  await choose(removed);
  key(song(removed),'Delete');
  await wait(()=>bar().dataset.playId!==playId&&bar().dataset.trackId==='3'&&bar().dataset.state==='playing'&&Number(bar().dataset.position)>.15,'removing paused current starts its successor');
  await ready();
  const successorQueue=await client.playerQueue(epoch);
  assert(successorQueue.current===successor&&successorQueue.tracks.map(item=>item.track.id).join()==='1,3','only current song was removed and successor stays in order');
  await wait(()=>document.activeElement===song(successor),'current deletion focuses its successor');
  phase='current_removed';playId=bar().dataset.playId;
  await capture('paused_current_removed',{successor});
  await emit('complete',{ok:true,queueSelectionUi:true,stageCount:count,realMusicRequests:0,accountWrites:0});
 } catch(error) {await emit('failed',{ok:false,last,count,error:String(error),phase,editTrace,active:document.activeElement?.outerHTML.slice(0,350),body:document.body.textContent.slice(-2500)});}
})();
