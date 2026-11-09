(async()=>{
  const sleep=ms=>new Promise(r=>setTimeout(r,ms)),all=s=>[...document.querySelectorAll(s)],one=s=>document.querySelector(s);
  const assert=(ok,message)=>{if(!ok)throw Error(message);};
  const emit=(stage,rest={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...rest}});
  const wait=async(test,label)=>{for(let i=0;i<180;i++){if(test())return;await sleep(100);}throw Error(label);};
  const button=text=>all('.relay-settings button').find(n=>n.textContent.trim()===text);
  const click=n=>{assert(n&&!n.disabled,'button unavailable');n.scrollIntoView({block:'nearest'});n.focus({preventScroll:true});n.click();};
  const ready=()=>one('.relay-settings')?.getAttribute('aria-busy')==='false';
  const state=()=>one('.playerbar')?.dataset.state;
  const position=()=>Number(one('.playerbar')?.dataset.position??0);
  const detail=()=>one('.lyrics-panel:not([inert])');
  const detailTitle=()=>detail()?.querySelector('.vinyl-info h1')?.textContent.trim()??null;
  async function closeDetail(){
    click(one('[aria-label="收起播放详情"]'));
    await wait(()=>!detail(),'handoff detail closes without stopping playback');
    assert(state()==='playing','closing handoff detail interrupted playback');
  }
  let contentionSequence=0,contentionVerified=false,unsentVerified=false;
  window.__relayContentionAck={};
  async function contention(action){
    const id=++contentionSequence;await emit('relay_contention',{id,action});
    await wait(()=>window.__relayContentionAck[id],'native contention receipt');
    const value=window.__relayContentionAck[id];assert(!value.error,value.error);return value;
  }
  // Bounded synthetic-only event trace for cross-WebView focus failures.
  const focusTrace=[],windowFocusReceipts=[];
  window.__relayWindowFocusAck={};
  async function focusWindow(outside,label){
    const id=windowFocusReceipts.length+1;
    await emit('window_focus',{outside,id});
    await wait(()=>window.__relayWindowFocusAck[id],'native relay focus receipt');
    const receipt=window.__relayWindowFocusAck[id];
    windowFocusReceipts.push(receipt);
    assert(receipt.id===id&&receipt.outside===outside&&!receipt.error,receipt.error||'stale native relay focus receipt');
    assert(receipt.mainForeground===!outside&&(!outside||receipt.helperForeground===true),'native relay foreground mismatch');
    await wait(()=>document.hasFocus()===!outside,label);
  }
  // A fading prompt is still connected, but must already reject keyboard and
  // pointer interaction. Test the real WebView behavior during that interval.
  const leavingFocusChecks=[],seenLeaving=new WeakSet();
  const leavingObserver=new MutationObserver(()=>{
    const node=one('.relay-prompt-leave-active');
    if(!node||seenLeaving.has(node))return;
    seenLeaving.add(node);
    const button=node.querySelector('.relay-continue');
    button?.focus({preventScroll:true});
    const continueBlocked=document.activeElement!==button;
    const close=node.querySelector('.relay-close');close?.focus({preventScroll:true});
    leavingFocusChecks.push({inert:node.inert,focusBlocked:continueBlocked&&document.activeElement!==close});
  });
  leavingObserver.observe(document.documentElement,{subtree:true,attributes:true,attributeFilter:['class','inert']});
  const describe=node=>node instanceof Element?`${node.tagName}.${node.className}`:String(node);
  const focusState=()=>({active:describe(document.activeElement),documentFocused:document.hasFocus(),prompt:!!one('.relay-prompt'),promptFocused:!!one('.relay-prompt')?.contains(document.activeElement),hovered:!!one('.relay-prompt')?.matches(':hover'),disabled:one('.relay-continue')?.disabled,settings:!!one('.settings-page'),relaySettings:!!one('.relay-settings'),relayBusy:one('.relay-settings')?.getAttribute('aria-busy'),privacyFields:all('input[name="shareSetting"]').length});
  for(const event of ['focusin','focusout','pointerenter','pointerleave']) document.addEventListener(event,e=>{
    if(e.target instanceof Element && (e.target.closest('.relay-prompt')||event.startsWith('focus'))){
      focusTrace.push({at:performance.now(),event,target:describe(e.target),related:describe(e.relatedTarget),...focusState()});
      if(focusTrace.length>60)focusTrace.shift();
    }
  },true);
  const geometry=()=>({
    viewport:{width:innerWidth,height:innerHeight,x:scrollX,y:scrollY},
    boxes:['.relay-prompt','.playerbar','.app-shell','.settings-scroll'].map(selector=>{
      const node=one(selector);return node?{selector,rect:node.getBoundingClientRect().toJSON(),scrollTop:node.scrollTop,scrollHeight:node.scrollHeight,clientHeight:node.clientHeight,transform:getComputedStyle(node).transform}:null;
    }),
  });
  let stageCount=0;
  async function capture(name,playing=false){
    await sleep(300);assert(scrollY===0&&scrollX===0,'desktop document scrolled behind fixed overlays');assert(document.documentElement.scrollWidth<=innerWidth,'horizontal overflow');assert(!document.body.textContent.includes('SYNTHETIC_PRIVATE'),'native alert leaked');
    const card=one('.relay-prompt')?.getBoundingClientRect(),bar=one('.playerbar').getBoundingClientRect(),scroll=one('.settings-scroll')?.getBoundingClientRect();
    if(card)assert(card.left>=0&&card.right<=innerWidth&&card.bottom<=bar.top-3,'relay prompt overlaps player or window');
    if(card){const action=one('.relay-continue');assert(action.scrollWidth<=action.clientWidth,'default relay action text clipped');}
    if(scroll)assert(scroll.bottom<=bar.top+1,'settings content overlaps player');
    await emit(`ux_${String(++stageCount).padStart(2,'0')}_${name}`,{geometry:geometry(),width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,prompt:!!card,promptBottom:card?.bottom,playerTop:bar.top,position:position(),playing,detailTitle:detailTitle(),dialog:!!one('dialog[open]'),status:one('.relay-settings [role="status"]')?.textContent,operations:all('.relay-operation').map(n=>n.textContent.trim())});await sleep(850);
  }
  async function theme(value){click(one(`input[name="settings-theme"][value="${value}"]`));await wait(()=>document.documentElement.dataset.theme===value,'theme');}
  async function resize(width,height){await emit('resize',{width,height});await sleep(180);if(innerWidth!==width||innerHeight!==height)await emit('resize',{width:width*width/innerWidth,height:height*height/innerHeight});await wait(()=>innerWidth===width&&innerHeight===height,'small window');}
  async function refresh(mode){await emit('fixture',{mode});click(button('检查其他设备'));await sleep(120);await wait(ready,'relay refresh');}
  try{
    await wait(()=>one('.relay-prompt')&&one('.track-row'),'initial relay offer');await capture('initial_light');
    click(one('[aria-label="打开设置"]'));await wait(()=>ready()&&one('input[name="shareSetting"]'),'settings');await theme('light');await capture('settings_light');
    await theme('dark');await refresh('song');await capture('prompt_dark');
    await resize(760,540);await capture('small_dark');await theme('light');await capture('small_light');
    // Rendering/theme/resize captures may consume the original offer's whole
    // eight-second lifetime on a slower runner. Start focus-hold checks on a
    // newly mounted offer, never on a node that is already leaving.
    await refresh('empty');await wait(()=>!one('.relay-prompt'),'old prompt removed');
    assert(leavingFocusChecks.length>0&&leavingFocusChecks.every(v=>v.inert&&v.focusBlocked),'leaving prompt must be inert and reject focus');
    await refresh('song');await wait(()=>one('.relay-prompt:not([inert]):not(.relay-prompt-leave-active)')&&!one('.relay-continue')?.disabled,'fresh interactive prompt');
    // Real pointer movement: runner cursor placement after resize is not an
    // idle state. Hover and keyboard focus must independently hold the offer.
    const rect=one('.relay-prompt').getBoundingClientRect();
    await emit('pointer',{x:rect.left+10,y:rect.top+10});await wait(()=>one('.relay-prompt')?.matches(':hover'),'pointer entered prompt');
    one('.relay-continue').focus({preventScroll:true});
    assert(one('.relay-prompt').contains(document.activeElement),'continue button did not take focus');
    await emit('pointer',{x:4,y:4});await wait(()=>!one('.relay-prompt')?.matches(':hover'),'pointer left prompt');
    // Window/document blur can omit relatedTarget while the DOM retains its
    // focused control. Exercise that event boundary independently of the OS.
    one('.relay-continue').dispatchEvent(new FocusEvent('focusout',{bubbles:true,relatedTarget:null}));
    await sleep(8500);assert(one('.relay-prompt')?.contains(document.activeElement),'keyboard focus must prevent auto-dismiss after pointer leaves');
    await focusWindow(true,'native window lost focus');
    await sleep(8500);assert(one('.relay-prompt')?.contains(document.activeElement),'window blur must preserve the focused prompt');
    await focusWindow(false,'native window regained focus');
    assert(one('.relay-prompt')?.contains(document.activeElement),'window activation lost the focused prompt');
    await emit('pointer',{x:rect.left+10,y:rect.top+10});await wait(()=>one('.relay-prompt')?.matches(':hover'),'pointer reentered prompt');
    one('[aria-label="打开设置"]').focus({preventScroll:true});
    await sleep(8500);assert(one('.relay-prompt')?.matches(':hover'),'hover must prevent auto-dismiss after focus leaves');
    await emit('pointer',{x:4,y:4});await wait(()=>!one('.relay-prompt')?.matches(':hover'),'pointer released');
    await wait(()=>!one('.relay-prompt')&&ready(),'8 second auto-dismiss acknowledgement');await capture('auto_dismiss');
    one('.relay-settings').scrollIntoView({block:'center'});await refresh('empty');assert(one('.relay-settings').textContent.includes('暂时没有'),'empty device state');await capture('empty');
    await refresh('excluded');assert(one('.relay-settings').textContent.includes('尚未向当前账号开放'),'experiment excluded');await capture('excluded');
    await refresh('disabled');assert(one('.relay-settings').textContent.includes('未开启'),'server disabled');await capture('disabled');
    await refresh('offline');assert(one('.relay-error'),'offline message');await capture('offline');
    await refresh('rejected');click(button('继续播放'));await sleep(150);await wait(ready,'rejected pull');assert(one('.relay-operation').textContent.includes('拒绝'),'rejected outcome');await capture('rejected');
    await refresh('lost');click(button('继续播放'));await sleep(150);await wait(ready,'lost pull');assert(all('.relay-operation').some(n=>n.textContent.includes('未自动重试')),'lost response not explained');await capture('lost_pull');
    await refresh('permission');click(button('继续播放'));await sleep(150);await wait(()=>ready()&&button('重新准备播放'),'permission failure');await capture('permission_failure');
    await emit('fixture',{mode:'song'});click(button('重新准备播放'));await sleep(150);await wait(()=>ready()&&state()==='playing'&&position()>0.2&&detailTitle(),'music playback detail');assert(position()<10,'song did not restart');await capture('music_playing',true);await closeDetail();click(one('[aria-label="停止播放"]'));await wait(()=>state()==='idle'&&!one('[aria-label="停止播放"]')?.disabled,'music stop acknowledgement');
    await refresh('voice');await capture('voice_offer');
    if(window.__CUMULUS_PUBLICATION_PROOF__){
      const held=await contention('hold');assert(held.held&&held.preparing.length===0,'timeout fixture not held');
      click(button('继续播放'));await sleep(100);await wait(ready,'bounded local contention timeout');
      const after=await contention('inspect');
      assert(after.held&&after.preparing.length===0&&after.pulls===held.pulls,'timeout sent a pull or retained a preparing record');
      assert(all('.relay-operation').some(n=>n.textContent.includes('尚未发送接续请求')),'unsent timeout was presented as an unknown remote write');
      await capture('not_sent');assert(!(await contention('release')).held,'timeout fixture not released');
      await refresh('voice');unsentVerified=true;
    }
    const held=window.__CUMULUS_PUBLICATION_PROOF__?await contention('hold'):null;
    if(held)assert(held.held&&held.preparing.length===0,'synthetic publisher lock not held');
    click(button('继续播放'));
    if(held){
      let preparing;for(let i=0;i<10;i++){preparing=await contention('inspect');if(preparing.preparing.length===1)break;await sleep(25);}
      assert(preparing.held&&preparing.preparing.length===1&&preparing.pulls===held.pulls&&!ready(),'continuation did not wait before sending');
      assert(!(await contention('release')).held,'synthetic publisher lock not released');contentionVerified=true;
    }
    await sleep(150);await wait(()=>ready()&&state()==='playing'&&position()>=12.5&&detailTitle(),'voice resume detail');
    if(window.__CUMULUS_PUBLICATION_PROOF__){const slider=one('[aria-label="播放进度"]');slider.value='18';slider.dispatchEvent(new Event('input',{bubbles:true}));slider.dispatchEvent(new Event('change',{bubbles:true}));await wait(()=>position()>=18,'native voice seek publication');}
    await capture('voice_playing',true);await closeDetail();click(one('[aria-label="停止播放"]'));await wait(()=>state()==='idle'&&!one('[aria-label="停止播放"]')?.disabled,'voice stop acknowledgement');
    await refresh('song');click(one('input[name="personalServiceSetting"][value="2"]'));await wait(()=>one('dialog[open]')&&!one('.relay-prompt'),'modal suppression');await capture('modal_suppression');click(all('dialog[open] button').find(n=>n.textContent.trim()==='取消'));await wait(()=>!one('dialog[open]')&&one('.relay-prompt'),'prompt restored');await capture('modal_restored');
    click(one('[aria-label="关闭接续提示"]'));await wait(()=>!one('.relay-prompt'),'close prompt');assert(document.activeElement===one('[aria-label="打开设置"]'),'close focus did not return');await capture('closed_focus');
    assert(leavingFocusChecks.every(v=>v.inert&&v.focusBlocked),'leaving prompt later accepted focus');
    await emit('complete',{ok:true,stageCount,realMusicRequests:0,syntheticQueueApplications:2,playbackVerified:true,publicationLockContention:contentionVerified,unsentTimeout:unsentVerified,decoder:'libmpv',audioOutput:'null',mediaSubstitution:'exact synthetic CDN URL to generated PCM',autoDismiss:true,focusHoldAfterPointerLeave:true,hoverHoldAfterFocusLeave:true,nativeWindowFocusHold:true,nullRelatedFocusOutInjected:true,freshOfferBeforeFocusHold:true,leavingFocusChecks,focusTrace,windowFocusReceipts});
  }catch(error){await emit('failed',{ok:false,error:String(error),stageCount,geometry:geometry(),focusState:focusState(),focusTrace,leavingFocusChecks,windowFocusReceipts});}
  finally{leavingObserver.disconnect();}
})();
