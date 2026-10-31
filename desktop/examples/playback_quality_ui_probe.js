// The production UI, account-local preferences, authorization and libmpv/null.
(async () => {
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const one = selector => document.querySelector(selector);
  const all = selector => [...document.querySelectorAll(selector)];
  const assert = (value, label) => { if (!value) throw Error(label); };
  const wait = async (test, label) => { for (let i=0;i<180;i++) { if (test()) return; await sleep(60); } throw Error(label); };
  const invoke = (...args) => (window.__CUMULUS_PROOF_INVOKE__ ?? window.__TAURI_INTERNALS__.invoke)(...args);
  const emit = (stage, value={}) => invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...value}});
  const button = () => one('.playerbar [aria-label="音质"]');
  const panel = () => one('.quality-action-menu:not(.menu-leave-active)');
  const bar = () => one('.playerbar');
  const click = node => { assert(node&&!node.disabled,'control unavailable'); node.focus({preventScroll:true});node.click(); };
  const key = value => document.activeElement.dispatchEvent(new KeyboardEvent('keydown',{key:value,bubbles:true,cancelable:true}));
  const rect = node => { const r=node.getBoundingClientRect();return {left:r.left,top:r.top,right:r.right,bottom:r.bottom,width:r.width,height:r.height}; };
  const hit = node => { const r=rect(node),target=document.elementFromPoint(r.left+r.width/2,r.top+r.height/2);return node===target||node.contains(target); };
  let count=0, expectedTrack='1';
  async function capture(label) {
    await sleep(220);
    const popup=panel(), items=popup ? [...popup.querySelectorAll('.menu-items button')] : [], r=popup&&rect(popup);
    const row={width:innerWidth,height:innerHeight,ratio:devicePixelRatio,screenX:window.screenX,screenY:window.screenY,
      theme:document.documentElement.dataset.theme,visible:document.visibilityState==='visible',focused:document.hasFocus(),
      trigger:{...rect(button()),text:button().textContent.trim(),tag:button().tagName,description:document.getElementById(button().getAttribute('aria-describedby'))?.textContent.trim(),focused:document.activeElement===button(),hit:hit(button())},
      popup:r?{...r,background:getComputedStyle(popup).backgroundColor,heading:popup.querySelector('.quality-menu-heading').textContent.trim(),
        rows:items.map(n=>({label:n.textContent.trim(),checked:n.getAttribute('aria-checked')==='true',focused:n===document.activeElement,hit:hit(n),...rect(n)}))}:null,
      expectedTrack,trackId:bar().dataset.trackId,requested:bar().dataset.requestedQuality,actual:bar().dataset.quality,preferred:bar().dataset.preferredQuality,
      damaged:bar().dataset.qualityPreferencesDamaged==='true',position:Number(bar().dataset.position),state:bar().dataset.state,
      message:one('.playback-message')?.textContent.trim()??'',documentContained:document.documentElement.scrollWidth<=innerWidth,paintSamples:[]};
    assert(row.trackId===expectedTrack,'selected song identity');
    assert(row.documentContained&&row.trigger.hit,'quality trigger overflow/obstruction');
    assert(row.trigger.description==='当前标准','accessible current quality');
    assert(row.trigger.text==='标准','tag must show actual quality, even when server downgrades');
    assert(row.state==='paused'&&Math.abs(row.position-10)<.2,'quality preserves native pause/seek');
    if(r) {
      assert(r.left>=8&&r.top>=8&&r.right<=innerWidth-8&&r.bottom<=innerHeight-8,'quality popup outside viewport');
      assert(Math.abs(r.width-378)<1,'official quality popup width');
      const desired=Math.max(8,Math.min(row.trigger.left+row.trigger.width/2-189,innerWidth-386));
      assert(Math.abs(r.left-desired)<1,'quality popup centered above tag');
      assert(items.length===(row.damaged?1:11),'quality rows');
      const focused=items.find(n=>n===document.activeElement);
      assert(focused&&hit(focused),'focused quality row must scroll into view');
      row.paintSamples=[{x:r.left+4,y:r.top+6,color:row.popup.background}];
      if(getComputedStyle(focused).outlineStyle!=='none') {
        const focus=rect(focused),color=getComputedStyle(focused).outlineColor;
        row.paintSamples.push({x:focus.left+1,y:focus.top+focus.height/2,color},
          {x:focus.right-1,y:focus.top+focus.height/2,color});
      }
    }
    await emit(`ux_${String(++count).padStart(2,'0')}_${label}`,row);
    if(!window.__CUMULUS_PROOF_INVOKE__)await sleep(100);
  }
  async function fixture(kind) {
    window.__qualityFixtureAck=null;await emit('quality-fixture',{kind});
    await wait(()=>window.__qualityFixtureAck,'fixture acknowledgement');
  }
  async function open() {
    click(button());await wait(()=>panel(),'quality menu opens');await sleep(450);
    assert(button().getAttribute('aria-expanded')==='true','expanded trigger');
    if(bar().dataset.qualityPreferencesDamaged!=='true')assert(document.activeElement.getAttribute('aria-checked')==='true','focus actual quality');
  }
  function row(label) { return [...panel().querySelectorAll('button')].find(n=>n.querySelector('.quality-menu-option > span')?.textContent===label); }
  async function choose(label, value) {
    if(!panel())await open();click(row(label));
    await wait(()=>!one('.quality-action-menu')&&bar().dataset.requestedQuality===value&&bar().dataset.preferredQuality===value&&!one('.playback-message')&&document.activeElement===button(),'quality choice applied and focus restored');
  }
  async function metadata() {
    const account=await invoke('account_snapshot');
    const details=await invoke('player_quality_details',{epoch:account.epoch,playId:bar().dataset.playId});
    assert(details.playId===bar().dataset.playId&&details.songId===bar().dataset.trackId,'metadata bound to native current song');
    assert(details.status.privilege==='ready'&&details.status.subscriber==='ready'&&details.status.sizes==='ready'&&Object.values(details.status.useRights).every(v=>v==='ready'),'native metadata reads');
    const hires=details.choices.find(c=>c.quality==='hires'),master=details.choices.find(c=>c.quality==='jymaster');
    assert(hires.availability==='available'&&hires.access==='allowed'&&master.access==='trial_eligible','native capability/entitlement policy');
    assert(details.choices.find(c=>c.quality==='lossless').sizeBytes==='123456','native file size whitelist');
    const json=JSON.stringify(details);assert(!json.includes('SECRET')&&!json.includes('https://')&&!json.includes('cookie'),'private response fields excluded');
  }
  async function pauseAtTen() {
    if(bar().dataset.state!=='paused') {
      await wait(()=>bar().dataset.state==='playing'&&Number(bar().dataset.position)>.2,'native decode');
      click(one('.playerbar [aria-label="暂停"]'));await wait(()=>bar().dataset.state==='paused','pause');
    }
    const seek=one('.playerbar [aria-label="播放进度"]');seek.value='10';seek.dispatchEvent(new Event('change',{bubbles:true}));
    await wait(()=>Math.abs(Number(bar().dataset.position)-10)<.2&&!seek.disabled,'seek');
  }
  async function next(id) {
    const oldPlay=bar().dataset.playId;
    click(one('.playerbar [aria-label="下一首"]'));
    await wait(()=>bar().dataset.playId!==oldPlay&&bar().dataset.trackId===id&&bar().dataset.state==='playing','native next song identity');
    expectedTrack=id;await pauseAtTen();
  }
  async function appearance(theme) {
    click(one('[aria-label="外观主题"]'));await wait(()=>one('.action-menu'),'theme menu');
    click(all('.action-menu button').find(n=>n.textContent.trim()===(theme==='dark'?'深色':'浅色')));
    await wait(()=>document.documentElement.dataset.theme===theme&&!one('.action-menu'),'theme applied');
  }
  async function resize(width,height) {
    await emit('resize',{width,height});await sleep(350);
    if(innerWidth!==width||innerHeight!==height)await emit('resize',{width:width*width/innerWidth,height:height*height/innerHeight});
    await wait(()=>innerWidth===width&&innerHeight===height,'resize');
  }
  try {
    await wait(()=>button()&&one('.track-row .track-play:not(:disabled)'),'UI ready');
    if(window.__qualityReopen) {
      await wait(()=>bar().dataset.requestedQuality==='lossless'&&bar().dataset.preferredQuality==='lossless'&&bar().dataset.state==='paused','durable restore');
      expectedTrack='2';await metadata();await capture('reopened_paused');await open();await capture('reopened_menu');key('Escape');await wait(()=>!one('.quality-action-menu'),'dismiss');
      await next('3');
      assert(bar().dataset.requestedQuality==='lossless','next after restart uses preference');await capture('reopened_next');
    } else {
      click(one('.track-row .track-play:not(:disabled)'));await pauseAtTen();await metadata();
      for(const [width,height,theme] of [[1120,760,'light'],[760,540,'light'],[760,540,'dark'],[1120,760,'dark']]) {
        await resize(width,height);await appearance(theme);
        if(bar().dataset.requestedQuality!=='standard')await choose('标准','standard');
        const suffix=`${width}_${theme}`;
        await capture(`resting_${suffix}`);await open();
        window.dispatchEvent(new Event('resize'));await sleep(160);assert(panel(),'unchanged resize retains quality menu');
        if(count===1) {await resize(1110,760);await wait(()=>!one('.quality-action-menu'),'real viewport resize closes menu');await resize(1120,760);await open();}
        await capture(`menu_${suffix}`);
        key('Home');assert(document.activeElement.textContent.includes('臻音全景声'),'Home first quality');await capture(`first_${suffix}`);
        key('End');assert(document.activeElement.textContent.trim()==='刷新音质','End refresh action');key('ArrowUp');assert(document.activeElement.textContent.includes('节省流量'),'last compatibility quality');await capture(`last_${suffix}`);
        key('Escape');await wait(()=>!one('.quality-action-menu'),'Escape dismiss');assert(document.activeElement===button(),'Escape focus');await capture(`escape_${suffix}`);
        await choose('无损','lossless');await capture(`downgraded_${suffix}`);
        await open();await capture(`preference_${suffix}`);
        assert(panel().textContent.includes('请求：无损')&&panel().textContent.includes('后续播放偏好：无损'),'actual/request/preference explanation');
        one('.main-content').dispatchEvent(new PointerEvent('pointerdown',{bubbles:true}));await wait(()=>!one('.quality-action-menu'),'outside dismiss');
      }
      await fixture('grant-vivid');await fixture('deny');const oldPlay=bar().dataset.playId;
      await open();click(row('臻音全景声'));
      await wait(()=>one('.playback-message button')&&!one('.quality-action-menu'),'denied authorization');
      assert(bar().dataset.playId===oldPlay&&bar().dataset.preferredQuality==='lossless','denial preserves previous audio and preference');await capture('authorization_denied');
      await fixture('allow');click(one('.playback-message button'));await wait(()=>bar().dataset.preferredQuality==='vivid'&&!one('.playback-message'),'explicit retry');await capture('authorization_retried');
      await fixture('damage');await wait(()=>bar().dataset.qualityPreferencesDamaged==='true','damaged preferences');await open();await capture('damaged_preferences');
      click(row('重置音质偏好'));await wait(()=>bar().dataset.qualityPreferencesDamaged==='false'&&!one('.quality-action-menu'),'explicit repair');await capture('repaired_preferences');
      await open();await capture('repaired_menu');await choose('无损','lossless');await capture('saved_preference');
      await next('2');
      assert(bar().dataset.requestedQuality==='lossless','already queued next uses preference');await capture('next_uses_preference');
    }
    await emit('complete',{ok:true,stageCount:count,reopened:!!window.__qualityReopen,nativeQualityReadVerified:true,realMusicRequests:0,accountWrites:0});
  } catch(error) {await emit('failed',{ok:false,error:String(error),stageCount:count,width:innerWidth,height:innerHeight,ratio:devicePixelRatio});}
})();
