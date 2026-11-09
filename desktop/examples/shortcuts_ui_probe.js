// Real production WebView/IPC/player; synthetic account and PCM, DOM key input.
// Native OS key delivery is verified separately by shortcut_smoke.py.
(async () => {
  const one=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)];
  const sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const assert=(v,message)=>{if(!v)throw Error(message);};
  const invoke=(command,args={})=>window.__TAURI_INTERNALS__.invoke(command,args);
  const emit=(stage,rest={})=>invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...rest}});
  const wait=async(test,message)=>{for(let i=0;i<160;i++){if(test())return;await sleep(80);}throw Error(message);};
  const click=node=>{assert(node&&!node.disabled,'unavailable action');node.focus({preventScroll:true});node.click();};
  const button=text=>all('button').find(node=>node.textContent.trim()===text);
  const section=()=>one('.shortcut-settings'),phase=()=>section()?.dataset.phase;
  const refresh=()=>one('[aria-label="重新读取快捷键设置"]');
  const binding=(action,scope='local')=>one(`.shortcut-row[data-action="${action}"] [data-scope="${scope}"] .shortcut-binding`);
  const recorder=()=>one('.shortcut-recorder');
  const state=()=>one('.playerbar')?.dataset.state;
  const box=name=>one(`input[name="shortcuts-${name}"]`);
  const focus=node=>{node.setAttribute('tabindex','-1');node.focus({preventScroll:true});return node;};
  function key(target,code,options={}) {
    const event=new KeyboardEvent('keydown',{code,key:code==='Escape'?'Escape':code==='Tab'?'Tab':code==='Space'?' ':code,keyCode:code==='Escape'?27:0,bubbles:true,cancelable:true,...options});
    target.dispatchEvent(event);return event;
  }
  // Observe the actual injected adapter. A player state event and its DOM
  // update are independent of the shortcut command's eventual reply.
  const calls=[], keyEvents=[];
  let holdNextReply=false, heldReply;
  function observePresses() {
    const provides=one('#app').__vue_app__._context.provides;
    const client=provides[Reflect.ownKeys(provides).find((key)=>typeof key==='symbol'&&key.description==='core-client')];
    const original=client.shortcutPress.bind(client);
    client.shortcutPress=(input)=>{
      const hold=holdNextReply;
      holdNextReply=false;
      const call={keys:[...input.keys],epoch:input.epoch,selection:{...input.selection},native:'pending',reply:'pending',settled:false};
      calls.push(call);
      return (async()=>{
        try {
          await original(input);
          call.native='ok';
          if(hold) await new Promise((release)=>{heldReply=release;});
          call.reply='ok';
        } catch(error) {
          call.native='error';
          call.reply=error?.code??'unknown';
          throw error;
        } finally {
          // The app releases its renderer gate in the caller's .finally.
          // Sample after the promise chain and microtasks have both drained.
          setTimeout(()=>{call.settled=true;},0);
        }
      })();
    };
  }
  function attempt(target,code,options) {
    const before=calls.length;
    const event=key(target,code,options);
    const after=calls.length;
    const receipt={code:event.code,key:event.key,keyCode:event.keyCode,ctrl:event.ctrlKey,
      prevented:event.defaultPrevented,submitted:after-before};
    keyEvents.push(receipt);
    assert(after-before<=1,'one renderer key must issue at most one native command');
    return {receipt,call:calls[before]};
  }
  async function submit(target,code,keys,options={}) {
    let call, attempts=0;
    await wait(()=>{
      const node=target();
      if(document.activeElement!==node||document.querySelector('dialog[open],[aria-modal="true"]'))return false;
      attempts++;
      const sample=attempt(node,code,options);
      if(sample.call) {
        assert(sample.receipt.prevented,'submitted key must be consumed');
        assert(JSON.stringify(sample.call.keys)===JSON.stringify(keys),'exact native chord '+code);
        call=sample.call;
        return true;
      }
      // Retry only a DOM event that neither the listener consumed nor sent.
      assert(!sample.receipt.prevented,'consumed key produced no command: '+code);
      return false;
    },'focused native shortcut: '+code);
    return {call,attempts};
  }
  async function settle(call,expected) {
    await wait(()=>{
      assert(call.reply==='ok'||call.reply==='pending','native shortcut rejected: '+call.reply);
      return call.settled&&state()===expected;
    },'acknowledged shortcut '+call.keys.join('+')+' / '+expected);
    const native=await invoke('player_snapshot',{epoch:call.epoch});
    assert(native.state===expected&&native.playId===call.selection.audioPlayId&&
      native.entryId===call.selection.audioEntryId,'shortcut must affect the same native play');
    return {keys:call.keys,reply:call.reply,nativeState:native.state};
  }
  async function transport(target,code,keys,expected) {
    return settle((await submit(target,code,keys)).call,expected);
  }
  async function ready(changeable=true){await wait(()=>phase()==='ready'&&binding('play_pause')&&(!changeable||!binding('play_pause').disabled)&&one('[data-action="play_pause"] [data-scope="local"] .shortcut-result')?.dataset.state==='active','local listener ready');}
  async function settings(){click(one('[aria-label="打开设置"]'));await wait(()=>section()&&!refresh().disabled,'settings loaded');await ready();}
  async function play(){click(button('每日推荐'));await wait(()=>one('.track-row .track-play')&&!one('.track-row .track-play').disabled,'native player ready');one('.track-row').dispatchEvent(new MouseEvent('dblclick',{bubbles:true}));await wait(()=>state()==='playing'&&Number(one('.playerbar').dataset.position)>.15,'native PCM');}
  async function theme(label){click(one('[aria-label="外观主题"]'));await wait(()=>one('.action-menu'),'theme menu');click(button(label));await wait(()=>!one('.action-menu'),'theme changed');}
  async function record(action,scope='local'){click(binding(action,scope));await wait(()=>phase()==='recording'&&recorder()&&!recorder().disabled,'recording acknowledged');await wait(()=>document.activeElement===recorder(),'recorder focus');}
  async function chord(code,expected,options={}){key(recorder(),code,options);await wait(()=>recorder()?.value===expected,'captured chord '+expected);}
  async function save(){click(button('保存'));await wait(()=>!recorder(),'saved editor closed');await ready();}
  async function cancel(){click(button('取消'));await wait(()=>!recorder(),'cancelled editor');await ready(false);}
  async function fixture(kind){window.__shortcutFixtureAck=null;await emit('shortcut-fixture',{kind});await wait(()=>window.__shortcutFixtureAck===kind,'isolated fixture ack');}
  async function toggle(name,value){assert(box(name).checked!==value,'switch must change');click(box(name));await wait(()=>box(name)?.checked===value&&!box(name).disabled,'switch saved');await ready();}
  let count=Number(sessionStorage.getItem('cumulus.shortcut-reload-count')||0);
  async function capture(label,target=refresh(),extra={}){
    target.scrollIntoView({block:'center',behavior:'instant'});await sleep(350);
    assert(scrollY===0&&document.documentElement.scrollWidth<=innerWidth,'frame overflow');
    const r=target.getBoundingClientRect();assert(r.x>=0&&r.right<=innerWidth+1&&r.y>=0&&r.bottom<=innerHeight+1,'control clipped');
    for(const cell of all('.shortcut-cell')) assert(cell.scrollWidth<=cell.clientWidth+1,'shortcut cell overflow');
    await emit(`ux_${String(++count).padStart(2,'0')}_${label}`,{theme:document.documentElement.dataset.theme,phase:phase(),playerState:state(),globalEnabled:box('global')?.checked??null,mediaEnabled:box('media')?.checked??null,focused:document.activeElement?.getAttribute('aria-label'),...extra});await sleep(850);
  }
  async function finish(reopened=false){await emit('complete',{ok:true,stageCount:count,reopened,realMusicRequests:0,domKeyboardInput:true,physicalAudioOutputVerified:false});}
  try{
    await wait(()=>one('.track-row .track-play')&&!one('.track-row .track-play').disabled,'initial UI');
    observePresses();
    if(sessionStorage.getItem('cumulus.shortcut-reload-count')){
      await settings();assert(!box('global').checked&&!box('media').checked,'reload switches');
      assert(binding('play_pause').textContent.trim()==='O','reload binding');
      assert(state()==='paused','reload changed actual player');
      await capture('reloaded_recording_released');
      sessionStorage.removeItem('cumulus.shortcut-reload-count');localStorage.setItem('cumulus.shortcut-finished','1');await finish();return;
    }
    if(localStorage.getItem('cumulus.shortcut-finished')){
      await settings();assert(!box('global').checked&&!box('media').checked&&binding('play_pause').textContent.trim()==='O','restart persistence');
      assert(document.documentElement.dataset.theme==='dark','theme restart');await capture('reopened_preferences');
      await play();await settings();
      await transport(()=>focus(section().querySelector('h2')),'KeyO',[79],'paused');
      await capture('reopened_native_pause');await finish(true);return;
    }
    await theme('浅色');await settings();await capture('default_light');
    await play();await settings();const header=()=>focus(section().querySelector('h2'));
    // Hold only the first *real* IPC result before returning it to the
    // application. The native pause event already reached the player bar.
    // This deterministically recreates the prior F5 timing without replaying
    // any submitted command or replacing the native player's result.
    holdNextReply=true;
    const first=await submit(header,'KeyP',[17,80],{ctrlKey:true});
    await wait(()=>heldReply&&first.call.native==='ok'&&state()==='paused','native pause arrived before renderer acknowledgement');
    const heldF5=attempt(header(),'F5',{});
    assert(heldF5.receipt.prevented&&heldF5.receipt.submitted===0&&!heldF5.call,
      'busy renderer must consume F5 without a second IPC');
    heldReply();heldReply=undefined;
    const shortcutSequence=[
      await settle(first.call,'paused'),
      await transport(header,'F5',[116],'playing'),
      await transport(header,'Space',[32],'paused'),
    ];
    await capture('local_native_transport',refresh(),{
      ctrlPAttempts:first.attempts,heldF5:heldF5.receipt,shortcutSequence,
    });
    const search=one('.search-box input')||one('header input');search.focus();assert(!key(search,'KeyP',{ctrlKey:true}).defaultPrevented,'typing consumed');
    click(one('[aria-label="均衡器"]'));await wait(()=>one('.equalizer-dialog[open]'),'modal opened');
    assert(!key(focus(one('.equalizer-dialog h2')),'KeyP',{ctrlKey:true}).defaultPrevented,'modal shortcut consumed');await sleep(250);assert(state()==='paused','protected UI changed playback');
    click(one('[aria-label="关闭均衡器"]'));await wait(()=>!one('.equalizer-dialog'),'modal closed');await capture('protected_input_and_modal');
    await record('play_pause');await chord('KeyO','O');await capture('record_local',recorder());await save();
    assert(binding('play_pause').textContent.trim()==='O','new binding');
    assert(!key(header(),'KeyP',{ctrlKey:true}).defaultPrevented,'old chord still captured');await sleep(200);assert(state()==='paused','old chord executed');
    await transport(header,'KeyO',[79],'playing');await transport(header,'KeyO',[79],'paused');await capture('rebound_native_transport');
    await record('play_pause','global');await chord('KeyQ','Ctrl + Alt + Q');await capture('global_normalized',recorder());await save();
    await record('play_pause');await chord('ArrowLeft','Ctrl + ←',{ctrlKey:true});click(button('保存'));
    await wait(()=>section().querySelector('.catalog-error')&&recorder()&&!recorder().disabled,'duplicate rejected');assert(state()==='paused','conflict changed playback');await capture('duplicate_rejected',recorder());
    await cancel();click(refresh());await ready();await wait(()=>!section().querySelector('.catalog-error'),'conflict refreshed');
    await record('play_pause');await chord('Backspace','未设置');key(recorder(),'Escape');await wait(()=>!recorder(),'Escape cancellation');await ready();assert(binding('play_pause').textContent.trim()==='O','cancel saved empty chord');await capture('clear_cancelled');
    await theme('深色');await emit('resize',{width:760,height:540});await wait(()=>innerWidth===760,'small window');await capture('small_dark',binding('love'));
    await fixture('shortcut_reject');click(box('global'));await wait(()=>section().querySelector('.catalog-error')&&!refresh().disabled,'save failure');
    assert(box('global').checked,'save failure lied');await wait(()=>document.activeElement===refresh(),'failure focus recovery');await capture('save_failed');
    const errorRect=section().querySelector('.catalog-error').getBoundingClientRect();assert(errorRect.y>=0&&errorRect.bottom<=innerHeight-80,'save failure explanation clipped');
    await fixture('shortcut_allow');click(refresh());await ready();await wait(()=>!section().querySelector('.catalog-error'),'read recovery');await capture('read_recovered');
    // Exercise the registration-event/save-result ordering more than once:
    // a former focus watcher could supersede the pending action's recovery.
    for(let i=0;i<2;i++){
      await fixture('shortcut_reject');click(box('global'));
      await wait(()=>section().querySelector('.catalog-error')&&!refresh().disabled,'repeated save failure');
      await wait(()=>document.activeElement===refresh(),'repeated failure focus');
      await fixture('shortcut_allow');click(refresh());await ready();
      await wait(()=>!section().querySelector('.catalog-error'),'repeated recovery');
    }
    await fixture('shortcut_damage');click(refresh());await wait(()=>phase()==='damaged'&&!box('global'),'malformed JSON');await capture('damaged');
    click(button('恢复默认快捷键设置'));await ready();await wait(()=>document.activeElement===refresh(),'repair focus');assert(binding('play_pause').textContent.trim()==='Ctrl + P','repair defaults');await capture('repaired');
    await record('play_pause');await chord('KeyO','O');await save();await toggle('global',false);await toggle('media',false);
    await transport(header,'KeyO',[79],'playing');await transport(header,'KeyO',[79],'paused');await capture('saved_separate_switches');
    await record('play_pause');await chord('KeyQ','Q');await capture('record_before_reload',recorder());
    sessionStorage.setItem('cumulus.shortcut-reload-count',String(count));location.reload();
  }catch(error){
    heldReply?.();
    await emit('failed',{ok:false,error:String(error),stageCount:count,phase:phase(),playerState:state(),
      focused:document.activeElement?.outerHTML?.slice(0,400),keyEvents:keyEvents.slice(-16),
      shortcutCalls:calls.slice(-12).map(({keys,native,reply,settled})=>({keys,native,reply,settled}))});
  }
})();
