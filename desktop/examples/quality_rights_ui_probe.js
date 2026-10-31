// Production menu/dialog actions; synthetic APIs, native SQLite and libmpv/null.
(async()=>{
  const sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const one=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)];
  const invoke=(...args)=>(window.__CUMULUS_PROOF_INVOKE__??window.__TAURI_INTERNALS__.invoke)(...args);
  const emit=(stage,value={})=>invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...value}});
  const assert=(ok,label)=>{if(!ok)throw Error(label);};
  const wait=async(test,label)=>{for(let i=0;i<240;i++){if(test())return;await sleep(70);}throw Error(label);};
  const bar=()=>one('.playerbar'),tag=()=>one('.playerbar [aria-label="音质"]');
  const panel=()=>one('.quality-action-menu:not(.menu-leave-active)');
  const dialog=()=>one('.quality-trial-dialog:not(.modal-leave-active)');
  const button=text=>all('button').find(n=>n.textContent.trim()===text);
  const click=n=>{assert(n&&!n.disabled,'unavailable control');n.focus({preventScroll:true});n.click();};
  const row=label=>[...(panel()?.querySelectorAll('.menu-items button')??[])].find(n=>n.querySelector('.quality-menu-option > span')?.textContent===label);
  const key=value=>document.activeElement.dispatchEvent(new KeyboardEvent('keydown',{key:value,bubbles:true,cancelable:true}));
  const rect=n=>{const r=n.getBoundingClientRect();return {left:r.left,top:r.top,right:r.right,bottom:r.bottom,width:r.width,height:r.height};};
  const hit=n=>{const r=rect(n),top=document.elementFromPoint(r.left+r.width/2,r.top+r.height/2);return n===top||n.contains(top);};
  let count=0,last='bootstrap';
  async function fixture(kind){window.__qualityFixtureAck=null;await emit('quality-fixture',{kind});await wait(()=>window.__qualityFixtureAck,'fixture acknowledgment');}
  async function open(){click(tag());await wait(()=>panel(),'menu opens');await wait(()=>panel()?.querySelector('[aria-busy="false"]'),'metadata settled');await sleep(120);}
  async function dismiss(){if(panel())key('Escape');else if(dialog())dialog().dispatchEvent(new Event('cancel',{cancelable:true}));await wait(()=>!one('.quality-action-menu')&&!one('.quality-trial-dialog'),'dismissed');assert(document.activeElement===tag(),'focus returns to quality tag');}
  async function choose(label){if(!panel())await open();click(row(label));}
  async function capture(label){
    last=label;await sleep(300);
    const surface=dialog()??panel(),r=surface&&rect(surface);
    const payload={width:innerWidth,height:innerHeight,ratio:devicePixelRatio,screenX:window.screenX,screenY:window.screenY,visible:document.visibilityState==='visible',focused:document.hasFocus(),
      theme:document.documentElement.dataset.theme,trackId:bar().dataset.trackId,requested:bar().dataset.requestedQuality,actual:bar().dataset.quality,
      preferred:bar().dataset.preferredQuality,state:bar().dataset.state,position:Number(bar().dataset.position),
      contained:document.documentElement.scrollWidth<=innerWidth,tagHit:hit(tag()),
      popup:panel()?{...rect(panel()),text:panel().textContent,rows:[...panel().querySelectorAll('.menu-items button')].map(n=>({text:n.textContent,disabled:n.disabled,checked:n.getAttribute('aria-checked')==='true',focused:document.activeElement===n,hit:hit(n)}))}:null,
      dialog:dialog()?{...rect(dialog()),text:dialog().textContent,phase:dialog().dataset.phase,kind:dialog().dataset.kind,busy:dialog().getAttribute('aria-busy')==='true',
        controls:[...dialog().querySelectorAll('button')].map(n=>({text:n.textContent.trim(),hit:hit(n),focused:document.activeElement===n,disabled:n.getAttribute('aria-disabled')==='true'}))}:null,paintSamples:[]};
    assert(payload.contained,'horizontal overflow');assert(payload.state==='paused'&&Math.abs(payload.position-10)<.2,'native pause/position');assert(payload.actual==='standard','actual quality remains standard');
    if(r){assert(r.left>=8&&r.top>=8&&r.right<=innerWidth-8&&r.bottom<=innerHeight-8,'surface inside viewport');payload.paintSamples=[{x:r.left+(dialog()?8:4),y:r.top+(dialog()?12:6),color:getComputedStyle(surface).backgroundColor}];}
    if(payload.dialog)assert(payload.dialog.controls.every(c=>c.hit)&&dialog().open,'dialog controls are visible and reachable');
    else {assert(payload.tagHit,'quality tag hit');if(panel()){const focused=document.activeElement;assert(panel().contains(focused)&&hit(focused),'menu focus remains visible');}}
    await emit(`ux_${String(++count).padStart(2,'0')}_${label}`,payload);
  }
  async function layout(w,h,theme){await emit('resize',{width:w,height:h});await sleep(300);if(innerWidth!==w||innerHeight!==h)await emit('resize',{width:w*w/innerWidth,height:h*h/innerHeight});await wait(()=>innerWidth===w&&innerHeight===h,'resize');click(one('[aria-label="外观主题"]'));await wait(()=>one('.action-menu'),'theme menu');click(button(theme==='dark'?'深色':'浅色'));await wait(()=>document.documentElement.dataset.theme===theme&&!one('.action-menu'),'theme applied');}
  async function confirm(){await choose('超清母带');await wait(()=>dialog()?.dataset.kind==='confirm','explicit offer');click(button('确认启用试用'));}
  async function applied(){await wait(()=>!one('.quality-trial-dialog')&&!one('.quality-action-menu')&&bar().dataset.preferredQuality==='jymaster','trial quality applied');assert(document.activeElement===tag(),'trial success focus');}
  async function expire(){await fixture('trial-expire');await wait(()=>bar().dataset.preferredQuality==='lossless','native preference fallback');}
  async function freshSong(){
    // Use a different song for denial: the previous song still has a valid
    // authorized Master URL in the real resolver cache, which must stay intact.
    const old=bar().dataset.playId;const target=all('.track-row .track-play:not(:disabled)')[2];target.scrollIntoView({block:'nearest'});click(target);
    await wait(()=>bar().dataset.playId!==old&&bar().dataset.trackId==='3'&&bar().dataset.state==='playing'&&Number(bar().dataset.position)>.2,'fresh selected song');
    click(one('.playerbar [aria-label="暂停"]'));await wait(()=>bar().dataset.state==='paused','fresh song pause');const seek=one('[aria-label="播放进度"]');seek.value='10';seek.dispatchEvent(new Event('change',{bubbles:true}));await wait(()=>Math.abs(Number(bar().dataset.position)-10)<.2&&!seek.disabled,'fresh song seek');
  }
  try{
    await wait(()=>tag()&&one('.track-row .track-play:not(:disabled)'),'UI ready');
    if(window.__rightsReopen){
      await wait(()=>bar().dataset.state==='paused'&&bar().dataset.preferredQuality==='jymaster','restored trial');await capture('reopened_active');await open();
      assert(row('超清母带').textContent.includes('试用中'),'current trial status');await capture('reopened_menu');await dismiss();await expire();await capture('reopened_expired');
    }else{
      click(one('.track-row .track-play:not(:disabled)'));await wait(()=>bar().dataset.state==='playing'&&Number(bar().dataset.position)>.2,'native decode');click(one('.playerbar [aria-label="暂停"]'));await wait(()=>bar().dataset.state==='paused','pause');
      const seek=one('[aria-label="播放进度"]');seek.value='10';seek.dispatchEvent(new Event('change',{bubbles:true}));await wait(()=>Math.abs(Number(bar().dataset.position)-10)<.2&&!seek.disabled,'native seek');
      const unchanged=bar().dataset.playId;await choose('标准');await wait(()=>!one('.quality-action-menu'),'current quality dismissed');await sleep(350);assert(bar().dataset.playId===unchanged,'selecting unchanged actual/requested/preferred quality does not reload playback');
      for(const [w,h,theme] of [[1120,760,'light'],[760,540,'light'],[760,540,'dark'],[1120,760,'dark']]){
        const suffix=`${w}_${theme}`;await layout(w,h,theme);await open();
        assert(row('超清母带').textContent.includes('可试用')&&row('无损').textContent.includes('120.6 KB'),'native rights and byte sizes in rows');
        assert(document.activeElement.getAttribute('aria-checked')==='true','focus actual checked quality');
        panel().dispatchEvent(new WheelEvent('wheel',{deltaY:-120,bubbles:true}));const list=panel().querySelector('.menu-items');list.scrollTop=0;await sleep(650);assert(list.scrollTop===0,'player polls must not snap a manually scrolled menu to its focused row');
        key('End');assert(document.activeElement.textContent.trim()==='刷新音质','footer keyboard action');key('Home');assert(document.activeElement===row('臻音全景声'),'first quality reachable');
        await capture(`menu_${suffix}`);click(row('超清母带'));await wait(()=>dialog(),'offer opens');assert(document.activeElement===button('取消'),'default focus does not consume quota');
        assert(dialog().textContent.includes('1 分钟')&&dialog().textContent.includes('暂停或关闭客户端不会停止计时'),'offer duration and consequence');await capture(`confirm_${suffix}`);
        click(button('取消'));await wait(()=>!one('.quality-trial-dialog'),'cancel');assert(document.activeElement===tag(),'cancel focus');await capture(`cancel_${suffix}`);
        await choose('沉浸环绕声');await wait(()=>dialog()?.dataset.kind==='notice','membership notice');assert(dialog().textContent.includes('超级会员'),'exact entitlement guidance');await capture(`member_${suffix}`);await dismiss();
      }
      await fixture('metadata-delay');click(tag());await wait(()=>panel()?.querySelector('[aria-busy="true"]'),'visible metadata loading');await capture('reading_metadata');await fixture('metadata-fast');await wait(()=>panel()?.querySelector('[aria-busy="false"]'),'metadata returns');await dismiss();
      await fixture('rights-partial');await open();assert(panel().textContent.includes('部分音质信息暂未确认')&&row('超清母带').textContent.includes('权益待确认'),'partial reads remain unknown');await capture('partial_metadata');click(row('超清母带'));await wait(()=>dialog(),'unknown rights notice');await capture('partial_notice');await dismiss();await fixture('rights-full');
      await fixture('assets-standard');await open();assert([...panel().querySelectorAll('.menu-items button')].length===2&&!row('Hi-Res'),'capability filter');await capture('unavailable_assets');await dismiss();await fixture('assets-all');
      await fixture('trial-damage');await open();assert(panel().textContent.includes('试用记录无法读取'),'damaged journal warning');await capture('journal_damaged_menu');click(row('超清母带'));await wait(()=>dialog(),'damaged journal blocks start');await capture('journal_damaged_notice');assert(!button('确认启用试用'),'no destructive repair or silent overwrite');await dismiss();await fixture('trial-restore');
      await choose('沉浸环绕声');await wait(()=>dialog(),'member notice');click(button('查看会员权益'));await wait(()=>one('.member-page')?.getAttribute('aria-busy')==='false'&&!one('.quality-trial-dialog'),'member navigation');await capture('member_navigation');click(one('[aria-label="返回上一页"]'));await wait(()=>one('.track-row .track-play'),'return to recommendations');
      await choose('无损');await wait(()=>bar().dataset.preferredQuality==='lossless'&&!one('.quality-action-menu'),'set original preference');
      await fixture('trial-delay');await confirm();await wait(()=>dialog()?.getAttribute('aria-busy')==='true','visible in-flight consent');await capture('consent_pending');await fixture('trial-fast');await applied();await capture('activated');await open();assert(row('超清母带').textContent.includes('试用中')&&row('超清母带').textContent.includes('剩余'),'active rights and remaining time');await capture('active_menu');await dismiss();const play=bar().dataset.playId;await expire();assert(bar().dataset.playId===play,'expiry does not reload audio');await capture('expired');
      await freshSong();await fixture('trial-eligible');await fixture('deny-master');const beforeDenial=bar().dataset.playId;await confirm();await wait(()=>dialog()?.dataset.phase==='active'&&!dialog().getAttribute('aria-busy').includes('true'),'activation succeeded but playback denied');assert(dialog().querySelector('[role="alert"]')&&bar().dataset.playId===beforeDenial,'independent playback failure preserves old audio');await capture('playback_denied');await fixture('allow-master');click(button('使用已启用音质'));await applied();await capture('playback_retried');await expire();await capture('retry_expired');
      await fixture('trial-eligible');await fixture('trial-lost-response');await confirm();await wait(()=>dialog()?.dataset.phase==='uncertain'&&dialog().getAttribute('aria-busy')==='false','uncertain activation');await capture('lost_response');await dismiss();await open();assert(row('超清母带').textContent.includes('待核对'),'persisted unresolved state');await capture('lost_reopened_menu');click(row('超清母带'));await wait(()=>dialog()?.dataset.phase==='uncertain','recover unresolved journal');await capture('lost_reopened_notice');click(button('核对试用结果'));await wait(()=>dialog()?.dataset.phase==='active'&&dialog().getAttribute('aria-busy')==='false','read-only reconciliation');await capture('reconciled');click(button('使用已启用音质'));await applied();await capture('active_retried');
    }
    await emit('complete',{ok:true,stageCount:count,reopened:!!window.__rightsReopen,consentDialogVerified:!window.__rightsReopen,manualScrollVerified:!window.__rightsReopen,dynamicRightsVerified:true,realMusicRequests:0,accountWrites:0});
  }catch(error){await emit('failed',{ok:false,error:String(error),stageCount:count,last,active:{text:document.activeElement?.textContent,tag:document.activeElement?.tagName,role:document.activeElement?.getAttribute('role'),rect:rect(document.activeElement)},popup:panel()?{rect:rect(panel()),scroll:panel().scrollTop,header:rect(panel().querySelector('.quality-menu-heading')),items:rect(panel().querySelector('.menu-items')),itemsScroll:panel().querySelector('.menu-items').scrollTop,footer:rect(panel().querySelector('.quality-menu-note'))}:null,body:document.body.textContent.slice(-2400)});}
})();
