// Actual desktop IPC over synthetic, sandbox-owned downloads. Never submits a playlist write.
(async () => {
 const one=s=>document.querySelector(s), all=s=>[...document.querySelectorAll(s)];
 const sleep=ms=>new Promise(r=>setTimeout(r,ms)), assert=(ok,m)=>{if(!ok)throw Error(m);};
 const wait=async(fn,m)=>{for(let i=0;i<200;i++){await sleep(50);if(fn())return;}throw Error(m);};
 const emit=(stage,data={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...data}});
 const button=(label,root=document)=>[...root.querySelectorAll('button')].find(n=>n.getAttribute('aria-label')===label||n.textContent.trim()===label);
 const click=n=>{assert(n&&!n.disabled,'enabled '+n?.outerHTML);n.focus({preventScroll:true});n.click();};
 const key=(n,k,extra={})=>n.dispatchEvent(new KeyboardEvent('keydown',{key:k,bubbles:true,cancelable:true,...extra}));
 const type=(n,text)=>{n.value=text;n.dispatchEvent(new Event('input',{bubbles:true}));};
 const page=()=>one('.download-batch-page'), row=i=>one('[data-batch-index="'+i+'"]');
 const field=()=>one('.download-search input'), selected=()=>one('.download-batch-table .batch-select-label')?.textContent;
 const done=()=>one('.download-batch-page .batch-done'), menu=()=>button('更多下载操作');
 const titleId=r=>Number(r.querySelector('.download-track-name').textContent.match(/(\d+) ·/)[1]);
 const fixture=async(mode,details={})=>{window.__downloadPageAck=null;await emit('download-page-fixture',{mode,...details});await wait(()=>window.__downloadPageAck,'fixture');assert(window.__downloadPageAck.ok,window.__downloadPageAck.message);};
 let count=0,step='initial',category='song';
 async function enter(ready=true){
  click(menu());await wait(()=>one('.action-menu')?.style.visibility==='visible','menu');click(button('批量操作',one('.action-menu')));
  await wait(()=>page()&&!one('.download-batch-page.page-enter-active'),'batch route');if(ready)await wait(()=>row(0),'batch data');
 }
 async function close(back=false){
  if(back)click(one('[aria-label="返回上一页"]'));else click(done());
  await wait(()=>!page()&&document.activeElement===menu(),'origin focus');
 }
 async function section(id){category=id;click(one('#download-tab-'+id));await wait(()=>one('#download-tab-'+id).getAttribute('aria-selected')==='true'&&one('.download-table')&&!one('.download-section-enter-active,.download-section-leave-active,#download-content[aria-busy="true"]'),'category');}
 function playerBounds(){
  for(const n of all('.playerbar select,.playerbar button,.playerbar input').filter(n=>n.getClientRects().length)){
   const r=n.getBoundingClientRect(),hit=document.elementFromPoint(r.x+r.width/2,r.y+r.height/2);assert(r.left>=0&&r.right<=innerWidth+1&&n.contains(hit),'playing control reachable '+(n.getAttribute('aria-label')||n.title));}
  const hint=one('.actual-quality'),end=one('.seek-control time:last-child');assert(hint&&end,'actual quality and duration visible');
  const a=hint.getBoundingClientRect(),b=end.getBoundingClientRect();assert(a.left>=b.right||a.top>=b.bottom||a.bottom<=b.top,'quality note does not cover duration');
 }
 async function capture(label){
  step=label;await sleep(225);
  assert(document.documentElement.scrollWidth<=innerWidth,'page width');
  if(page()){
   const b=page().getBoundingClientRect();assert(b.width>0&&b.left>=0&&b.right<=innerWidth+1,'page bounds');
   assert(all('[data-batch-index]').length<=32,'bounded');
   const body=one('.download-batch-page .batch-table-scroll');
   if(body)assert(body.scrollWidth<=body.clientWidth+1,'table width');
   if(row(0)){const heads=all('.download-batch-table [role=columnheader]');assert(heads.length===(category==='song'?5:4),'columns');
    heads.forEach((h,i)=>{const cell=row(0).children[i],a=h.getBoundingClientRect(),z=cell.getBoundingClientRect();assert(Math.abs(a.left-z.left)<1&&Math.abs(a.width-z.width)<1,'aligned columns');
       if(i>=2){const textStart=n=>{const range=document.createRange();range.selectNodeContents(n.childNodes[0]);return range.getBoundingClientRect().left;};const header=h.querySelector('button');assert(Math.abs(textStart(header)-textStart(cell))<1,'header text aligns with row text');}});}
   if(!one('dialog[open]'))for(const n of all('.download-batch-page .batch-toolbar button')){
    const b=n.getBoundingClientRect(),hit=document.elementFromPoint(b.x+b.width/2,b.y+b.height/2);
    assert(b.height===40&&n.contains(hit),'toolbar target');}
  }
  await emit('ux_'+String(++count).padStart(2,'0')+'_'+label,{width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,category,selected:selected(),focus:document.activeElement?.outerHTML.slice(0,160)});
  await sleep(330);
 }
 try{
  await wait(()=>one('.track-row'),'daily');await fixture('seed');click(button('下载管理'));await wait(()=>menu()&&one('[data-download-index="0"]'),'downloads');
  for(const [w,h,theme] of [[1120,760,'light'],[1120,760,'dark'],[760,540,'light'],[760,540,'dark']]){
   await emit('resize',{width:w,height:h});await wait(()=>innerWidth===w&&innerHeight===h,'size');click(button('外观主题'));await wait(()=>one('.action-menu'),'theme');click(button(theme==='dark'?'深色':'浅色'));await wait(()=>!one('.action-menu'),'theme closed');
   for(const id of ['song','voice']){
    const suffix=id+'_'+w+'_'+theme,n=id==='song'?120:12;
    await section(id);await enter();assert(document.activeElement===done(),'entry focus');
    assert(!!button('收藏',page())===(id==='song'),'category actions');assert(button('播放所选下载',page()).disabled,'zero selection');
    await capture('default_'+suffix);
    click(one('[aria-label="全选下载"]'));await wait(()=>selected().includes(String(n)),'full category selected');await capture('selected_'+suffix);
    click(one('[aria-label="全选下载"]'));click(row(0));row(2).dispatchEvent(new MouseEvent('click',{bubbles:true,shiftKey:true}));await wait(()=>selected().includes('3 '),'range selection');
    const hSort=button('下载时间',page());click(hSort);await wait(()=>hSort.parentElement.getAttribute('aria-sort')==='ascending','ascending');await capture('sort_'+suffix);
    click(hSort);await wait(()=>hSort.parentElement.getAttribute('aria-sort')==='descending','descending');click(hSort);await wait(()=>hSort.parentElement.getAttribute('aria-sort')==='none','base order');
    click(button('删除',page()));await wait(()=>one('.download-batch-confirm[open]'),'confirm');assert(document.activeElement===button('取消',one('dialog')),'cancel default');await capture('confirm_'+suffix);
    click(button('取消',one('dialog')));await wait(()=>!one('dialog')&&document.activeElement===button('删除',page()),'cancel focus');await close();
   }
  }
  // At 125% WebView zoom, a 760×540 window has only ~608×432 CSS pixels.
  // Exercise each target by hit testing rather than relying on root scrollWidth.
  await emit('compact-zoom',{enabled:true});await wait(()=>Math.abs(innerWidth-608)<=1,'scaled viewport');
  for(const id of ['song','voice']){
   click(button('外观主题'));await wait(()=>one('.action-menu'),'compact theme');click(button(id==='song'?'浅色':'深色',one('.action-menu')));await wait(()=>!one('.action-menu'),'theme closed');
   await section(id);await enter();click(one('[aria-label="全选下载"]'));
   await wait(()=>selected().includes(id==='song'?'120':'12'),'compact selection');
   const controls=all('.window-controls button,.settings-button,.appearance-menu button,.search-host input,.account-button,.header-spacer,.playerbar button,.playerbar input,.playerbar select,.batch-toolbar button').filter(n=>n.getClientRects().length);
   assert(all('.window-controls button').length===4,'four custom window actions');
   for(const n of controls){const r=n.getBoundingClientRect(),hit=document.elementFromPoint(r.x+r.width/2,r.y+r.height/2);assert(r.width>0&&r.x>=0&&r.right<=innerWidth+1&&n.contains(hit),'compact hit '+(n.getAttribute('aria-label')||n.className));}
   await capture('compact_'+id);await close();
  }
  await emit('compact-zoom',{enabled:false});await wait(()=>innerWidth===760,'restore normal scale');
  await section('song');type(field(),'单曲 119');await wait(()=>one('.download-table')?.getAttribute('aria-rowcount')==='2','filtered parent');await enter();assert(one('.download-batch-table').getAttribute('aria-rowcount')==='121','ignores parent keyword');await capture('full_from_filter');await close(true);assert(field().value==='单曲 119'&&one('.download-table').getAttribute('aria-rowcount')==='2','query restored');await capture('return_filter');
  click(button('清空下载搜索'));await wait(()=>one('.download-table')?.getAttribute('aria-rowcount')==='121','parent reset');const scroller=one('.download-scroll');scroller.scrollTop=1800;scroller.dispatchEvent(new Event('scroll'));await wait(()=>!one('[data-download-index="0"]'),'source scrolled');const top=scroller.scrollTop;
  await enter();click(row(0));key(row(0),'End');await wait(()=>document.activeElement===row(119),'virtual end');key(row(119),'a',{ctrlKey:true});await wait(()=>selected().includes('120'),'keyboard all beyond page 1');await capture('keyboard_end');await close();assert(Math.abs(scroller.scrollTop-top)<2,'source scroll restored');await capture('scroll_restored');
  await enter();click(row(1));await wait(()=>!button('收藏',page()).disabled,'collect selection');click(button('收藏',page()));await wait(()=>one('#collect-title'),'collect');await capture('collection');key(one('dialog'),'Escape');if(one('dialog[open]'))one('dialog').dispatchEvent(new Event('cancel',{cancelable:true}));await wait(()=>!one('dialog'), 'collect return');
  const current=titleId(row(1)),added=titleId(row(2));
  click(button('添加到播放列表',page()));await fixture('verify_queue',{ids:[current],current});await capture('queue_starts');
  click(row(1));click(row(2));click(button('添加到播放列表',page()));await fixture('verify_queue',{ids:[current,added],current});await capture('queue_keeps_current');
  click(button('播放所选下载',page()));await fixture('verify_queue',{ids:[added],current:added});await capture('play_selected');playerBounds();
  await emit('compact-zoom',{enabled:true});await wait(()=>Math.abs(innerWidth-608)<=1,'playing compact');await capture('compact_playing');playerBounds();await emit('compact-zoom',{enabled:false});await wait(()=>innerWidth===760,'playing zoom restored');
  click(button('暂停',one('.playerbar')));await close();
  const provides=one('#app').__vue_app__._context.provides,client=provides[Reflect.ownKeys(provides).find(k=>typeof k==='symbol'&&k.description==='core-client')], original=client.downloads;
  let mode='wait',release;client.downloads=async(...args)=>{if(args[1].limit===1000000){if(mode==='wait')await new Promise(r=>{release=r;});if(mode==='fail')throw {code:'connection',message:'合成批量读取失败'};const result=await original(...args);if(mode==='empty')return {...result,total:0,tasks:[]};if(mode==='partial')return {...result,total:result.total+1};if(mode==='vanish')return {...result,total:result.total-1,tasks:result.tasks.slice(1)};return result;}return original(...args);};
  await enter(false);await wait(()=>release&&page().textContent.includes('正在读取全部下载'),'batch read pending');await capture('loading');mode='fail';release();await wait(()=>page().textContent.includes('合成批量读取失败'),'batch failure');await capture('read_error');mode='';click(button('重新读取',page()));await wait(()=>row(0)&&document.activeElement===done(),'retry returns focus');await capture('read_recovered');await close();
  mode='wait';release=null;await enter(false);await wait(()=>release,'delayed read');await close();mode='';release();await sleep(140);assert(!page(),'late read does not reopen');await capture('late_read');
  mode='empty';await enter(false);await wait(()=>page().textContent.includes('没有已下载单曲'),'empty');await capture('empty');await close();
  mode='partial';await enter(false);await wait(()=>page().querySelector('[role=alert]')?.textContent.includes('完整读取'),'truncated snapshot rejected');assert(!one('.download-batch-table'),'partial selection not offered');await capture('partial_read');await close();
  mode='';await enter();click(row(0));await wait(()=>!button('删除',page()).disabled,'selected stale candidate');click(button('删除',page()));await wait(()=>one('.download-batch-confirm[open]'),'confirm stable list');mode='vanish';window.dispatchEvent(new Event('focus'));await wait(()=>one('.download-batch-confirm [role=alert]')?.textContent.includes('已改变'),'changed snapshot');assert(button('删除',one('dialog')).disabled,'stale confirmation disabled');await capture('confirm_changed');click(button('取消',one('dialog')));await wait(()=>!one('dialog'),'changed confirmation cancelled');await close();client.downloads=original;
  window.__motionProof.system(true);await section('voice');await enter();await capture('reduced');click(row(0));const rm=titleId(row(0)),keep=titleId(row(1));await wait(()=>!button('删除',page()).disabled,'voice selection');
  const originalCommand=client.downloadCommand;let finish,requests=0;
  client.downloadCommand=async(...args)=>{if(args[1].kind==='delete_files'){requests++;await new Promise(r=>{finish=r;});}return originalCommand(...args);};
  click(button('删除',page()));await wait(()=>one('.download-batch-confirm[open]'),'delete');click(button('删除',one('dialog')));await wait(()=>finish&&button('取消',one('dialog')).disabled,'writing');await capture('delete_pending');finish();await wait(()=>!one('dialog')&&page().textContent.includes('已删除 1 项下载')&&one('.download-batch-table')?.getAttribute('aria-rowcount')==='12','deletion completed');assert(requests===1,'one submitted delete');await fixture('verify_batch_delete',{removed:rm,retained:keep});await capture('deleted');client.downloadCommand=originalCommand;
  assert(count===53,'53 batch states: '+count);await emit('complete',{ok:true,downloadBatchUi:true,stageCount:count,syntheticNativeRecords:146,realMusicRequests:0,realAccountWrites:0});
 }catch(e){await emit('failed',{step,stageCount:count,message:String(e),stack:e?.stack,body:document.body.innerText.slice(-8000)});}
})();
