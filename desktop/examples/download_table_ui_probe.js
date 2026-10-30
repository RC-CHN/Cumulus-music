// Download table UI: official resize/sort states in the actual production WebView.
(async () => {
  const one=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)];
  const sleep=ms=>new Promise(r=>setTimeout(r,ms)),assert=(ok,m)=>{if(!ok)throw Error(m);};
  const wait=async(fn,m,attempts=240)=>{await sleep(40);for(let i=0;i<attempts;i++){if(fn())return;await sleep(50);}throw Error(m);};
  const emit=(stage,data={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...data}});
  const button=(label,root=document)=>[...root.querySelectorAll('button')].find(n=>n.getAttribute('aria-label')===label||n.textContent.trim()===label);
  const click=n=>{assert(n&&!n.disabled,`enabled ${n?.outerHTML}`);n.focus({preventScroll:true});n.click();};
  const key=(n,k,extra={})=>n.dispatchEvent(new KeyboardEvent('keydown',{key:k,bubbles:true,cancelable:true,...extra}));
  const input=(n,text)=>{n.value=text;n.dispatchEvent(new Event('input',{bubbles:true}));};
  const near=(a,b,m)=>assert(Math.abs(a-b)<1.2,`${m}: ${a}/${b}`);
  const first=()=>one('[data-download-index="0"]'),head=()=>all('.download-table-header>[role="columnheader"]');
  const widths=()=>head().map(n=>n.getBoundingClientRect().width),handle=i=>one(`[data-download-resize="${i}"]`);
  const ready=()=>!one('#download-content[aria-busy="true"],.download-section-enter-active,.download-section-leave-active');
  const tab=id=>one(`#download-tab-${id}`);
  const fixture=async(mode,details={})=>{window.__downloadPageAck=null;await emit('download-page-fixture',{mode,...details});await wait(()=>window.__downloadPageAck,'fixture '+mode,mode==='seed'?1200:undefined);assert(window.__downloadPageAck.ok,window.__downloadPageAck.message);};
  const pointer=(n,type,x,id=9)=>n.dispatchEvent(new PointerEvent(type,{pointerId:id,pointerType:'mouse',isPrimary:true,button:0,buttons:type==='pointerup'?0:1,clientX:x,clientY:240,bubbles:true,cancelable:true}));
  const bases={song:[45,180,115,90,60],voice:[45,180,90,60],active:[45,180,140]};
  let count=0,step='startup',section='song',downX=0;
  async function select(id){section=id;click(tab(id));await wait(()=>ready()&&tab(id).getAttribute('aria-selected')==='true'&&(id==='mv'?one('.download-mv-card'):first()),`tab ${id}`);await sleep(100);}
  async function capture(label){step=label;await sleep(200);assert(document.documentElement.scrollWidth<=innerWidth,'document overflow');
    if(first()) {
      const scroll=one('.download-scroll');assert(scroll.scrollWidth<=scroll.clientWidth+1,'table overflow');assert(all('[data-download-index]').length<=32,'bounded rows');
      const sizes=widths();near(sizes[0],45,'fixed index');sizes.forEach((value,i)=>assert(value>=bases[section][i]-1,'minimum width'));
      const cells=[...first().children];head().forEach((n,i)=>{near(n.getBoundingClientRect().x,cells[i].getBoundingClientRect().x,'header/body aligned');near(n.getBoundingClientRect().width,cells[i].getBoundingClientRect().width,'same width');});
      for(const n of all('[data-download-resize]')) {const r=n.getBoundingClientRect(),hit=document.elementFromPoint(r.x+r.width/2,r.y+r.height/2);assert(hit===n||n.contains(hit),'resizer hit');near(r.width,8,'8px handle');}
    }
    await emit(`ux_${String(++count).padStart(2,'0')}_${label}`,{width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,section,columns:widths(),sort:head().map(n=>n.getAttribute('aria-sort')),focused:document.activeElement?.outerHTML.slice(0,250),notice:one('.downloads-page .notice')?.textContent});await sleep(400);
  }
  async function dragStart(i,delta){const n=handle(i);downX=n.getBoundingClientRect().x+4;pointer(n,'pointerdown',downX);pointer(document,'pointermove',downX+delta);await sleep(80);}
  async function dragEnd(delta){pointer(document,'pointerup',downX+delta);await sleep(80);}
  async function reset(){handle(1).focus({preventScroll:true});key(handle(1),'Enter');await sleep(80);}
  async function sort(column){click(one(`[data-download-sort="${column}"]`));await wait(()=>ready()&&first(),'sort finishes');await sleep(180);}
  function sorting(column,direction){const n=one(`[data-download-sort="${column}"]`);assert(n.parentElement.getAttribute('aria-sort')===direction,`${column} ${direction}`);}
  function off(){assert(!one('.download-sort-heading .icon'),'no active sort icon');assert(head().filter(n=>n.hasAttribute('aria-sort')).every(n=>n.getAttribute('aria-sort')==='none'),'all sort states off');}
  try {
    await wait(()=>one('.track-row'),'daily ready');await fixture('seed');click(button('下载管理'));await wait(()=>ready()&&first(),'downloads');
    for(const [w,h,theme] of [[1120,760,'light'],[1120,760,'dark'],[760,540,'light'],[760,540,'dark']]) {
      await emit('resize',{width:w,height:h});await wait(()=>innerWidth===w&&innerHeight===h,'resize');click(button('外观主题'));await wait(()=>one('.action-menu'),'theme menu');click(button(theme==='dark'?'深色':'浅色'));await wait(()=>!one('.action-menu'),'theme changed');
      for(const id of ['song','voice','active']) {
        await select(id);await reset();const suffix=`${id}_${w}_${theme}`,normal=widths();assert(all('[data-download-resize]').length===bases[id].length-2,'all nonfinal resizable columns');await capture(`default_${suffix}`);
        for(let i=1;i<bases[id].length-1;i++){const before=widths();await dragStart(i,20);await dragEnd(20);const surplus=before.slice(i+1).reduce((sum,v,j)=>sum+v-bases[id][i+j+1],0);near(widths()[i],before[i]+Math.min(20,surplus),'drag consumes following surplus');}await capture(`drag_${suffix}`);
        key(handle(1),'Home');await sleep(60);near(widths()[1],180,'title minimum');const last=bases[id].length-2;key(handle(last),'End');await sleep(60);near(widths().at(-1),bases[id].at(-1),'last column minimum');await capture(`extremes_${suffix}`);
        await reset();await dragStart(1,15);key(handle(1),'Escape');await sleep(60);assert(!one('.table-column-resizer.dragging'),'cancel clears drag');widths().forEach((v,i)=>near(v,normal[i],'Escape restores'));key(handle(1),'ArrowLeft',{shiftKey:true});await sleep(60);key(handle(1),'Enter');await sleep(60);await capture(`restored_${suffix}`);
      }
    }
    await select('voice');await sort('completed');off();await capture('voice_initial_sort_off');
    for(const [i,direction] of ['ascending','descending','ascending','descending','none'].entries()) {await sort('name');if(direction==='none')off();else sorting('name',direction);assert(one('[data-download-sort="name"]').textContent.includes('歌手')===(i===2||i===3),'artist sort heading');await capture(`voice_title_${i+1}`);}
    for(const column of ['size','completed'])for(const direction of ['ascending','descending','none']) {await sort(column);if(direction==='none')off();else sorting(column,direction);await capture(`voice_${column}_${direction}`);}
    click(button('播放全部',one('.download-toolbar')));await fixture('verify_queue',{ids:Array.from({length:12},(_,i)=>121+i),current:121});click(button('暂停',one('.playerbar')));await capture('voice_unsorted_play_all');
    await dragStart(1,20);await dragEnd(20);const kept=widths();const field=one('.download-search input');field.focus();input(field,'声音 12');await wait(()=>ready()&&first()&&all('[data-download-index]').length<=9,'filter');await sleep(200);widths().forEach((v,i)=>near(v,kept[i],'filter retains column width'));await capture('filter_preserves_width');await sort('name');widths().forEach((v,i)=>near(v,kept[i],'sort retains column width'));await capture('sort_preserves_width');click(button('清空下载搜索'));await wait(()=>ready()&&first(),'clear filter');
    await reset();await dragStart(1,15);await emit('resize',{width:1120,height:760});await wait(()=>innerWidth===1120&&!one('.table-column-resizer.dragging'),'window cancels drag');await capture('resize_during_drag');
    await dragStart(1,20);click(button('每日推荐'));await wait(()=>!one('.downloads-page'),'leave');pointer(document,'pointermove',downX+1000);pointer(document,'pointerup',downX+1000);await capture('leave_during_drag');click(button('下载管理'));await wait(()=>ready()&&first(),'return');await capture('reopen_defaults');
    await select('song');await dragStart(1,20);await select('voice');pointer(document,'pointermove',downX+1000);pointer(document,'pointerup',downX+1000);await sleep(100);const extra=one('.download-scroll').clientWidth-375;const expected=[45,180+extra*360/435,90+extra*45/435,60+extra*30/435];widths().forEach((v,i)=>near(v,expected[i],'category cancels old drag and resets widths'));await capture('category_during_drag');
    const provides=one('#app').__vue_app__._context.provides,client=provides[Reflect.ownKeys(provides).find(k=>typeof k==='symbol'&&k.description==='core-client')],original=client.downloadCommand;
    let mode='hold',release;client.downloadCommand=async(epoch,command)=>{if(mode==='hold')await new Promise(r=>release=r);if(mode==='fail')throw {code:'state',message:'合成暂停恢复失败',retryable:true};return original(epoch,command);};
    await select('active');click(button('全部暂停'));await wait(()=>release&&handle(1).getAttribute('aria-disabled')==='true','pending command');const held=widths();key(handle(1),'ArrowRight');widths().forEach((v,i)=>near(v,held[i],'busy disables resize'));await capture('busy_columns');await select('voice');mode='';release();await wait(()=>one('.downloads-page .notice')?.textContent.includes('暂停全部下载'),'late operation context');await capture('notice_after_category_change');
    await select('active');mode='fail';click(button('全部下载'));await wait(()=>one('.downloads-page .error-state')?.textContent.includes('继续全部下载'),'contextual failure');await select('mv');assert(one('.error-state').textContent.includes('继续全部下载'),'error context survives tab');await capture('error_after_category_change');client.downloadCommand=original;
    await select('song');window.__motionProof.system(true);await wait(()=>document.documentElement.dataset.motion==='reduced','reduced motion');key(handle(1),'ArrowLeft');await capture('reduced_resize');await reset();const beforeCancel=widths();await dragStart(1,20);pointer(document,'pointercancel',downX+20);await sleep(80);widths().forEach((v,i)=>near(v,beforeCancel[i],'pointer cancel'));await capture('pointer_cancel');
    await emit('complete',{ok:true,downloadTableUi:true,stageCount:count,realMusicRequests:0,realAccountWrites:0,syntheticNativeRecords:146,physicalPointerVerified:false});
  }catch(error){const failedAt=step;await capture('failure_state').catch(()=>{});await emit('failed',{step:failedAt,stageCount:count,message:String(error),stack:error.stack});}
})();
