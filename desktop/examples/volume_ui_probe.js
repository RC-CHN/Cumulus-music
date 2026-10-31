// Production volume controls; synthetic library, native persistence and libmpv/null.
(async()=>{
 const one=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)],sleep=ms=>new Promise(r=>setTimeout(r,ms));
 const invoke=(...args)=>(window.__CUMULUS_PROOF_INVOKE__??window.__TAURI_INTERNALS__.invoke)(...args);
 const emit=(stage,value={})=>invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...value}});
 const assert=(ok,m)=>{if(!ok)throw Error(m);};
 const wait=async(f,m)=>{for(let i=0;i<240;i++){if(f())return;await sleep(60);}throw Error(m);};
 const bar=()=>one('.playerbar'),trigger=()=>one('.volume-trigger'),popup=()=>one('.volume-popover:not(.menu-leave-active)'),slider=()=>popup()?.querySelector('input');
 const click=n=>{assert(n&&!n.disabled,'enabled control');n.focus({preventScroll:true});n.click();};
 const key=(n,k)=>n.dispatchEvent(new KeyboardEvent('keydown',{key:k,bubbles:true,cancelable:true}));
 const rect=n=>{const r=n.getBoundingClientRect();return {left:r.left,top:r.top,right:r.right,bottom:r.bottom,width:r.width,height:r.height};};
 const hit=n=>{const r=rect(n),top=document.elementFromPoint((r.left+r.right)/2,(r.top+r.bottom)/2);return top===n||n.contains(top);};
 const button=text=>all('button').find(n=>n.textContent.trim()===text);
 let count=0,last='startup',playId;
 async function open(){if(document.activeElement!==slider())document.activeElement?.blur();one('.volume-control').dispatchEvent(new PointerEvent('pointerenter'));await wait(()=>slider(),'hover opens slider');await sleep(140);}
 async function close(){if(popup())key(slider()??trigger(),'Escape');await wait(()=>!one('.volume-popover'),'volume closed');assert(document.activeElement===trigger(),'Escape returns to icon');}
 async function set(value){if(!slider())await open();slider().focus({preventScroll:true});slider().value=String(value);slider().dispatchEvent(new Event('input',{bubbles:true}));await wait(()=>Number(bar().dataset.volume)===value,'native volume '+value);await sleep(80);}
 async function fixture(kind){window.__volumeFixtureAck=null;await emit('volume-fixture',{kind});await wait(()=>window.__volumeFixtureAck===kind,'volume fixture');}
 async function prepare(){
  if(bar().dataset.state!=='paused'){
   if(bar().dataset.state!=='playing')click(one('.track-row .track-play:not(:disabled)'));
   await wait(()=>bar().dataset.state==='playing'&&Number(bar().dataset.position)>.2,'native playback');click(one('.playerbar [aria-label="暂停"]'));await wait(()=>bar().dataset.state==='paused','native pause');
  }
  const seek=one('[aria-label="播放进度"]');seek.value='10';seek.dispatchEvent(new Event('change',{bubbles:true}));await wait(()=>Math.abs(Number(bar().dataset.position)-10)<.2&&!seek.disabled,'paused seek');playId=bar().dataset.playId;
 }
 async function capture(label){
  last=label;await sleep(300);
  assert(document.documentElement.scrollWidth<=innerWidth,'document overflow');assert(bar().dataset.state==='paused'&&Math.abs(Number(bar().dataset.position)-10)<.2,'transport unchanged');assert(bar().dataset.playId===playId,'volume/mode does not reload audio');assert(!one('.playback-message'),'no generic audio error or transport busy');assert(hit(trigger()),'volume icon reachable');
  const p=popup(),r=p&&rect(p),s=slider();
  if(p){assert(r.left>=8&&r.top>=8&&r.right<=innerWidth-8&&r.bottom<=innerHeight-8,'volume surface contained');assert(hit(s),'slider hit');assert(s.getAttribute('aria-orientation')==='vertical','vertical semantics');assert(Math.abs(rect(s).width-20)<.2&&Math.abs(rect(s).height-88)<.2,'native 88px vertical track');if(!p.classList.contains('volume-error'))assert(r.width===38&&r.height===132,'official volume surface dimensions');}
  await emit(`ux_${String(++count).padStart(2,'0')}_${label}`,{width:innerWidth,height:innerHeight,ratio:devicePixelRatio,screenX:window.screenX,screenY:window.screenY,visible:document.visibilityState==='visible',focused:document.hasFocus(),theme:document.documentElement.dataset.theme,volume:Number(bar().dataset.volume),unmuteVolume:Number(bar().dataset.unmuteVolume),mode:bar().dataset.mode,playId,position:Number(bar().dataset.position),contained:true,triggerHit:true,popup:p?{...r,displayed:Number(s.value),text:p.textContent,focusInside:p.contains(document.activeElement),sliderHit:hit(s),error:!!p.querySelector('[role="alert"]')}:null,paintSamples:r?[{x:r.left+5,y:r.top+8,color:getComputedStyle(p).backgroundColor}]:[]});
 }
 try{
  await wait(()=>trigger()&&!trigger().disabled&&one('.track-row .track-play:not(:disabled)'),'ready');
  if(window.__volumeReopen){
   await wait(()=>Number(bar().dataset.volume)===0&&Number(bar().dataset.unmuteVolume)===27,'durable muted value');await prepare();await open();await capture('reopened_muted');click(trigger());await wait(()=>Number(bar().dataset.volume)===27,'reopened unmute');await capture('reopened_unmuted');click(trigger());await wait(()=>Number(bar().dataset.volume)===0,'reopened remute');await capture('reopened_remuted');
  }else{
   await prepare();trigger().focus({preventScroll:true});await wait(()=>slider(),'first keyboard focus opens volume');await close();one('[aria-label="外观主题"]').focus();trigger().focus({preventScroll:true});await wait(()=>slider(),'returning keyboard focus reopens volume');await close();
   for(const [w,h,theme] of [[1120,760,'light'],[760,540,'light'],[760,540,'dark'],[1120,760,'dark']]){
    await emit('resize',{width:w,height:h});await sleep(240);if(innerWidth!==w||innerHeight!==h)await emit('resize',{width:w*w/innerWidth,height:h*h/innerHeight});await wait(()=>innerWidth===w&&innerHeight===h,'resize');click(one('[aria-label="外观主题"]'));await wait(()=>one('.action-menu'),'theme menu');click(button(theme==='dark'?'深色':'浅色'));await wait(()=>document.documentElement.dataset.theme===theme&&!one('.action-menu'),'theme applied');
    const suffix=`${w}_${theme}`;await open();await capture(`hover_${suffix}`);await set(37);await capture(`changed_${suffix}`);click(trigger());await wait(()=>Number(bar().dataset.volume)===0,'muted');await capture(`muted_${suffix}`);click(trigger());await wait(()=>Number(bar().dataset.volume)===37,'unmuted exact previous');await capture(`unmuted_${suffix}`);await close();key(trigger(),'ArrowUp');await wait(()=>document.activeElement===slider(),'keyboard slider focus');await capture(`keyboard_${suffix}`);await close();
   }
   for(const mode of ['single_loop','random','sequential','order_loop']){click(one('.transport-controls button'));await wait(()=>bar().dataset.mode===mode,'native mode '+mode);await capture('mode_'+mode);}
   await set(100);await capture('maximum');for(let i=0;i<2;i++)popup().dispatchEvent(new WheelEvent('wheel',{deltaY:120,bubbles:true,cancelable:true}));await wait(()=>Number(bar().dataset.volume)===94,'3 percent per wheel event');await capture('wheel');await set(0);await capture('zero');popup().dispatchEvent(new WheelEvent('wheel',{deltaY:-120,bubbles:true,cancelable:true}));await wait(()=>Number(bar().dataset.volume)===3,'wheel from zero');await capture('wheel_up');
   const provides=one('#app').__vue_app__._context.provides;const client=provides[Reflect.ownKeys(provides).find(k=>typeof k==='symbol'&&k.description==='core-client')];const currentEpoch=(await invoke('account_snapshot')).epoch;
   await invoke('player_action',{epoch:currentEpoch,action:{kind:'volume',value:63}});await invoke('player_action',{epoch:currentEpoch,action:{kind:'volume',value:0}});await wait(()=>Number(bar().dataset.volume)===0&&Number(bar().dataset.unmuteVolume)===63,'external volume update');await capture('external_muted');click(trigger());await wait(()=>Number(bar().dataset.volume)===63,'external unmute');await capture('external_unmuted');
   const original=client.playerAction;let release,held=true,writes=0;
   client.playerAction=async(...args)=>{if(args[1].kind==='volume'){writes++;if(held){held=false;await new Promise(r=>release=r);}}return original(...args);};
   slider().focus();slider().dispatchEvent(new PointerEvent('pointerdown',{bubbles:true}));for(const value of [28,29,31]){slider().value=String(value);slider().dispatchEvent(new Event('input',{bubbles:true}));}await wait(()=>release,'held first volume');assert(writes===1&&Number(slider().value)===31,'serialized draft');await capture('pending');release();await wait(()=>Number(bar().dataset.volume)===31,'coalesced latest target');slider().dispatchEvent(new PointerEvent('pointerup',{bubbles:true}));assert(writes===2,'only first and latest writes');client.playerAction=original;await capture('coalesced');
   await fixture('reject');slider().focus();slider().value='75';slider().dispatchEvent(new Event('input',{bubbles:true}));await wait(()=>popup()?.querySelector('[role="alert"]'),'local volume failure');assert(Number(bar().dataset.volume)===31&&Number(bar().dataset.unmuteVolume)===31,'failed preference rolled back');assert(document.activeElement===slider(),'failure preserves slider focus');await capture('save_failed');await fixture('allow');click(button('重试音量'));await wait(()=>Number(bar().dataset.volume)===75&&!popup()?.classList.contains('volume-error'),'absolute retry');assert(document.activeElement===slider(),'retry disappearing restores slider focus');await capture('save_retried');await set(27);click(trigger());await wait(()=>Number(bar().dataset.volume)===0&&Number(bar().dataset.unmuteVolume)===27,'persisted muted state');await close();await capture('persist_muted');
  }
  await emit('complete',{ok:true,stageCount:count,reopened:!!window.__volumeReopen,volumeUiVerified:true,initialKeyboardEntryVerified:!window.__volumeReopen,keyboardReentryVerified:!window.__volumeReopen,realMusicRequests:0,accountWrites:0});
 }catch(error){await emit('failed',{ok:false,error:String(error),last,stageCount:count,active:document.activeElement?.outerHTML.slice(0,500),body:document.body.textContent.slice(-1200)});}
})();
