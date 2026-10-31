(async()=>{
 const sleep=ms=>new Promise(r=>setTimeout(r,ms)),one=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)];
 const invoke=(...a)=>(window.__CUMULUS_PROOF_INVOKE__??window.__TAURI_INTERNALS__.invoke)(...a);
 const emit=(stage,data={})=>invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...data}});
 const assert=(ok,m)=>{if(!ok)throw Error(m);};
 const wait=async(f,m)=>{for(let i=0;i<280;i++){if(f())return;await sleep(60);}throw Error(m);};
 const bar=()=>one('.playerbar'),control=()=>one('.mode-control'),trigger=()=>control()?.querySelector('button'),feedback=()=>one('.mode-feedback:not(.menu-leave-active)');
 const button=t=>all('button').find(n=>n.getAttribute('aria-label')===t||n.textContent.trim()===t);
 const click=n=>{assert(n&&!n.disabled,'enabled '+n?.outerHTML.slice(0,240));n.focus({preventScroll:true});n.click();};
 const hit=n=>{if(!n)return false;const r=n.getBoundingClientRect(),e=document.elementFromPoint((r.left+r.right)/2,(r.top+r.bottom)/2);return e===n||n.contains(e);};
 let count=0,last='bootstrap',normalPlayId;
 async function settled(mode){await wait(()=>bar().dataset.mode===mode&&trigger().getAttribute('aria-busy')==='false','mode '+mode);}
 async function cycle(mode){click(trigger());await settled(mode);}
 async function fixture(kind){window.__modeFixtureAck=null;await emit('mode-fixture',{kind});await wait(()=>window.__modeFixtureAck===kind,'fixture '+kind);}
 async function layout(w,h,theme){
  await emit('resize',{width:w,height:h});await sleep(240);
  if(innerWidth!==w||innerHeight!==h)await emit('resize',{width:w*w/innerWidth,height:h*h/innerHeight});
  await wait(()=>innerWidth===w&&innerHeight===h,'size');click(one('[aria-label="外观主题"]'));await wait(()=>one('.action-menu'),'theme menu');click(button(theme==='dark'?'深色':'浅色'));await wait(()=>document.documentElement.dataset.theme===theme&&!one('.action-menu'),'theme');
 }
 async function capture(label){
  last=label;await sleep(300);const fm=bar().dataset.fm==='true',p=feedback(),r=p?.getBoundingClientRect();
  assert(document.documentElement.scrollWidth<=innerWidth&&hit(trigger()),'layout/mode hit');
  assert(!one('.playback-message'),'mode error is separate from audio');
  const emptyControlsDisabled=fm&&['上一首','播放','下一首'].every(t=>one(`.playerbar [aria-label="${t}"]`)?.disabled)&&['FM上一首','FM下一首'].every(t=>button(t)?.disabled);
  if(fm){
   assert(control().dataset.family==='fm'&&bar().dataset.state==='idle'&&!bar().dataset.trackId&&!bar().dataset.playId,'empty native FM');
   assert(hit(button('返回原播放队列'))&&emptyControlsDisabled,'return/disabled transport');
   assert(one('.fm-copy h2').textContent==='暂时没有可播放的推荐','empty text');assert(one('.fm-copy>p').textContent.trim()==='重新获取推荐，发现更多音乐','empty recommendation hint');assert(one('.now-copy small').textContent.trim()==='发现每一首喜欢的歌','empty footer copy remains');
   assert(one('.fm-actions .primary').textContent.trim()===(one('.fm-page').dataset.modePending==='true'?'同步并继续播放':'重新获取推荐'),'empty action describes fetching recommendations');
   assert(trigger().getAttribute('aria-label')===(bar().dataset.mode==='single_loop'?'单曲循环':'无限漫游'),'FM mode label');
  }else{
   assert(bar().dataset.trackId==='2'&&bar().dataset.state==='paused'&&Math.abs(Number(bar().dataset.position)-10)<.2,'saved normal selection/position');
   assert(bar().dataset.playId===normalPlayId,'normal mode keeps decoder');
  }
  if(r)assert(r.left>=8&&r.top>=8&&r.right<=innerWidth-8&&r.bottom<=innerHeight-8&&[...p.querySelectorAll('button')].every(hit),'feedback containment');
  await emit(`ux_${String(++count).padStart(2,'0')}_${label}`,{width:innerWidth,height:innerHeight,ratio:devicePixelRatio,screenX:window.screenX,screenY:window.screenY,visible:document.visibilityState==='visible',focused:document.hasFocus(),theme:document.documentElement.dataset.theme,mode:bar().dataset.mode,family:control().dataset.family,fm,playId:bar().dataset.playId??null,trackId:bar().dataset.trackId??null,position:Number(bar().dataset.position),contained:true,triggerHit:true,returnHit:fm&&hit(button('返回原播放队列')),emptyControlsDisabled,active:{tag:document.activeElement?.tagName,label:document.activeElement?.getAttribute('aria-label'),text:document.activeElement?.textContent.trim().slice(0,80)},feedback:r?{text:p.textContent,buttons:[...p.querySelectorAll('button')].map(n=>({text:n.textContent,hit:hit(n)}))}:null,paintSamples:r?[{x:r.left+8,y:r.top+8,color:getComputedStyle(p).backgroundColor}]:[]});
 }
 try{
  await wait(()=>trigger()&&!trigger().disabled&&one('.track-row .track-play:not(:disabled)'),'ready');
  if(!window.__modeReopen){
   click(all('.track-row .track-play:not(:disabled)')[1]);await wait(()=>bar().dataset.trackId==='2'&&bar().dataset.state==='playing'&&Number(bar().dataset.position)>.2,'play normal');
   click(one('.playerbar [aria-label="暂停"]'));await wait(()=>bar().dataset.state==='paused','pause');
   const seek=one('[aria-label="播放进度"]');seek.value='10';seek.dispatchEvent(new Event('change',{bubbles:true}));await wait(()=>Math.abs(Number(bar().dataset.position)-10)<.2&&!seek.disabled,'seek');normalPlayId=bar().dataset.playId;
   for(let i=0;i<4&&bar().dataset.mode!=='random';i++){const old=bar().dataset.mode;click(trigger());await wait(()=>bar().dataset.mode!==old&&trigger().getAttribute('aria-busy')==='false','normal cycle');}
   assert(bar().dataset.mode==='random','saved random mode');await capture('normal_backup');
  }
  click(button('私人漫游'));await wait(()=>one('.fm-page')&&one('.fm-actions .primary')&&!one('.fm-actions .primary').disabled,'FM page');
  if(window.__modeReopen){
   await wait(()=>bar().dataset.fm==='true'&&button('返回原播放队列'),'restored empty FM');await settled('single_loop');await capture('empty_reopened');
   await cycle('sequential');await capture('empty_infinite_after_reopen');click(button('返回原播放队列'));
   await wait(()=>bar().dataset.fm==='false'&&bar().dataset.trackId==='2'&&bar().dataset.state==='paused'&&!button('返回原播放队列'),'normal restored');
   normalPlayId=bar().dataset.playId;assert(bar().dataset.mode==='random','saved mode restored');assert(document.activeElement===one('.fm-header h1'),'return focus');await capture('left_empty_fm');
   await cycle('sequential');await capture('normal_switched');await cycle('order_loop');await cycle('single_loop');await cycle('random');await capture('normal_restored');
  }else{
   click(button('开始漫游'));await wait(()=>bar().dataset.fm==='true'&&button('返回原播放队列'),'empty FM entered');
   await capture('empty_initial');click(button('重新获取推荐'));await wait(()=>!one('.fm-actions .primary').disabled,'recommendation refresh settled');await capture('empty_refetched');
   for(const [w,h,theme] of [[1120,760,'light'],[760,540,'light'],[760,540,'dark'],[1120,760,'dark']]){
    await layout(w,h,theme);if(bar().dataset.mode!=='sequential')await cycle('sequential');await capture(`empty_infinite_${w}_${theme}`);await cycle('single_loop');await capture(`empty_single_${w}_${theme}`);
   }
   await cycle('sequential');await fixture('reject');click(trigger());await wait(()=>feedback()&&trigger().getAttribute('aria-busy')==='false'&&one('.fm-page').dataset.modePending==='true','empty mode failure');
   assert(bar().dataset.mode==='sequential','failed mode unchanged');await capture('empty_failed');await fixture('allow');click(button('重试切换'));await settled('single_loop');await wait(()=>!feedback()&&!one('.fm-notice'),'mode repaired');assert(document.activeElement===trigger(),'retry focus');await capture('empty_retried');
  }
  await emit('complete',{ok:true,stageCount:count,reopened:!!window.__modeReopen,modeUiVerified:true,ordinaryVerified:true,fmVerified:true,emptyFmVerified:true,togetherPresentationVerified:false,realMusicRequests:0,accountWrites:0});
 }catch(error){await emit('failed',{ok:false,error:String(error),last,stageCount:count,body:document.body.textContent.slice(-1400)});}
})();
