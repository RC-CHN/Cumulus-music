// Production speed menu, keyboard focus and real libmpv/null audio.
(async () => {
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const one = selector => document.querySelector(selector);
  const all = selector => [...document.querySelectorAll(selector)];
  const assert = (value, label) => { if (!value) throw Error(label); };
  const wait = async (test, label) => { for (let i=0;i<180;i++) { if (test()) return; await sleep(60); } throw Error(label); };
  const invoke = (...args) => (window.__CUMULUS_PROOF_INVOKE__ ?? window.__TAURI_INTERNALS__.invoke)(...args);
  const emit = (stage, value={}) => invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...value}});
  const button = () => one('.playerbar [aria-label="播放速度"]');
  const panel = () => one('.speed-action-menu:not(.menu-leave-active)');
  const bar = () => one('.playerbar');
  const click = node => { assert(node&&!node.disabled,'control unavailable'); node.focus({preventScroll:true});node.click(); };
  const key = value => document.activeElement.dispatchEvent(new KeyboardEvent('keydown',{key:value,bubbles:true,cancelable:true}));
  const rect = node => { const r=node.getBoundingClientRect();return {left:r.left,top:r.top,right:r.right,bottom:r.bottom,width:r.width,height:r.height}; };
  const hit = node => { const r=rect(node),target=document.elementFromPoint(r.left+r.width/2,r.top+r.height/2);return node===target||node.contains(target); };
  let count=0,seekDiagnostic=null;
  const speeds=[],nativeSeeks=[];
  const pause = () => bar()?.querySelector('[aria-label="暂停"]');
  const range = () => bar()?.querySelector('input[type="range"][aria-label="播放进度"]');
  function observeSeek(input=range()) {
    const player=bar()?.dataset;
    return {playId:player?.playId,trackId:player?.trackId,state:player?.state,
      position:Number(player?.position),message:one('.playback-message')?.textContent.trim()??'',
      input:input?{connected:input.isConnected,disabled:input.disabled,min:Number(input.min),
        max:Number(input.max),value:Number(input.value)}:null};
  }
  async function pauseCurrent(playId,label) {
    const canPause=()=>!!playId&&bar()?.dataset.playId===playId&&bar().dataset.state==='playing'&&
      pause()?.isConnected&&!pause().disabled;
    seekDiagnostic={label,playId,initial:observeSeek()};
    await wait(canPause,`${label} control ready`);
    assert(canPause(),'pause control changed before dispatch');
    click(pause());
    await wait(()=>bar()?.dataset.playId===playId&&bar().dataset.state==='paused',label);
  }
  async function seekPaused(seconds,playId,label) {
    const trackId=bar()?.dataset.trackId;
    const ready=(input=range())=>!!playId&&!!trackId&&bar()?.dataset.playId===playId&&
      bar().dataset.trackId===trackId&&bar().dataset.state==='paused'&&input?.isConnected&&
      !input.disabled&&Number(input.min)<=seconds&&Number(input.max)>=seconds;
    seekDiagnostic={label,seconds,playId,trackId,initial:observeSeek()};
    // The native pause/seek event may precede the renderer's release of the
    // busy state. Wait for the current range before each one-shot action.
    await wait(()=>ready(),`${label} control ready`);
    const input=range();
    assert(ready(input),'seek control changed before dispatch');
    input.value=String(seconds);
    seekDiagnostic.dispatch=observeSeek(input);
    assert(Math.abs(Number(input.value)-seconds)<.2,'seek input clamps the target');
    input.dispatchEvent(new Event('change',{bubbles:true}));
    await wait(()=>ready()&&Math.abs(Number(bar().dataset.position)-seconds)<.2&&!button().disabled,label);
    const {epoch}=await invoke('account_snapshot');
    const native=await invoke('player_snapshot',{epoch});
    const observed={seconds,playId:native.playId,trackId:native.track?.id,state:native.state,position:native.position};
    seekDiagnostic.native=observed;
    assert(observed.playId===playId&&observed.trackId===trackId&&observed.state==='paused'&&
      Math.abs(observed.position-seconds)<.2,`${label} native receipt`);
    nativeSeeks.push(observed);
  }
  async function capture(label) {
    await sleep(220);
    const popup=panel();
    const items=popup ? [...popup.querySelectorAll('button')] : [];
    const r=popup&&rect(popup);
    const row={width:innerWidth,height:innerHeight,ratio:devicePixelRatio,screenX:window.screenX,screenY:window.screenY,
      theme:document.documentElement.dataset.theme,visible:document.visibilityState==='visible',focused:document.hasFocus(),
      trigger:{...rect(button()),text:button().textContent.trim(),tag:button().tagName,focused:document.activeElement===button(),hit:hit(button())},
      popup:r?{...r,background:getComputedStyle(popup).backgroundColor,
        rows:items.map(n=>({label:n.textContent.trim(),checked:n.getAttribute('aria-checked')==='true',focused:n===document.activeElement,hit:hit(n),...rect(n)}))}:null,
      speed:Number(bar().dataset.speed),position:Number(bar().dataset.position),state:bar().dataset.state,
      documentContained:document.documentElement.scrollWidth<=innerWidth,paintSamples:[]};
    assert(row.documentContained,'document overflow');
    assert(row.trigger.hit,'speed trigger obstructed');
    if(r) {
      assert(r.left>=8&&r.top>=8&&r.right<=innerWidth-8&&r.bottom<=innerHeight-8,'speed popup outside viewport');
      assert(Math.abs(r.width-178)<1,'official speed menu width');
      assert(row.popup.rows.length===9&&row.popup.rows.every(n=>Math.abs(n.height-30)<1&&n.hit),'speed rows geometry/hit targets');
      assert(row.popup.rows.filter(n=>n.checked).length===1,'exactly one authoritative speed');
      row.paintSamples=[{x:r.left+4,y:r.top+5,color:row.popup.background}];
      const focused=items.find(n=>n===document.activeElement);
      if(focused&&getComputedStyle(focused).outlineStyle!=='none') {
        const focus=rect(focused),color=getComputedStyle(focused).outlineColor;
        row.paintSamples.push({x:focus.left+1,y:focus.top+focus.height/2,color},
          {x:focus.right-1,y:focus.top+focus.height/2,color});
      }
    }
    await emit(`ux_${String(++count).padStart(2,'0')}_${label}`,row);
    if(!window.__CUMULUS_PROOF_INVOKE__)await sleep(100);
  }
  async function open() {
    click(button());await wait(()=>panel(),'speed menu opens');await sleep(180);
    assert(button().getAttribute('aria-expanded')==='true','expanded trigger');
    assert(document.activeElement?.getAttribute('aria-checked')==='true','focus current speed on open');
  }
  async function choose(value) {
    if(!panel())await open();
    click([...panel().querySelectorAll('button')].find(n=>n.textContent.trim()===`${value}×`));
    await wait(()=>!one('.speed-action-menu')&&Number(bar().dataset.speed)===value&&!one('.playerbar [aria-label="音质"]').disabled&&document.activeElement===button(),'authoritative speed applied');
    assert(bar().dataset.state==='paused'&&Math.abs(Number(bar().dataset.position)-10)<.2,'speed preserves native pause and position');
    assert(document.activeElement===button(),'selection returns focus to trigger');
  }
  async function appearance(theme) {
    click(one('[aria-label="外观主题"]'));await wait(()=>one('.action-menu'),'theme menu');
    click(all('.action-menu button').find(n=>n.textContent.trim()===(theme==='dark'?'深色':'浅色')));
    await wait(()=>document.documentElement.dataset.theme===theme&&!one('.action-menu'),'theme applied');
  }
  async function resize(width,height) {
    await emit('resize',{width,height});await sleep(350);
    // Fractional X11 font DPI can differ from Tauri/GTK's integer window scale.
    // Correct the test window once, keeping the exact CSS viewport requirement.
    if(innerWidth!==width||innerHeight!==height)
      await emit('resize',{width:width*width/innerWidth,height:height*height/innerHeight});
    await wait(()=>innerWidth===width&&innerHeight===height,'resize');
  }
  try {
    await wait(()=>button()&&!button().disabled&&one('.track-row .track-play:not(:disabled)'),'native ready');
    if(window.__speedBaseline) {await capture('legacy_speed_selector');await emit('complete',{ok:true,baseline:true,stageCount:count});return;}
    assert(button().tagName==='BUTTON','native speed select still present');
    click(one('.track-row .track-play:not(:disabled)'));
    await wait(()=>bar().dataset.state==='playing'&&Number(bar().dataset.position)>.2,'native decoding');
    const seekPlay=bar().dataset.playId;
    await pauseCurrent(seekPlay,'native pause');
    await seekPaused(16,seekPlay,'native forward seek while paused');
    await seekPaused(10,seekPlay,'native rewind while paused');
    assert(bar().dataset.state==='paused'&&bar().dataset.playId===seekPlay,'rewind keeps the same paused native play');
    const provides=one('#app').__vue_app__._context.provides;
    const core=provides[Reflect.ownKeys(provides).find(key=>typeof key==='symbol'&&key.description==='core-client')];
    const playerAction=core.playerAction;
    let failedSpeedWrites=0;
    core.playerAction=async (epoch,action)=>{
      if(action.kind==='speed_for'){
        assert(action.scope==='song','the song menu writes the song rate');
        failedSpeedWrites++;
        throw {code:'storage',message:'合成倍速保存失败',retryable:true};
      }
      return playerAction(epoch,action);
    };
    try {
      await open();
      click([...panel().querySelectorAll('button')].find(n=>n.textContent.trim()==='1.5×'));
      await wait(()=>!one('.speed-action-menu')&&one('[data-speed-save-state="failed"]'),'speed-specific error after failed write');
      assert(failedSpeedWrites===1&&Number(bar().dataset.speed)===1&&bar().dataset.state==='paused'&&
        !one('.playback-message button'),'failed speed does not change native playback or offer an audio retry');
      await open();
      assert(panel().textContent.includes('合成倍速保存失败'),'failure is visible in the speed menu');
    } finally { core.playerAction=playerAction; }
    await choose(1.5);
    await wait(()=>one('[data-speed-save-state="idle"]'),'successful explicit speed selection clears failure');
    await choose(1);
    for(const [width,height,theme] of [[1120,760,'light'],[760,540,'light'],[760,540,'dark'],[1120,760,'dark']]) {
      await resize(width,height);await appearance(theme);
      if(Number(bar().dataset.speed)!==1)await choose(1);
      const suffix=`${width}_${theme}`;
      await capture(`resting_${suffix}`);
      await open();await capture(`menu_${suffix}`);
      key('ArrowUp');assert(document.activeElement.textContent.trim()==='1.25×','ArrowUp moves one speed row');
      await capture(`keyboard_${suffix}`);
      click(document.activeElement);await wait(()=>!one('.speed-action-menu')&&Number(bar().dataset.speed)===1.25,'speed changed');
      await capture(`changed_${suffix}`);assert(document.activeElement===button(),'changed focus: '+document.activeElement.outerHTML.slice(0,500));
      await open();await capture(`selected_${suffix}`);
      document.dispatchEvent(new Event('scroll'));
      await sleep(200);assert(panel(),'unchanged scroll retains popup');
      key('Home');assert(document.activeElement.textContent.trim()==='2×','Home reaches first speed');
      key('ArrowDown');assert(document.activeElement.textContent.trim()==='1.5×','ArrowDown moves one speed row');
      key('End');assert(document.activeElement.textContent.trim()==='4×','End reaches extended speed');
      await capture(`extended_${suffix}`);
      key('Escape');await wait(()=>!one('.speed-action-menu'),'Escape dismisses');
      assert(document.activeElement===button(),'Escape restores trigger focus');await capture(`escape_${suffix}`);
      button().focus({preventScroll:true});key('ArrowUp');await wait(()=>panel(),'trigger ArrowUp opens menu');
      await wait(()=>document.activeElement.textContent.trim()==='4×','trigger ArrowUp focuses last speed');
      const target=one('.main-content');target.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true}));
      await wait(()=>!one('.speed-action-menu'),'outside dismisses');
      assert(button().getAttribute('aria-expanded')==='false','collapsed after outside');await capture(`outside_${suffix}`);
      if(speeds.length===0) {
        for(const value of [2,1.5,1.25,1,.75,.5,.25,3,4]) {await choose(value);speeds.push(value);}
        await choose(1.25);
      }
    }
    const resumedAt=performance.now();
    click(one('.playerbar [aria-label="播放"]'));
    await wait(()=>bar().dataset.state==='playing'&&Number(bar().dataset.position)>10.2,'resume from paused seek');
    const elapsed=(performance.now()-resumedAt)/1000;
    const resumedPosition=Number(bar().dataset.position);
    assert(Math.abs(resumedPosition-10-elapsed*1.25)<.35,'resumed native clock differs from requested seek');
    click(one('.playerbar [aria-label="暂停"]'));await wait(()=>bar().dataset.state==='paused','final pause');
    const {epoch}=await core.accountSnapshot();
    const songId=bar().dataset.trackId;
    const voice=await core.playSelection(epoch,{kind:'voice',id:'402'});
    assert(voice.speedScope==='voice'&&voice.speed===1,'podcast starts at its independent default rate');
    await wait(()=>bar().dataset.state==='playing'&&bar().dataset.playId===voice.playId&&Number(bar().dataset.position)>.2,'podcast native clock');
    await pauseCurrent(voice.playId,'native podcast pause');
    await seekPaused(10,voice.playId,'native podcast seek');
    await choose(1.5);
    const voiceAfter=await core.playerSnapshot(epoch);
    assert(voiceAfter.speedScope==='voice'&&voiceAfter.speed===1.5,'podcast menu writes the voice rate');
    const songAgain=await core.playSelection(epoch,{kind:'songs',ids:[songId],index:0,quality:'standard'});
    assert(songAgain.speedScope==='song'&&songAgain.speed===1.25,'song rate restored after podcast');
    await wait(()=>bar().dataset.state==='playing'&&bar().dataset.playId===songAgain.playId&&Number(bar().dataset.position)>.2,'returned song native clock');
    const voiceAgain=await core.playSelection(epoch,{kind:'voice',id:'402'});
    assert(voiceAgain.speedScope==='voice'&&voiceAgain.speed===1.5,'podcast rate restored after song');
    await emit('complete',{ok:true,stageCount:count,speeds,nativeSeeks,resumedPosition,resumeElapsed:elapsed,resumedFromSeek:true,speedFailureIsolated:true,independentSongAndPodcastRates:true,realMusicRequests:0,accountWrites:0});
  } catch(error) {await emit('failed',{ok:false,error:String(error),stageCount:count,width:innerWidth,height:innerHeight,ratio:devicePixelRatio,
    seekDiagnostic:seekDiagnostic?{...seekDiagnostic,after:observeSeek()}:null});}
})();
