// Production download pages over synthetic native records, never remote media.
(async () => {
  const one=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)];
  const sleep=ms=>new Promise(r=>setTimeout(r,ms)),assert=(ok,m)=>{if(!ok)throw Error(m);};
  const wait=async(fn,m,attempts=240)=>{await sleep(25);for(let i=0;i<attempts;i++){if(fn())return;await sleep(50);}throw Error(m);};
  const emit=(stage,data={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...data}});
  const button=(label,root=document)=>[...root.querySelectorAll('button')].find(n=>n.getAttribute('aria-label')===label||n.textContent.trim()===label);
  const click=n=>{assert(n&&!n.disabled,`enabled ${n?.outerHTML}`);n.focus({preventScroll:true});n.click();};
  const key=(n,k)=>n.dispatchEvent(new KeyboardEvent('keydown',{key:k,bubbles:true,cancelable:true}));
  const input=(n,text)=>{n.value=text;n.dispatchEvent(new Event('input',{bubbles:true}));};
  const scrollFrame=async()=>{await new Promise(requestAnimationFrame);await new Promise(requestAnimationFrame);};
  const rect=n=>{const r=n.getBoundingClientRect();return {x:r.x,y:r.y,width:r.width,height:r.height,right:r.right,bottom:r.bottom};};
  const near=(a,b,m)=>assert(Math.abs(a-b)<1.2,`${m}: ${a}/${b}`);
  const fixture=async (mode,details={})=>{window.__downloadPageAck=null;await emit('download-page-fixture',{mode,...details});await wait(()=>window.__downloadPageAck,'fixture response '+mode,mode==='seed'?1200:undefined);assert(window.__downloadPageAck.ok,window.__downloadPageAck.message);};
  const tab=id=>one(`#download-tab-${id}`),ready=()=>!one('#download-content[aria-busy="true"],.download-section-enter-active,.download-section-leave-active');
  const first=()=>one('[data-download-index="0"]');
  let count=0,stage='startup';
  async function select(id){click(tab(id));await wait(()=>tab(id).getAttribute('aria-selected')==='true'&&ready()&& (id==='mv'?one('.download-mv-card'):first()),`tab ${id}`);await sleep(100);}
  async function capture(label,extra={}){stage=label;await sleep(250);assert(document.documentElement.scrollWidth<=innerWidth,'document overflow');
    for(const n of all('.download-navigation,.download-toolbar,.download-table-header').filter(n=>n.getClientRects().length)) {const r=rect(n);assert(r.x>=0&&r.right<=innerWidth+1,'page bounds');}
    assert(all('[data-download-index]').length<=32,'bounded table');assert(all('.download-mv-card').length<=32,'bounded grid');
    const card=one('.download-mv-card');if(card){const title=card.querySelector('.download-mv-title'),credit=card.querySelector('.download-mv-artists'),range=document.createRange();range.selectNodeContents(title);near(range.getBoundingClientRect().left,rect(credit).x,'MV title text aligns with artist');}
    if(first()&&!one('dialog,.download-batch-page')) {
      near(rect(first()).height,64,'default row height');near(parseFloat(getComputedStyle(one('.download-track-name')).fontSize),16,'title size');
      const cells=[...first().children];all('.download-table-header>[role="columnheader"]').forEach((n,i)=>{near(rect(n).x,rect(cells[i]).x,'column alignment');near(rect(n).width,rect(cells[i]).width,'column widths');});
      const scroller=one('.download-scroll');assert(scroller.scrollWidth<=scroller.clientWidth+1,'table overflow');
    }
    if(!one('dialog,.download-batch-page')) for(const n of all('.download-tabs button,.download-navigation-actions button,.download-toolbar>button')) {const r=rect(n);const hit=document.elementFromPoint(r.x+r.width/2,r.y+r.height/2);assert(n.contains(hit),'header action hit');}
    await emit(`ux_${String(++count).padStart(2,'0')}_${label}`,{width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,section:one('.download-tabs [aria-selected="true"]')?.textContent,rows:all('[data-download-index]').length,cards:all('.download-mv-card').length,focused:document.activeElement?.outerHTML.slice(0,200),...extra});await sleep(400);
  }
  async function closeDialog(){click(button('关闭下载对话框'));await wait(()=>!one('dialog'),'dialog closes');}
  try {
    await wait(()=>one('.track-row'),'initial page');await fixture('seed');click(button('下载管理'));await wait(()=>first()&&ready(),'download records');
    for(const [w,h,theme] of [[1120,760,'light'],[1120,760,'dark'],[760,540,'light'],[760,540,'dark']]) {
      await emit('resize',{width:w,height:h});await wait(()=>innerWidth===w&&innerHeight===h,'window resize');click(button('外观主题'));await wait(()=>one('.action-menu'),'theme menu');click(button(theme==='dark'?'深色':'浅色'));await wait(()=>!one('.action-menu'),'theme applied');
      const suffix=`${w}_${theme}`;
      await select('song');assert(all('.download-table-header>[role="columnheader"]').length===5,'song columns');assert(one('.download-table').getAttribute('aria-rowcount')==='121'&&!one('.download-pagination'),'songs include cloud and have no page controls');await capture(`songs_${suffix}`);
      const songScroll=one('.download-scroll');songScroll.scrollTop=songScroll.scrollHeight;songScroll.dispatchEvent(new Event('scroll'));await wait(()=>!one('.download-continuation')&&all('[data-download-index]').some(n=>Number(n.dataset.downloadIndex)>99),'continuous growth');songScroll.scrollTop=songScroll.scrollHeight;songScroll.dispatchEvent(new Event('scroll'));await wait(()=>one('[data-download-index="119"]'),'last download reachable');await capture(`continuous_${suffix}`);songScroll.scrollTop=0;songScroll.dispatchEvent(new Event('scroll'));await wait(()=>first(),'return after growth');
      first().focus({preventScroll:true});const manage=first().querySelector('[aria-label^="管理下载"]');click(manage);await wait(()=>one('.action-menu'),'file actions menu');click(button('管理下载文件…',one('.action-menu')));await wait(()=>one('.download-dialog[open]'),'file dialog');await capture(`file_${suffix}`);await closeDialog();assert(document.activeElement===manage,'file menu return focus');
      await select('voice');assert(all('.download-table-header>[role="columnheader"]').length===4&&!one('.download-pagination'),'voice classified');assert(first().textContent.includes('合成声音'),'voice resource');await capture(`voices_${suffix}`);
      await select('mv');const grid=one('.download-mv-scroll');const ids=new Set(all('.download-mv-card').map(n=>n.dataset.downloadMv));assert(all('.download-mv-card').every(n=>n.textContent.includes('合成MV')),'MV source');grid.scrollTop=grid.scrollHeight;grid.dispatchEvent(new Event('scroll'));await sleep(180);all('.download-mv-card').forEach(n=>ids.add(n.dataset.downloadMv));assert(ids.size===9,'all MV records reachable through virtualization');grid.scrollTop=0;grid.dispatchEvent(new Event('scroll'));await sleep(180);const cover=one('.download-mv-cover'),r=rect(cover);near(r.height,r.width*.565,'MV aspect');await capture(`mvs_${suffix}`);
      await select('active');assert(all('.download-table-header>[role="columnheader"]').length===3&&all('[data-download-index]').length===5,'active all resources');for(const n of all('.download-toolbar>.download-button')){near(rect(n).width,114,'action width');near(rect(n).height,48,'action height');}assert(one('.download-progress.failed'),'failure row');const paused=all('.download-progress-status').find(n=>n.textContent==='已暂停');assert(paused&&paused.scrollWidth<=paused.clientWidth+1,'paused label fits');await capture(`active_${suffix}`);
      click(button('全部清空'));await wait(()=>one('.download-dialog[open]'),'clear confirm');await capture(`clear_${suffix}`);await closeDialog();assert(document.activeElement===button('全部清空'),'clear return focus');
    }
    await emit('resize',{width:1120,height:760});await wait(()=>innerWidth===1120,'wide controls');await select('song');
    for(const [label,expected] of [['name_ascending','合成单曲 001'],['name_descending','合成单曲 120'],['artist_ascending',null],['artist_descending',null]]) {
      click(one('[data-download-sort="name"]'));await wait(()=>ready()&&first(),'sort result');if(expected)assert(first().textContent.includes(expected),label);assert(document.activeElement===one('[data-download-sort="name"]'),'sort returns focus');await capture(label);
    }
    const field=one('.download-search input');field.focus();input(field,'单曲 119');await wait(()=>ready()&&all('[data-download-index]').length===1,'search filtering');assert(first().textContent.includes('119'),'search result');await capture('search_found');
    input(field,'不匹配的下载');await wait(()=>one('.empty-state')?.textContent.includes('没有找到'),'search empty');await capture('search_empty');click(button('清空下载搜索'));await wait(()=>ready()&&first(),'clear search');assert(document.activeElement===field,'clear search focus');await capture('search_clear');
    await select('voice');tab('voice').focus();key(tab('voice'),'ArrowRight');await wait(()=>tab('mv').getAttribute('aria-selected')==='true'&&one('.download-mv-card'),'keyboard tab');assert(document.activeElement===tab('mv'),'roving tabs');await capture('keyboard_tabs');
    await select('song');const pageScroll=one('.download-scroll');pageScroll.scrollTop=pageScroll.scrollHeight;pageScroll.dispatchEvent(new Event('scroll'));await wait(()=>!one('.download-continuation'),'continuous full prefix');const pageIds=new Set();for(let top=0;top<=pageScroll.scrollHeight;top+=pageScroll.clientHeight){pageScroll.scrollTop=top;pageScroll.dispatchEvent(new Event('scroll'));await sleep(70);all('[data-download-index]').forEach(n=>pageIds.add(n.dataset.downloadTask));}assert(pageIds.size===120,'all continuous records reachable');await capture('continuous_inventory');pageScroll.scrollTop=0;pageScroll.dispatchEvent(new Event('scroll'));await wait(()=>first(),'first after inventory');
    const scroller=one('.download-scroll');first().focus({preventScroll:true});scroller.scrollTop=scroller.scrollHeight;scroller.dispatchEvent(new Event('scroll'));await wait(()=>!one('[data-download-index="0"]'),'virtual recycle');assert(document.activeElement===scroller,'recycled focus body');await capture('virtual_focus');key(scroller,'Home');await wait(()=>document.activeElement===first(),'keyboard home');
    click(button('更多下载操作'));await wait(()=>one('.action-menu')?.style.visibility==='visible','page menu');await capture('more_menu');click(button('批量操作',one('.action-menu')));await wait(()=>one('.download-batch-page [aria-label="全选下载"]'),'complete batch ready');assert(document.activeElement===one('.download-batch-page .batch-done'),'batch entry focus');click(one('[aria-label="全选下载"]'));await wait(()=>one('.download-batch-page .batch-select-label').textContent.includes('120'),'selects whole category');await capture('batch_selection');click(one('.download-batch-page .batch-done'));await wait(()=>!one('.download-batch-page')&&document.activeElement===button('更多下载操作'),'batch return focus');
    await fixture('read_error');click(tab('voice'));await wait(()=>one('.error-state')&&ready(),'read failure');assert(!first(),'no stale rows after failed category read');await capture('read_error');await fixture('repair_read');click(button('刷新状态'));await wait(()=>ready()&&first(),'read recovery');await capture('read_recovered');
    await select('active');const before=Number(one('progress').value);await fixture('progress');await wait(()=>Number(one('progress').value)>before+.005,'native progress update');await capture('progress');
    click(button('全部暂停'));await wait(()=>all('.download-progress-status').filter(n=>n.textContent==='已暂停').length===4&&all('.download-progress.failed').length===1,'pause runnable tasks, preserve existing failure');await fixture('verify_pause');await capture('paused_all');
    await fixture('empty');await wait(()=>one('.empty-state')?.textContent.includes('没有正在下载'),'empty tasks');await capture('empty_active');await fixture('restore');await wait(()=>first(),'restore records');
    window.__motionProof.system(true);await wait(()=>document.documentElement.dataset.motion==='reduced','reduced motion');await select('mv');await capture('reduced_mv');
    window.__motionProof.system(false);await wait(()=>document.documentElement.dataset.motion!=='reduced','full motion restored');
    const pausePlayer=()=>click(button('暂停',one('.playerbar')));
    await select('song');assert(one('.download-continuation'),'only first window loaded');click(button('播放全部',one('.download-toolbar')));await fixture('verify_queue',{ids:Array.from({length:120},(_,i)=>i+1),current:1});pausePlayer();await capture('play_all_before_growth');
    const search=one('.download-search input');search.focus();input(search,'单曲 01');await wait(()=>ready()&&all('[data-download-index]').length===10,'play filter');click(one('[data-download-sort="name"]'));await wait(()=>ready()&&first(),'name ascending');click(one('[data-download-sort="name"]'));await wait(()=>ready()&&first()?.textContent.includes('019'),'name descending');click(button('播放全部',one('.download-toolbar')));await fixture('verify_queue',{ids:Array.from({length:10},(_,i)=>19-i),current:19});pausePlayer();await capture('play_filtered_order');
    const chosen=all('[data-download-index]').find(n=>n.textContent.includes('合成单曲 017'));chosen.focus({preventScroll:true});key(chosen,'Enter');await fixture('verify_queue',{ids:Array.from({length:10},(_,i)=>19-i),current:17});pausePlayer();await capture('play_filtered_row');
    await select('voice');click(button('播放全部',one('.download-toolbar')));await fixture('verify_queue',{ids:Array.from({length:12},(_,i)=>121+i),current:121});pausePlayer();await capture('play_voice_all');
    let mode='',releaseGrowth,readStarted=false;const provides=one('#app').__vue_app__._context.provides;const client=provides[Reflect.ownKeys(provides).find(key=>typeof key==='symbol'&&key.description==='core-client')];const originalDownloads=client.downloads;
    // Only inject delay/failure at the existing typed adapter in this isolated
    // probe. Tauri's internal invoke property is intentionally read-only.
    const intercept=async(epoch,query)=>{if(query.limit>100&&mode){readStarted=true;if(mode==='hold')await new Promise(r=>releaseGrowth=r);if(mode==='fail')throw {code:'state',message:'合成后续读取失败',retryable:true};}return originalDownloads(epoch,query);};client.downloads=intercept;assert(client.downloads===intercept,'typed download fault adapter');
    await select('song');mode='hold';readStarted=false;let scroll=one('.download-scroll');scroll.scrollTop=scroll.scrollHeight;scroll.dispatchEvent(new Event('scroll'));await wait(()=>readStarted&&one('.download-continuation .spinner'),'growing wait');assert(all('[data-download-index]').every(n=>Number(n.dataset.downloadIndex)<100),'old prefix preserved while loading');await capture('growth_wait');mode='fail';releaseGrowth();await wait(()=>button('重试加载'),'growth failure');assert(!one('.error-state'),'growth failure does not replace page');await capture('growth_failed');mode='';click(button('重试加载'));await wait(()=>!one('.download-continuation'),'growth retry');assert(document.activeElement?.matches('[data-download-index]'),'growth retry returns to visible row');await capture('growth_retried');
    await select('voice');await select('song');await fixture('remove_first');scroll=one('.download-scroll');scroll.scrollTop=scroll.scrollHeight;scroll.dispatchEvent(new Event('scroll'));await wait(()=>!one('.download-continuation')&&one('.download-table').getAttribute('aria-rowcount')==='120','changed prefix');const freshIds=new Set();for(let top=0;top<=scroll.scrollHeight;top+=scroll.clientHeight){scroll.scrollTop=top;scroll.dispatchEvent(new Event('scroll'));await sleep(70);all('[data-download-index]').forEach(n=>freshIds.add(n.dataset.downloadTask));}assert(freshIds.size===119,'changed prefix has no skipped or duplicate records');await capture('growth_changed_inventory');await fixture('restore');
    await select('voice');await select('song');mode='hold';readStarted=false;scroll=one('.download-scroll');scroll.scrollTop=scroll.scrollHeight;scroll.dispatchEvent(new Event('scroll'));await wait(()=>readStarted,'held second window');await select('voice');mode='';releaseGrowth();await sleep(250);assert(tab('voice').getAttribute('aria-selected')==='true'&&first().textContent.includes('合成声音'),'late window cannot restore previous category');await capture('growth_late_category');client.downloads=originalDownloads;
    await fixture('mv_window');
    for(const [w,h] of [[1120,760],[760,540]]){
      await emit('resize',{width:w,height:h});await wait(()=>innerWidth===w,'MV continuation resize');
      await select('voice');await select('mv');const grid=one('.download-mv-scroll');
      assert(one('.download-continuation'),'MV first window has more');
      grid.scrollTop=grid.scrollHeight;grid.dispatchEvent(new Event('scroll'));
      await wait(()=>!one('.download-continuation'),'MV continuation completed');assert(grid.scrollTop>0,'MV growth preserves scroll');
      const ids=new Set(),trace=[];
      for(let top=0;top<=grid.scrollHeight;top+=grid.clientHeight){
        const y=Math.min(top,grid.scrollHeight-grid.clientHeight);
        grid.scrollTop=y;grid.dispatchEvent(new Event('scroll'));
        // A refresh, scroll event and the virtual row patch can cross a frame.
        // Check the actual requested viewport and the rows covering it.
        await wait(()=>{
          const rendered=all('.download-mv-row');
          return Math.abs(grid.scrollTop-y)<=1 && rendered.length && rect(rendered[0]).y<=rect(grid).y+1 && rect(rendered.at(-1)).bottom>=rect(grid).bottom-1;
        },'MV viewport '+w+' target '+y);
        const cards=all('.download-mv-card');cards.forEach(n=>ids.add(n.dataset.downloadMv));
        trace.push([y,grid.scrollTop,grid.scrollHeight,grid.clientHeight,cards[0].dataset.mvIndex,cards.at(-1).dataset.mvIndex]);
      }
      assert(ids.size===129,'all 129 distinct MV records reachable '+JSON.stringify({w,count:ids.size,trace}));
      await capture('mv_continuous_'+w,{viewportSteps:trace});
    }
    // Full-size and narrow grids use four and two columns, respectively.
    for(const [w,h,n] of [[1120,760,4],[760,540,2]]){
      await emit('resize',{width:w,height:h});await wait(()=>innerWidth===w,'keyboard grid size');await select('voice');await select('mv');
      const grid=one('.download-mv-scroll'), by=i=>one('[data-mv-index="'+i+'"]'), cover=i=>by(i)?.querySelector('.download-mv-cover');
      await wait(()=>one('.download-mv-row').children.length===n,'columns settled');
      cover(0).focus({preventScroll:true});key(cover(0),'ArrowDown');await wait(()=>document.activeElement===cover(n),'vertical card navigation');key(cover(n),'ArrowRight');await wait(()=>document.activeElement===cover(n+1),'horizontal card navigation');
      const current=cover(n+1);current.dispatchEvent(new KeyboardEvent('keydown',{key:'F10',shiftKey:true,bubbles:true,cancelable:true}));await wait(()=>one('.action-menu')?.style.visibility==='visible','keyboard MV actions');
      await capture('mv_menu_'+w);key(one('.action-menu'),'Escape');await wait(()=>!one('.action-menu')&&document.activeElement===current,'MV menu returns exact origin');
      key(current,'End');await wait(()=>document.activeElement?.closest('[data-mv-index="99"]'),'end of first window');await wait(()=>!one('.download-continuation'),'MV next window');key(document.activeElement,'End');await wait(()=>document.activeElement?.closest('[data-mv-index="128"]'),'last loaded MV');
      const manage=by(128).querySelector('.download-mv-manage');click(manage);await wait(()=>one('.action-menu'),'more button');click(button('管理下载文件…',one('.action-menu')));await wait(()=>one('.download-dialog[open]'),'MV file controls');await capture('mv_manage_'+w);await closeDialog();await wait(()=>document.activeElement===manage,'MV management returns exact origin');
      grid.scrollTop=0;grid.dispatchEvent(new Event('scroll'));await wait(()=>!by(128)&&document.activeElement===grid,'recycled action yields to grid');key(grid,'Home');await wait(()=>document.activeElement===cover(0),'keyboard recovery');await capture('mv_keyboard_'+w);
      cover(0).dispatchEvent(new KeyboardEvent('keydown',{key:'F10',shiftKey:true,bubbles:true,cancelable:true}));await wait(()=>one('.action-menu')?.contains(document.activeElement),'scroll starts with focus in menu');grid.scrollTop=grid.scrollHeight;grid.dispatchEvent(new Event('scroll'));await wait(()=>!one('.action-menu')&&document.activeElement===grid&&!by(0),'scrolled menu returns focus to grid');await capture('mv_menu_scroll_'+w);key(grid,'Home');await wait(()=>document.activeElement===cover(0),'return from dismissed menu');
      const firstManage=by(0).querySelector('.download-mv-manage');click(firstManage);await wait(()=>one('.action-menu'),'reopen first');click(button('管理下载文件…',one('.action-menu')));await wait(()=>one('.download-dialog[open]'),'second file dialog');
      grid.scrollTop=grid.scrollHeight;grid.dispatchEvent(new Event('scroll'));await wait(()=>!by(0),'dialog origin recycled');await closeDialog();
      await wait(()=>document.activeElement?.matches('.download-mv-cover[tabindex="0"]'),'removed dialog origin returns to available card');await capture('mv_recycled_dialog_'+w);
    }
    await fixture('restore');await select('voice');await select('mv');
    const credit=button('Artist 17',one('.download-mv-scroll'));assert(credit&&!button('无标识艺人',one('.download-mv-scroll')),'only identified artist is interactive');click(credit);await wait(()=>one('.catalog-page[aria-label="歌手详情"] h1')?.textContent.includes('17'),'artist detail for this credit');assert(!one('.mv-detail:not(.mv-mini)'),'artist click does not open MV');await capture('mv_artist');click(button('返回上一页'));await wait(()=>one('.download-mv-card')&&ready(),'artist back to MV downloads');
    const movie=all('.download-mv-cover').find(el=>el.getAttribute('aria-label').includes('合成MV 133'));assert(movie,'real MP4 fixture card');movie.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,cancelable:true,button:2,clientX:150,clientY:180}));await wait(()=>one('.action-menu'),'pointer context menu');click(button('播放 MV',one('.action-menu')));
    await wait(()=>one('.mv-info h1')?.textContent.includes('合成MV 133')&&one('.mv-play-facts')?.textContent.includes('本地文件')&&Number(one('.video-canvas')?.dataset.renderedFrames)>0&&button('重新读取MV资料'),'offline MP4 playback independent of detail');
    const canvas=one('.video-canvas');let nonBlack=0;
    await wait(()=>{const px=canvas.getContext('2d').getImageData(0,0,canvas.width,canvas.height).data;nonBlack=0;for(let i=0;i<px.length;i+=4)if(px[i]+px[i+1]+px[i+2]>20)nonBlack++;return nonBlack>=100;},'nonblank decoded video frame');
    await fixture('verify_local_mv');const videoBefore=await client.videoSnapshot((await client.accountSnapshot()).epoch);click(button('重新读取MV资料'));await wait(()=>button('重新读取MV资料')&&!button('重新读取MV资料').disabled,'detail retry');const videoAfter=await client.videoSnapshot(videoBefore.epoch);assert(videoAfter.playId===videoBefore.playId,'detail retry does not restart local video');await capture('mv_offline_playback',{nonBlackPixels:nonBlack,frames:Number(canvas.dataset.renderedFrames),local:videoAfter.local,id:videoAfter.id,retryKeptPlayId:videoAfter.playId===videoBefore.playId});
    await emit('complete',{ok:true,downloadPageUi:true,stageCount:count,realMusicRequests:0,realAccountWrites:0,syntheticNativeRecords:146,physicalPointerVerified:false});
  }catch(error){const failedAt=stage;await capture('failure_state').catch(()=>{});await emit('failed',{stageCount:count,step:failedAt,message:String(error),stack:error.stack});}
})();
