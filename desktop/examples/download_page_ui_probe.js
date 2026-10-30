// Production download pages over synthetic native records, never remote media.
(async () => {
  const one=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)];
  const sleep=ms=>new Promise(r=>setTimeout(r,ms)),assert=(ok,m)=>{if(!ok)throw Error(m);};
  const wait=async(fn,m)=>{await sleep(25);for(let i=0;i<240;i++){if(fn())return;await sleep(50);}throw Error(m);};
  const emit=(stage,data={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...data}});
  const button=(label,root=document)=>[...root.querySelectorAll('button')].find(n=>n.getAttribute('aria-label')===label||n.textContent.trim()===label);
  const click=n=>{assert(n&&!n.disabled,`enabled ${n?.outerHTML}`);n.focus({preventScroll:true});n.click();};
  const key=(n,k)=>n.dispatchEvent(new KeyboardEvent('keydown',{key:k,bubbles:true,cancelable:true}));
  const input=(n,text)=>{n.value=text;n.dispatchEvent(new Event('input',{bubbles:true}));};
  const rect=n=>{const r=n.getBoundingClientRect();return {x:r.x,y:r.y,width:r.width,height:r.height,right:r.right,bottom:r.bottom};};
  const near=(a,b,m)=>assert(Math.abs(a-b)<1.2,`${m}: ${a}/${b}`);
  const fixture=async mode=>{window.__downloadPageAck=null;await emit('download-page-fixture',{mode});await wait(()=>window.__downloadPageAck,'fixture response');assert(window.__downloadPageAck.ok,window.__downloadPageAck.message);};
  const tab=id=>one(`#download-tab-${id}`),ready=()=>!one('#download-content[aria-busy="true"],.download-section-enter-active,.download-section-leave-active');
  const first=()=>one('[data-download-index="0"]');
  let count=0,stage='startup';
  async function select(id){click(tab(id));await wait(()=>tab(id).getAttribute('aria-selected')==='true'&&ready()&& (id==='mv'?one('.download-mv-card'):first()),`tab ${id}`);await sleep(100);}
  async function capture(label){stage=label;await sleep(250);assert(document.documentElement.scrollWidth<=innerWidth,'document overflow');
    for(const n of all('.download-navigation,.download-toolbar,.download-table-header')) {const r=rect(n);assert(r.x>=0&&r.right<=innerWidth+1,'page bounds');}
    assert(all('[data-download-index]').length<=32,'bounded table');assert(all('.download-mv-card').length<=32,'bounded grid');
    if(first()&&!one('dialog')) {
      near(rect(first()).height,64,'default row height');near(parseFloat(getComputedStyle(one('.download-track-name')).fontSize),16,'title size');
      const cells=[...first().children];all('.download-table-header>[role="columnheader"]').forEach((n,i)=>{near(rect(n).x,rect(cells[i]).x,'column alignment');near(rect(n).width,rect(cells[i]).width,'column widths');});
      const scroller=one('.download-scroll');assert(scroller.scrollWidth<=scroller.clientWidth+1,'table overflow');
    }
    if(!one('dialog')) for(const n of all('.download-tabs button,.download-navigation-actions button,.download-toolbar>button')) {const r=rect(n);const hit=document.elementFromPoint(r.x+r.width/2,r.y+r.height/2);assert(n.contains(hit),'header action hit');}
    await emit(`ux_${String(++count).padStart(2,'0')}_${label}`,{width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,section:one('.download-tabs [aria-selected="true"]')?.textContent,rows:all('[data-download-index]').length,cards:all('.download-mv-card').length,focused:document.activeElement?.outerHTML.slice(0,200)});await sleep(400);
  }
  async function closeDialog(){click(button('关闭下载对话框'));await wait(()=>!one('dialog'),'dialog closes');}
  try {
    await wait(()=>one('.track-row'),'initial page');await fixture('seed');click(button('下载管理'));await wait(()=>first()&&ready(),'download records');
    for(const [w,h,theme] of [[1120,760,'light'],[1120,760,'dark'],[760,540,'light'],[760,540,'dark']]) {
      await emit('resize',{width:w,height:h});await wait(()=>innerWidth===w&&innerHeight===h,'window resize');click(button('外观主题'));await wait(()=>one('.action-menu'),'theme menu');click(button(theme==='dark'?'深色':'浅色'));await wait(()=>!one('.action-menu'),'theme applied');
      const suffix=`${w}_${theme}`;
      await select('song');assert(all('.download-table-header>[role="columnheader"]').length===5,'song columns');assert(one('.download-pagination').textContent.includes('120'),'songs include cloud before pagination');await capture(`songs_${suffix}`);
      first().focus({preventScroll:true});const manage=first().querySelector('[aria-label^="管理下载"]');click(manage);await wait(()=>one('.download-dialog[open]'),'file dialog');await capture(`file_${suffix}`);await closeDialog();assert(document.activeElement===manage,'file menu return focus');
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
    await select('song');click(button('下一页'));await wait(()=>ready()&&one('.download-pagination')?.textContent.includes('2 / 2'),'page two');const pageScroll=one('.download-scroll');const pageIds=new Set(all('[data-download-index]').map(n=>n.dataset.downloadTask));assert(first().textContent.includes('合成单曲 101'),'second page starts after source filtering');pageScroll.scrollTop=pageScroll.scrollHeight;pageScroll.dispatchEvent(new Event('scroll'));await sleep(150);all('[data-download-index]').forEach(n=>pageIds.add(n.dataset.downloadTask));assert(pageIds.size===20&&one('.download-table').getAttribute('aria-rowcount')==='121','all second-page records reachable');pageScroll.scrollTop=0;pageScroll.dispatchEvent(new Event('scroll'));await sleep(150);await capture('second_page');click(button('上一页'));await wait(()=>ready()&&first(),'first page');
    const scroller=one('.download-scroll');first().focus({preventScroll:true});scroller.scrollTop=scroller.scrollHeight;scroller.dispatchEvent(new Event('scroll'));await wait(()=>!one('[data-download-index="0"]'),'virtual recycle');assert(document.activeElement===scroller,'recycled focus body');await capture('virtual_focus');key(scroller,'Home');await wait(()=>document.activeElement===first(),'keyboard home');
    click(button('更多下载操作'));await wait(()=>one('.action-menu')?.style.visibility==='visible','page menu');await capture('more_menu');click(button('批量操作',one('.action-menu')));await wait(()=>button('完成',one('.download-toolbar')),'batch selected');assert(document.activeElement===button('完成',one('.download-toolbar')),'batch entry focus');click(one('[aria-label="选择本页全部下载"]'));await capture('batch_selection');click(button('完成',one('.download-toolbar')));await wait(()=>button('更多下载操作'),'batch closes');assert(document.activeElement===button('更多下载操作'),'batch return focus');
    await fixture('read_error');click(tab('voice'));await wait(()=>one('.error-state')&&ready(),'read failure');assert(!first(),'no stale rows after failed category read');await capture('read_error');await fixture('repair_read');click(button('刷新状态'));await wait(()=>ready()&&first(),'read recovery');await capture('read_recovered');
    await select('active');const before=Number(one('progress').value);await fixture('progress');await wait(()=>Number(one('progress').value)>before+.005,'native progress update');await capture('progress');
    click(button('全部暂停'));await wait(()=>all('.download-progress-status').filter(n=>n.textContent==='已暂停').length===4&&all('.download-progress.failed').length===1,'pause runnable tasks, preserve existing failure');await fixture('verify_pause');await capture('paused_all');
    await fixture('empty');await wait(()=>one('.empty-state')?.textContent.includes('没有正在下载'),'empty tasks');await capture('empty_active');await fixture('restore');await wait(()=>first(),'restore records');
    window.__motionProof.system(true);await wait(()=>document.documentElement.dataset.motion==='reduced','reduced motion');await select('mv');await capture('reduced_mv');
    await emit('complete',{ok:true,downloadPageUi:true,stageCount:count,realMusicRequests:0,realAccountWrites:0,syntheticNativeRecords:146,physicalPointerVerified:false});
  }catch(error){const failedAt=stage;await capture('failure_state').catch(()=>{});await emit('failed',{stageCount:count,step:failedAt,message:String(error),stack:error.stack});}
})();
