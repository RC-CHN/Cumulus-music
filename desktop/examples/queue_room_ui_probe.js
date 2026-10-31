// Real production UI and Rust actor; synthetic HTTP/IM/audio devices only.
(async () => {
  const one=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)];
  const invoke=(...a)=>window.__TAURI_INTERNALS__.invoke(...a);
  const emit=async(stage,value={})=>{if(stage.startsWith('ux_'))window.__queueRoomRecorded='';await invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...value}});if(stage.startsWith('ux_'))await wait(()=>window.__queueRoomRecorded===stage,'native observation saved');};
  const sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const assert=(v,m)=>{if(!v)throw Error(m);};
  const wait=async(f,m)=>{for(let i=0;i<400;i++){if(await f())return;await sleep(60);}throw Error(m);};
  const button=(text,root=document)=>[...root.querySelectorAll('button')].find(n=>n.getAttribute('aria-label')===text||n.textContent.trim()===text);
  const click=n=>{assert(n&&!n.disabled,'enabled control '+n?.outerHTML.slice(0,220));n.focus({preventScroll:true});n.click();};
  const panel=()=>one('.queue-panel'),modal=()=>one('.queue-clear-dialog[open]:not(.modal-leave-active)'),bar=()=>one('.playerbar');
  const rect=n=>{const r=n.getBoundingClientRect();return {left:r.left,top:r.top,right:r.right,bottom:r.bottom,width:r.width,height:r.height};};
  const hit=n=>{const r=rect(n),v=document.elementFromPoint((r.left+r.right)/2,(r.top+r.bottom)/2);return v===n||n.contains(v);};
  let epoch,client,playId,count=0,last='bootstrap';
  const focusTrace=[];for(const name of ['focusin','focusout'])document.addEventListener(name,e=>{focusTrace.push({event:name,tag:e.target.tagName,text:e.target.textContent?.trim().slice(0,60),queue:!!e.target.closest('.queue-panel'),at:performance.now()});if(focusTrace.length>30)focusTrace.shift();},true);
  const room=async()=>(await client.togetherView(epoch,false)).value;
  async function fixture(kind){window.__queueRoomFixtureAck=false;await emit('fixture',{kind});await wait(()=>window.__queueRoomFixtureAck,'fixture '+kind);}
  async function resize(width,height){await emit('resize',{width,height});await sleep(240);if(innerWidth!==width||innerHeight!==height)await emit('resize',{width:width*width/innerWidth,height:height*height/innerHeight});await wait(()=>innerWidth===width&&innerHeight===height,'resize');}
  async function theme(value){click(button('外观主题'));await wait(()=>one('.action-menu'),'theme menu');click(button(value==='dark'?'深色':'浅色',one('.action-menu')));await wait(()=>document.documentElement.dataset.theme===value&&!one('.action-menu'),'theme applied');}
  async function prepare(){
    await client.playSelection(epoch,{kind:'songs',ids:['91','92','93'],index:0,quality:'standard'});
    await wait(()=>bar().dataset.state==='playing','playing');click(button('暂停',bar()));await wait(()=>bar().dataset.state==='paused','paused');
    const seek=one('[aria-label="播放进度"]');seek.value='12';seek.dispatchEvent(new Event('change',{bubbles:true}));await wait(()=>Math.abs(Number(bar().dataset.position)-12)<.1&&!seek.disabled,'seek');playId=bar().dataset.playId;
  }
  async function openQueue(){click(button('播放队列'));await wait(()=>panel()&&!one('.queue-panel-enter-active')&&!one('.queue-panel[aria-busy="true"]'),'queue opened');}
  async function closeQueue(){click(button('关闭播放队列',panel()));await wait(()=>!panel(),'queue closed');assert(document.activeElement===button('播放队列'),'queue close focus');}
  async function openClear(together){click(button('清空',panel()));await wait(()=>modal(),'confirmation');assert(!!one('.queue-clear-together')===together,'captured room warning');assert(document.activeElement===button('取消',modal()),'cancel receives initial focus');}
  async function cancel(){click(button('取消',modal()));await wait(()=>!one('.queue-clear-dialog'),'cancel finished');assert(document.activeElement===(button('清空',panel()).disabled?button('关闭播放队列',panel()):button('清空',panel())),'cancel return focus');}
  async function create(peer=false){
    click(button('创建房间'));await wait(async()=>{const v=await room();return v.attached&&v.targetId&&!v.pending.length&&v.connection.state==='connected';},'room attached');
    if(peer){await fixture('peer');await wait(async()=>{const v=await room();return v.sharedPhase==='applied'&&v.room.members.length===2;},'synthetic peer joined');}
    await wait(async()=>{const q=await client.playerQueue(epoch);return !q.clearContext.issue;},'clear policy ready');
  }
  async function capture(label,empty=false){
    last=label;await sleep(330);await wait(()=>!one('.queue-panel-enter-active,.modal-enter-active,.modal-leave-active'),'motion settled');
    const p=panel(),d=modal(),r=p&&rect(p),dr=d&&rect(d);
    assert(document.documentElement.scrollWidth<=innerWidth,'document contained');
    if(p)assert(r.left>=-1/64&&r.right<=innerWidth+1/64&&r.bottom<=rect(bar()).top+1,'queue contained');
    if(d){assert(dr.left>=8&&dr.right<=innerWidth-8&&dr.top>=8&&dr.bottom<=innerHeight-8,'dialog contained');assert(hit(button('取消',d)),'cancel hit');}
    if(label==='pending_page')assert(document.activeElement===button('核对状态'),'recovery focus remains after transitions');
    if(empty)assert(bar().dataset.state==='idle'&&!bar().dataset.playId,'empty native transport');
    else assert(bar().dataset.state==='paused'&&bar().dataset.playId===playId&&Math.abs(Number(bar().dataset.position)-12)<.1,'same paused media');
    assert(!one('.playback-message'),'room operation is not an audio retry');
    await emit(`ux_${String(++count).padStart(2,'0')}_${label}`,{width:innerWidth,height:innerHeight,ratio:devicePixelRatio,screenX:window.screenX,screenY:window.screenY,visible:document.visibilityState==='visible',focused:document.hasFocus(),theme:document.documentElement.dataset.theme,empty,playId:bar().dataset.playId||null,warning:!!one('.queue-clear-together'),completion:one('.queue-clear-completion')?.textContent.trim()??null,active:{tag:document.activeElement?.tagName,text:document.activeElement?.textContent.trim().slice(0,90)},dialogs:d?[{open:d.open,opacity:getComputedStyle(d).opacity,background:getComputedStyle(d).backgroundColor,rect:dr}]:[],paintSamples:p&&!d?[{x:r.left+4,y:r.top+4,color:getComputedStyle(p).backgroundColor}]:[]});
  }
  try{
    await wait(()=>one('.account-button')?.textContent.includes('本地验证账号'),'fixture account');
    const provides=one('#app').__vue_app__._context.provides;client=provides[Reflect.ownKeys(provides).find(k=>typeof k==='symbol'&&k.description==='core-client')];epoch=(await client.accountSnapshot()).epoch;
    await resize(1120,760);await prepare();await client.playerAction(epoch,{kind:'speed',value:1.25});await client.playerAction(epoch,{kind:'mode',mode:'sequential'});
    await openQueue();await openClear(false);await capture('ordinary_confirmation');
    await fixture('observe_room');click(button('确认清空',modal()));await wait(()=>button('确认清空',modal())?.disabled&&modal().textContent.includes('播放列表已变化'),'room change rejected');assert(!one('.queue-clear-together'),'old consent is not rewritten for a new room');await capture('room_changed');await cancel();await capture('unattached_room');
    click(button('查看一起听状态',panel()));await wait(()=>!panel()&&one('.together-page')&&button('继续本机控制'),'observed room page');await capture('observed_room');click(button('退出房间'));await wait(()=>one('.together-leave'),'captured exit');await capture('exit_confirmation');await fixture('replace_room');await wait(()=>button('确认退出')?.disabled&&one('.together-leave').textContent.includes('房间已变化'),'changed exit is disabled');await capture('exit_room_changed');click(button('继续收听'));await fixture('erase_room');click(button('刷新一起听'));await wait(()=>button('创建房间')&&!button('创建房间').disabled,'room absent');
    await create();await wait(()=>bar().dataset.speed==='1','room forces normal speed');
    for(const [width,height,value] of [[1120,760,'light'],[760,540,'light'],[760,540,'dark'],[1120,760,'dark']]){
      await resize(width,height);await theme(value);await openQueue();await wait(()=>!button('清空',panel()).disabled,'clear available');await capture(`list_${width}_${value}`);await openClear(true);await capture(`confirm_${width}_${value}`);await cancel();await capture(`cancel_${width}_${value}`);await closeQueue();
    }
    await openQueue();await openClear(true);await fixture('reject_queue');click(button('确认清空',modal()));await wait(()=>modal()?.textContent.includes('无法读写本地数据')&&document.activeElement===button('取消',modal()),'local clear failure');await capture('clear_failed');await cancel();await fixture('allow_queue');await capture('failure_cancelled');
    await fixture('hold_end');await openClear(true);click(button('确认清空',modal()));await wait(()=>!one('.queue-clear-dialog')&&one('.queue-empty')&&one('.queue-clear-completion')&&!one('.queue-clear-completion').textContent.includes('已结束'),'local clear does not claim room exit');await capture('end_pending',true);
    await fixture('release_end');await wait(()=>one('.queue-clear-completion')?.textContent.includes('已恢复本机播放设置')&&bar().dataset.speed==='1.25'&&button('顺序播放',bar()),'confirmed room restoration');await capture('end_confirmed',true);await closeQueue();
    await prepare();await create(true);await openQueue();await openClear(true);await capture('peer_confirmation');await fixture('lose_end');click(button('确认清空',modal()));await wait(async()=>{const v=await room();return !one('.queue-clear-dialog')&&v.pending.some(op=>op.action==='end')&&v.syncError&&one('.queue-clear-completion')?.textContent.includes('尚未完成');},'lost end acknowledged as pending');await capture('end_uncertain',true);
    click(button('查看一起听状态',panel()));await wait(()=>!panel()&&button('核对状态')&&document.activeElement===button('核对状态'),'read-only recovery page focuses its action');assert(button('退出房间').disabled&&one('.together-room h2').textContent.includes('正在结束'),'pending exit has consistent title and controls');assert(hit(button('核对状态')),'reconciliation action is visible and reachable');assert(one('.together-queue h2').textContent.includes('上次同步')&&all('.together-track-title').every(n=>n.textContent.includes('Song')),'pending exit retains confirmed song metadata');await capture('pending_page',true);click(button('核对状态'));await wait(async()=>{const v=await room();return !v.room&&!v.attached&&!v.localRecovery&&!v.pending.length&&button('创建房间')&&!button('创建房间').disabled;},'end reconciled');await capture('reconciled_page',true);await openQueue();await wait(()=>one('.queue-empty'),'confirmed empty');await capture('reopened_empty',true);
    await emit('complete',{ok:true,stageCount:count,queueRoomUi:true,realMusicRequests:0,realAccountWrites:0,fixtureApi:true,fixtureIm:true,fixtureAudio:true,realPeerVerified:false,physicalAudioOutputVerified:false});
  }catch(error){await emit('failed',{ok:false,error:String(error),last,stageCount:count,focusTrace,body:document.body.textContent.slice(-2200),active:document.activeElement?.outerHTML.slice(0,600)});}
})();
