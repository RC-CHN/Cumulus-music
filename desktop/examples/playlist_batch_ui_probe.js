// Production Vue, Rust IPC, synthetic playlist writes and native null audio.
(async () => {
  const one = (s) => document.querySelector(s), all = (s) => [...document.querySelectorAll(s)];
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const assert = (v, m) => { if (!v) throw Error(m); };
  const wait = async (fn, m) => { for (let i=0;i<240;i++) { if(fn()) return; await sleep(50); } throw Error(m); };
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const emit = (stage, data={}) => invoke('plugin:event|emit', { event:'ux-proof', payload:{stage,...data} });
  const button = (label, root=document) => [...root.querySelectorAll('button')].find(n=>n.getAttribute('aria-label')===label || n.textContent.trim()===label);
  const click = (n) => { assert(n && !n.disabled,'enabled control'); n.scrollIntoView({block:'nearest'}); n.focus({preventScroll:true}); n.click(); };
  const key = (n,k,extra={}) => n.dispatchEvent(new KeyboardEvent('keydown',{key:k,bubbles:true,cancelable:true,...extra}));
  const page = () => one('.playlist-batch-page');
  const row = (i) => one(`[data-batch-index="${i}"]`);
  const label = () => one('.batch-select-label')?.textContent.trim();
  const fixture = async (mode) => { window.__libraryFixtureAck=null; await emit('library-fixture',{mode}); await wait(()=>window.__libraryFixtureAck,'fixture'); return window.__libraryFixtureAck; };
  let step='startup',count=0, originalScroll=0;
  async function capture(name) {
    step=name; await sleep(220);
    assert(document.documentElement.scrollWidth<=innerWidth,'document overflow');
    assert(all('.batch-track-row').length<=32,'bounded batch rendering');
    const overlay = one('dialog[open]');
    const controls = overlay ? [...overlay.querySelectorAll('header button,footer button')] : all('.batch-toolbar button');
    for(const n of controls) {
      const r=n.getBoundingClientRect(), hit=document.elementFromPoint(r.x+r.width/2,r.y+r.height/2);
      assert(r.left>=0 && r.right<=innerWidth+1 && r.top>=0 && r.bottom<=innerHeight+1,'control within window');
      assert(hit && (hit===n || n.contains(hit)),'controls reachable');
      if(!overlay) assert(r.height===40,'official toolbar button height');
    }
    if(page() && !overlay && row(0)) assert(row(0).getBoundingClientRect().height===64,'official row height');
    await emit(`ux_${String(++count).padStart(2,'0')}_${name}`,{width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,selection:label(),rendered:all('.batch-track-row').length,scrollTop:one('.batch-table-scroll')?.scrollTop,scrollHeight:one('.batch-table-scroll')?.scrollHeight,focused:document.activeElement?.outerHTML.slice(0,240)});
    await sleep(420);
  }
  async function enter(expectPage = true) {
    click(button('更多歌单操作')); await wait(()=>one('.action-menu')?.style.visibility==='visible','detail menu');
    click(button('批量操作',one('.action-menu'))); if(expectPage) await wait(()=>page()&&!one('.page-enter-active,.page-leave-active'),'batch page');
  }
  async function ready() { await wait(()=>row(0)&&!one('.batch-status .spinner'),'batch loaded'); }
  async function leave() {
    click(button('完成',page())); await wait(()=>!page()&&!one('.page-leave-active'),'batch left');
    assert(document.activeElement===button('更多歌单操作'),'return to entry focus');
    await sleep(120); // Allow the restored list's native scroll event to settle.
  }
  async function closeDialog() {
    const d=one('dialog[open]');
    assert(key(d,'Escape'),'batch page must not prevent dialog Escape');
    d.dispatchEvent(new Event('cancel',{cancelable:true}));
    await wait(()=>!one('dialog[open]'),'dialog closed');
    assert(page(),'dialog Escape keeps batch page');
  }
  async function choose(i) {
    const scroll=one('.batch-table-scroll'); scroll.scrollTop=Math.max(0,i*64-80); scroll.dispatchEvent(new Event('scroll'));
    await wait(()=>row(i),'virtual row'); click(row(i)); await sleep(60);
  }
  async function clear() { if(!label().startsWith('全选')) { key(one('.playlist-batch-table'),'a',{ctrlKey:true}); await sleep(60); click(one('.batch-select-label')); await sleep(60); } }
  async function pause() { await wait(()=>button('暂停',one('[aria-label="播放器"]'))&&!button('暂停',one('[aria-label="播放器"]')).disabled,'playing'); click(button('暂停',one('[aria-label="播放器"]'))); await wait(()=>button('播放',one('[aria-label="播放器"]')),'paused'); }
  try {
    await wait(()=>one('.track-more'),'daily'); click(button('我的歌单')); await wait(()=>one('[data-library-open="7"]'),'library');
    click(one('[data-library-open="7"]')); await wait(()=>one('.playlist-hero')&&!one('.page-enter-active,.page-leave-active'),'detail');
    for(const [w,h,t] of [[1120,760,'light'],[1120,760,'dark'],[760,540,'light'],[760,540,'dark']]) {
      await emit('resize',{width:w,height:h}); await wait(()=>innerWidth===w&&innerHeight===h,'resize');
      click(button('外观主题')); await wait(()=>one('.action-menu'),'theme'); click(button(t==='dark'?'深色':'浅色')); await wait(()=>!one('.action-menu'),'theme close');
      const normal=one('.content-page-previous .track-scroll'); normal.scrollTop=224; normal.dispatchEvent(new Event('scroll')); originalScroll=normal.scrollTop;
      const suffix=`${w}_${t}`; await enter(); await ready();
      assert(label()==='全选（共240首）','full playlist read');
      assert(all('.batch-buttons button').every(n=>n.disabled),'empty selection disables actions'); await capture(`none_${suffix}`);
      click(row(0)); click(row(2)); await sleep(60); assert(label()==='已选 2 首歌曲','whole row independent toggles'); await capture(`selected_${suffix}`);
      key(row(2),'a',{ctrlKey:true}); await sleep(60); key(row(2),'a',{ctrlKey:true}); await sleep(60);
      assert(label()==='已选 240 首歌曲','Ctrl+A always selects all'); await capture(`all_${suffix}`);
      click(one('.batch-select-label')); await choose(239); assert(label()==='已选 1 首歌曲','last row selection');
      key(row(239),'Home',{ctrlKey:true}); await wait(()=>document.activeElement===row(0),'keyboard returns to first row');
      assert(label()==='已选 1 首歌曲','Ctrl navigation retains selection'); await capture(`last_${suffix}`);
      click(button('下载',page())); await wait(()=>one('dialog[open]'),'download choice'); await capture(`download_${suffix}`); await closeDialog(); assert(document.activeElement===button('下载',page()),'download returns focus');
      click(button('收藏',page())); await wait(()=>one('.collect-list'),'collect choice'); await capture(`collect_${suffix}`); await closeDialog(); assert(document.activeElement===button('收藏',page()),'collect returns focus');
      click(button('删除',page())); await wait(()=>one('.playlist-mutation-dialog')?.getAttribute('aria-busy')==='false','delete ready'); await capture(`delete_${suffix}`); await closeDialog();
      await leave(); assert(one('.content-page-previous .track-scroll').scrollTop===originalScroll,'return preserves detail scroll');
    }
    await fixture('hold_detail'); await enter(); await capture('loading'); await fixture('fail_detail'); await wait(()=>one('.batch-error'),'read failure'); await capture('read_failed');
    await fixture('hold_detail'); click(button('重新读取',page())); await capture('read_retry_wait'); await fixture('success'); await ready(); assert(document.activeElement===button('完成',page()),'read retry focus'); await capture('read_recovered');
    await choose(2); row(5).dispatchEvent(new MouseEvent('click',{bubbles:true,shiftKey:true})); await sleep(60); assert(label()==='已选 4 首歌曲','Shift extension selects inclusive range'); await capture('range_selection');
    const scroll=one('.batch-table-scroll'); row(2).focus({preventScroll:true}); await sleep(120); scroll.scrollTop=scroll.scrollHeight; scroll.dispatchEvent(new Event('scroll'));
    await wait(()=>row(239)&&document.activeElement===scroll,'recycled focus moves to table'); await capture('virtual_focus');
    key(scroll,'Home',{ctrlKey:true}); await wait(()=>document.activeElement===row(0),'virtual focus keyboard recovery'); await clear();
    await choose(2); await choose(0); await choose(239); click(button('播放所选歌曲',page())); await pause(); await capture('selected_playback');
    await clear(); await choose(1); await choose(3); click(button('添加到播放列表',page())); await wait(()=>one('.batch-operation-feedback')?.textContent.includes('已添加'),'queue added'); await capture('selected_queue');
    await fixture('fail_detail'); click(button('添加到播放列表',page())); await wait(()=>one('.batch-operation-feedback.failed'),'queue failure'); await capture('queue_failed');
    await fixture('hold_detail'); click(button('重试',page())); await capture('queue_retry_wait'); await fixture('success'); await wait(()=>one('.batch-operation-feedback')?.textContent.includes('已添加'),'queue retry'); await capture('queue_recovered');
    await fixture('empty_detail'); click(button('添加到播放列表',page())); await wait(()=>one('.batch-operation-feedback.failed'),'changed playlist'); await capture('stale_selection');
    click(button('重新读取',page())); await wait(()=>one('.playlist-batch-page .empty-state'),'changed snapshot reread'); assert(!one('.batch-operation-feedback.failed'),'stale error cleared after read'); await capture('stale_empty');
    await leave(); await fixture('success'); await enter(); await ready(); await choose(1); await choose(3); await capture('stale_recovered');
    click(button('删除',page())); await wait(()=>one('.playlist-mutation-dialog')?.getAttribute('aria-busy')==='false','delete ready');
    await fixture('hold_write'); click(button('删除',one('dialog'))); await wait(()=>one('dialog')?.getAttribute('aria-busy')==='true','delete waiting'); await capture('delete_wait');
    await fixture('success'); await wait(()=>!one('dialog[open]')&&label()==='全选（共238首）','delete reflects two selected rows'); await capture('deleted');
    await choose(0); click(button('删除',page())); await wait(()=>one('dialog')?.getAttribute('aria-busy')==='false','delete ready'); await fixture('uncertain'); click(button('删除',one('dialog')));
    await wait(()=>button('核对结果',one('dialog')),'uncertain delete'); await capture('delete_uncertain');
    const writes=(await fixture('success')).writes; click(button('核对结果',one('dialog'))); await wait(()=>!one('dialog[open]')&&label()==='全选（共237首）','reconcile delete'); assert((await fixture('success')).writes===writes,'reconcile never repeats write'); await capture('delete_recovered');
    await choose(236); click(button('收藏',page())); await wait(()=>one('.collect-list'),'collect list');
    const destination=[...one('.collect-list').querySelectorAll('button')].find(n=>n.textContent.includes('午后散步'));
    await fixture('hold_write'); click(destination); await wait(()=>one('dialog')?.getAttribute('aria-busy')==='true','collect waiting'); await capture('collect_wait');
    await fixture('success'); await wait(()=>!one('dialog[open]'),'collect success'); assert(label()==='已选 1 首歌曲','collection preserves batch selection'); await capture('collected');
    window.__motionProof.system(true); await wait(()=>document.documentElement.dataset.motion==='reduced','reduced motion'); await leave(); await enter(); await ready(); assert(page().getAnimations().every(a=>Number(a.effect.getTiming().duration)<=1),'reduced batch transition'); await capture('reduced'); window.__motionProof.system(false);
    await leave(); await fixture('empty_detail'); await enter(); await wait(()=>one('.playlist-batch-page .empty-state'),'empty batch'); await capture('became_empty'); await leave();
    await fixture('hold_detail'); await enter(); click(button('每日推荐')); await wait(()=>!page()&&!one('.page-leave-active'),'navigate during load'); await fixture('success'); await sleep(250); assert(!page(),'late load cannot reopen'); await capture('left_during_read');
    click(button('我的歌单')); await wait(()=>one('[data-library-open="7"]'),'library'); click(one('[data-library-open="7"]')); await wait(()=>one('.playlist-hero')&&!one('.page-leave-active,.page-enter-active'),'detail');
    await fixture('expired_detail'); await enter(false); await wait(()=>button('重新登录')||button('点击登录'),'expired account'); await capture('expired');
    await emit('complete',{ok:true,playlistBatchUi:true,stageCount:count,realMusicRequests:0,realAccountWrites:0});
  } catch(error) { const last=step; await capture('failure_state').catch(()=>{}); await emit('failed',{step:last,message:String(error),stack:error.stack,stageCount:count,menus:all('.action-menu').map(n=>n.outerHTML),hero:one('.playlist-hero')?.outerHTML.slice(0,6000)}); }
})();
