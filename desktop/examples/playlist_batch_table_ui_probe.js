// Official batch column structure, live WebView sizing and selection focus.
(async () => {
  const one=s=>document.querySelector(s), all=s=>[...document.querySelectorAll(s)];
  const sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const assert=(v,m)=>{if(!v) throw Error(m);};
  const wait=async(fn,m)=>{for(let i=0;i<240;i++){if(fn())return;await sleep(50);}throw Error(m);};
  const emit=(stage,data={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...data}});
  const button=(label,root=document)=>[...root.querySelectorAll('button')].find(n=>n.getAttribute('aria-label')===label||n.textContent.trim()===label);
  const click=n=>{assert(n&&!n.disabled,'enabled control');n.scrollIntoView({block:'nearest'});n.focus({preventScroll:true});n.click();};
  const key=(n,k,extra={})=>n.dispatchEvent(new KeyboardEvent('keydown',{key:k,bubbles:true,cancelable:true,...extra}));
  const row=i=>one(`[data-batch-index="${i}"]`), handle=i=>one(`[data-batch-resize="${i}"]`), page=()=>one('.playlist-batch-page');
  const widths=()=>all('.batch-table-head > [role="columnheader"]').map(n=>n.getBoundingClientRect().width);
  const near=(a,b,m)=>assert(Math.abs(a-b)<1.2,`${m}: ${a}, ${b}`);
  const pointer=(n,type,x,id=9)=>n.dispatchEvent(new PointerEvent(type,{pointerId:id,pointerType:'mouse',isPrimary:true,button:0,buttons:type==='pointerup'?0:1,clientX:x,clientY:165,bubbles:true,cancelable:true}));
  const fixture=async mode=>{window.__libraryFixtureAck=null;await emit('library-fixture',{mode});await wait(()=>window.__libraryFixtureAck,'fixture');return window.__libraryFixtureAck;};
  let step='startup',count=0,downX=0;
  async function capture(label) {
    step=label;await sleep(200);
    assert(document.documentElement.scrollWidth<=innerWidth,'document overflow');
    if(page()&&row(0)) {
      const table=one('.batch-table-scroll');assert(table.scrollWidth<=table.clientWidth+1,'batch overflow');
      assert(all('.batch-track-row').length<=32,'bounded rows');
      const columns=widths();near(columns[0],45,'fixed checkbox');near(columns[3],60,'fixed like');
      assert(columns[1]>=179&&columns[2]>=114&&columns[4]>=63,'column minimums');
      const body=[...row(0).children].map(n=>n.getBoundingClientRect());
      all('.batch-table-head > [role="columnheader"]').forEach((n,i)=>near(n.getBoundingClientRect().x,body[i].x,'header/body alignment'));
      for(const h of all('.batch-column-resizer')) {
        const r=h.getBoundingClientRect(),hit=document.elementFromPoint(r.x+r.width/2,r.y+r.height/2);
        assert(hit===h||h.contains(hit),'resizer hit area');
      }
    }
    await emit(`ux_${String(++count).padStart(2,'0')}_${label}`,{width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,columns:page()?widths():[],selection:one('.batch-select-label')?.textContent,focused:document.activeElement?.outerHTML.slice(0,250)});await sleep(420);
  }
  async function enter() {
    click(button('更多歌单操作'));await wait(()=>one('.action-menu')?.style.visibility==='visible','menu');click(button('批量操作',one('.action-menu')));
    await wait(()=>row(0)&&!one('.page-enter-active,.page-leave-active'),'batch ready');await sleep(120);
  }
  async function leave() {click(button('完成',page()));await wait(()=>!page()&&!one('.page-leave-active'),'batch closed');await sleep(120);assert(document.activeElement===button('更多歌单操作'),'return focus');}
  async function dragStart(i,delta) {const n=handle(i);downX=n.getBoundingClientRect().x+4;pointer(n,'pointerdown',downX);pointer(document,'pointermove',downX+delta);await sleep(80);}
  async function dragEnd(delta) {pointer(document,'pointerup',downX+delta);await sleep(80);}
  async function reset() {handle(0).focus();key(handle(0),'Enter');await sleep(60);}
  async function clear() {key(one('.playlist-batch-table'),'a',{ctrlKey:true});await sleep(30);click(one('.batch-select-label'));await sleep(30);}
  try {
    await wait(()=>one('.track-more'),'daily');click(button('我的歌单'));await wait(()=>one('[data-library-open="7"]'),'library');click(one('[data-library-open="7"]'));await wait(()=>one('.playlist-hero')&&!one('.page-enter-active,.page-leave-active'),'detail');
    for(const [w,h,theme] of [[1120,760,'light'],[1120,760,'dark'],[760,540,'light'],[760,540,'dark']]) {
      await emit('resize',{width:w,height:h});await wait(()=>innerWidth===w&&innerHeight===h,'resize');click(button('外观主题'));await wait(()=>one('.action-menu'),'theme menu');click(button(theme==='dark'?'深色':'浅色'));await wait(()=>!one('.action-menu'),'theme');
      await enter();const suffix=`${w}_${theme}`;
      assert(all('.batch-table-head > [role="columnheader"]').length===5,'official five columns');
      assert(all('.batch-table-head > [role="columnheader"]').slice(2).map(n=>n.textContent.trim()).join('|')==='专辑|喜欢|时长','official header order');
      near(parseFloat(getComputedStyle(row(0).querySelector('.batch-track-name')).fontSize),16,'title typography');
      assert(row(0).querySelector('.batch-track-subtitle').getBoundingClientRect().y>row(0).querySelector('.batch-track-name').getBoundingClientRect().y+16,'artist under title');
      assert(row(2).querySelector('[aria-label="已喜欢"]')&&row(0).querySelector('[aria-label="未喜欢"]'),'read-only account like state');
      const normal=widths();await capture(`columns_${suffix}`);
      click(row(2).querySelector('.batch-like-status'));await sleep(50);assert(row(2).getAttribute('aria-selected')==='true'&&row(2).querySelector('[aria-label="已喜欢"]'),'heart area only selects row');await capture(`liked_selection_${suffix}`);
      await dragStart(0,30);await dragEnd(30);near(widths()[1],Math.min(normal[1]+30,normal[1]+normal[2]-115+normal[4]-64),'title grows');assert(row(2).getAttribute('aria-selected')==='true','resize preserves selection');await capture(`title_resize_${suffix}`);
      key(handle(0),'Home');await sleep(60);near(widths()[1],180,'title minimum');await capture(`title_min_${suffix}`);
      key(handle(1),'End');await sleep(60);near(widths()[4],64,'album consumes duration surplus');await capture(`album_max_${suffix}`);
      await reset();widths().forEach((n,i)=>near(n,normal[i],'keyboard reset'));await capture(`reset_${suffix}`);
      await dragStart(0,25);key(handle(0),'Escape');await sleep(80);assert(page()&&!one('.batch-column-resizer.dragging'),'Escape cancels resize without leaving');widths().forEach((n,i)=>near(n,normal[i],'cancel restores widths'));await capture(`drag_cancel_${suffix}`);
      await clear();click(row(1));click(row(4));await sleep(50);row(1).focus({preventScroll:true});key(row(1),'ArrowDown');await wait(()=>document.activeElement===row(5),'selection edge down');assert(row(5).getAttribute('aria-selected')==='true'&&one('.batch-select-label').textContent.includes('已选 1'),'down uses last selected edge');
      key(row(5),'ArrowUp');await wait(()=>document.activeElement===row(4),'selection edge up');await capture(`keyboard_${suffix}`);
      await leave();
    }
    await enter();await clear();click(button('完成',page()));await wait(()=>!page(),'close before focus proof');await sleep(120);await enter();
    handle(1).focus();one('.batch-table-scroll').focus();await wait(()=>document.activeElement===row(0)&&row(0).getAttribute('aria-selected')==='true','tab entry selects first');await capture('body_entry');
    await reset();const before=widths();await dragStart(1,-20);pointer(document,'pointercancel',downX-20);await sleep(60);widths().forEach((n,i)=>near(n,before[i],'pointer cancel restores'));await capture('pointer_cancel');
    await dragStart(0,20);await emit('resize',{width:1120,height:760});await wait(()=>innerWidth===1120&&!one('.batch-column-resizer.dragging'),'window resize ends transaction');await capture('window_resize');
    await dragStart(0,30);click(button('每日推荐'));await wait(()=>!page()&&!one('.page-leave-active'),'leave during drag');pointer(document,'pointermove',downX+1000);pointer(document,'pointerup',downX+1000);await capture('leave_during_drag');
    click(button('我的歌单'));await wait(()=>one('[data-library-open="7"]'),'library');click(one('[data-library-open="7"]'));await wait(()=>one('.playlist-hero')&&!one('.page-leave-active,.page-enter-active'),'detail');await enter();await capture('reopen_defaults');
    click(row(0));await sleep(40);await fixture('hold_detail');click(button('添加到播放列表',page()));await wait(()=>one('.batch-operation-feedback')?.textContent.includes('正在添加'),'operation wait');assert(handle(0).getAttribute('aria-disabled')==='true','disable resizer during mutation');const held=widths();key(handle(0),'ArrowRight');widths().forEach((n,i)=>near(n,held[i],'disabled handle does not move'));await capture('operation_wait');
    await fixture('success');await wait(()=>one('.batch-operation-feedback')?.textContent.includes('已添加'),'operation finished');assert(handle(0).getAttribute('aria-disabled')==='false','resizer restores');await capture('operation_done');
    window.__motionProof.system(true);await wait(()=>document.documentElement.dataset.motion==='reduced','reduced');key(handle(0),'ArrowRight');await capture('reduced');
    await emit('complete',{ok:true,playlistBatchTableUi:true,stageCount:count,realMusicRequests:0,realAccountWrites:0});
  } catch(error) {const last=step;await capture('failure_state').catch(()=>{});await emit('failed',{step:last,message:String(error),stack:error.stack,stageCount:count});}
})();
