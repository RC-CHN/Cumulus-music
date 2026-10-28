(async()=>{
  const sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const all=s=>[...document.querySelectorAll(s)],one=s=>document.querySelector(s);
  const assert=(ok,message)=>{if(!ok)throw Error(message);};
  const emit=(stage,rest={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...rest}});
  const wait=async(test,label)=>{for(let i=0;i<150;i++){if(test())return;await sleep(100);}throw Error(label);};
  const button=text=>all('button').find(n=>n.textContent.trim()===text);
  const click=node=>{assert(node&&!node.disabled,`unavailable ${node?.outerHTML}`);node.scrollIntoView({block:'nearest'});node.focus({preventScroll:true});node.click();};
  const input=(node,value)=>{node.value=value;node.dispatchEvent(new Event('input',{bubbles:true}));};
  const ready=()=>one('.history-page')?.getAttribute('aria-busy')==='false';
  let stageCount=0;
  async function capture(name){
    await sleep(400);
    assert(document.documentElement.scrollWidth<=innerWidth,'horizontal overflow');
    const page=one('.history-page').getBoundingClientRect(),main=one('.main-content').getBoundingClientRect();
    const note=one('.history-note').getBoundingClientRect();
    assert(page.bottom<=main.bottom+1,'history page clipped');assert(note.bottom<=main.bottom+1,'history footer clipped');
    const rows=all('.history-window .history-row');assert(rows.length<=35,'unbounded history rows');
    for(const row of rows){for(const action of row.querySelectorAll('.history-actions button')){
      const b=action.getBoundingClientRect(),cell=action.parentElement.getBoundingClientRect();
      assert(b.left>=cell.left-1&&b.right<=cell.right+1,'action outside cell');
    }}
    await emit(`ux_${String(++stageCount).padStart(2,'0')}_${name}`,{width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,rows:rows.length,total:one('.history-table')?.getAttribute('aria-rowcount'),pageBottom:page.bottom,mainBottom:main.bottom,footerBottom:note.bottom,scope:one('[aria-label="播放记录类型"] [aria-current]')?.textContent.trim(),dialog:!!one('dialog[open]')});await sleep(900);
  }
  async function theme(value){click(one('[aria-label="外观主题"]'));await wait(()=>button(value),'theme menu');click(button(value));await wait(()=>document.documentElement.dataset.theme===(value==='深色'?'dark':'light'),'theme changed');}
  async function tab(label){click(all('[aria-label="播放记录类型"] button').find(n=>n.textContent.trim()===label));await wait(()=>ready()&&one('[aria-label="播放记录类型"] [aria-current]')?.textContent.trim()===label,'history tab '+label);await sleep(250);}
  async function removeFirst(){click(one('.history-actions [aria-label^="删除"]'));await wait(()=>one('dialog[open]'),'delete confirmation');click(button('确认'));await wait(()=>!one('dialog[open]')&&ready(),'delete settled');}
  try{
    await wait(()=>button('最近播放')&&one('.track-row'),'app initialized');await theme('浅色');
    click(button('最近播放'));await wait(()=>ready()&&one('.history-row[aria-rowindex]'),'history loaded');await capture('songs_light');
    const scroller=one('.history-scroll');scroller.scrollTop=scroller.scrollHeight;await sleep(350);
    assert(one('.history-row[aria-rowindex="301"]'),'last virtual row');await capture('songs_scrolled');scroller.scrollTop=0;await sleep(350);
    input(one('[aria-label="筛选播放记录"]'),'台湾 a');await wait(()=>one('.history-table')?.getAttribute('aria-rowcount')==='2','normalized filtering');await capture('filtered');
    input(one('[aria-label="筛选播放记录"]'),'no-match');await wait(()=>one('.history-content .empty-state'),'no result');await capture('filter_empty');click(one('[aria-label="清空历史筛选"]'));await wait(()=>one('.history-row[aria-rowindex]'),'clear filter');
    const remove=one('.history-actions [aria-label^="删除"]');click(remove);await wait(()=>one('dialog[open]'),'confirmation');await capture('delete_cancel');click(button('取消'));await wait(()=>!one('dialog[open]'),'modal closed');assert(document.activeElement===remove,'focus restored after modal');
    for(const name of ['播客','有声书','声音','歌单','专辑']){await tab(name);await capture('type_'+({播客:'podcast',有声书:'book',声音:'voice',歌单:'playlist',专辑:'album'}[name]));}
    await theme('深色');await capture('album_dark');await tab('歌曲');await capture('songs_dark');
    // Every write below targets the synthetic in-memory backend, never a profile.
    await emit('fixture',{mode:'lost'});await removeFirst();await wait(()=>one('.history-recovery'),'unresolved outcome');await capture('lost_reply');
    await emit('resize',{width:760,height:540});await wait(()=>innerWidth===760,'small pending');await capture('lost_small');
    await emit('fixture',{mode:'offline'});click(button('核对多端记录'));await wait(()=>ready()&&one('.catalog-error'),'pending read error');await capture('lost_offline_small');
    await emit('resize',{width:1120,height:760});await wait(()=>innerWidth===1120,'restore size');
    assert(button('清空记录').disabled,'pending mutation blocks new deletion');
    await emit('fixture',{mode:'normal'});click(button('核对多端记录'));await wait(()=>button('保留当前状态')&&!button('保留当前状态').disabled,'readback');await capture('reconciled');
    click(button('保留当前状态'));await wait(()=>one('dialog[open]'),'accept confirmation');await capture('accept_confirmation');click(button('确认'));await wait(()=>!one('.history-recovery')&&!one('dialog[open]'),'accepted state');
    await emit('fixture',{mode:'rejected'});await removeFirst();await wait(()=>one('.history-outcome')?.textContent.includes('未完成'),'definite rejection');await capture('rejected');
    await emit('fixture',{mode:'offline'});click(one('[aria-label="刷新播放记录"]'));await wait(()=>ready()&&one('.catalog-error'),'cached error');await capture('offline_cached');
    await emit('fixture',{mode:'normal'});click(one('[aria-label="刷新播放记录"]'));await wait(()=>ready()&&!one('.catalog-error'),'read recovered');
    await emit('resize',{width:760,height:540});await wait(()=>innerWidth===760,'small window');await capture('small_dark');
    const nav=one('.sidebar-scroll');for(const item of all('.sidebar .nav-item')){item.scrollIntoView({block:'nearest'});await sleep(15);const a=item.getBoundingClientRect(),b=nav.getBoundingClientRect();assert(a.top>=b.top-1&&a.bottom<=b.bottom+1,'sidebar item unreachable at minimum height');}nav.scrollTop=0;await capture('small_navigation');
    await theme('浅色');await capture('small_light');await tab('有声书');await capture('book_small');
    await emit('fixture',{mode:'empty'});click(one('[aria-label="刷新播放记录"]'));await wait(()=>ready()&&one('.history-content .empty-state'),'empty history');await capture('empty');
    await emit('complete',{ok:true,stageCount,realMusicRequests:0,playbackVerified:false});
  }catch(error){await emit('failed',{ok:false,error:String(error),stageCount});}
})();
