// Isolated synthetic lyrics through the production search, row and clipboard IPC.
(async () => {
  const one=s=>document.querySelector(s), all=s=>[...document.querySelectorAll(s)], sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const assert=(v,m)=>{if(!v)throw Error(m);};
  const wait=async(f,m)=>{for(let i=0;i<200;i++){if(f())return;await sleep(60);}throw Error(m);};
  const nativeInvoke=window.__TAURI_INTERNALS__.invoke;
  const invoke=(c,a={})=>nativeInvoke(c,a);
  const emit=(stage,data={})=>invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...data}});
  let failCopy=false, holdCopy=false, copies=0, playCalls=0;
  const fetch=window.fetch;
  window.fetch=async function(url,options){
    const path=String(url);
    if(path.endsWith('/copy_text')) {
      copies++; if(holdCopy)await sleep(650);
      if(failCopy)return new Response(JSON.stringify('synthetic clipboard refusal'),{headers:{'Content-Type':'application/json','Tauri-Response':'error'}});
    }
    if(path.endsWith('/play_selection')||path.endsWith('/player_action'))playCalls++;
    return fetch.call(window,url,options);
  };
  const button=(name,root=document)=>[...root.querySelectorAll('button')].find(n=>n.getAttribute('aria-label')===name||n.textContent.trim()===name);
  const click=n=>{assert(n&&!n.disabled,'control unavailable');n.focus({preventScroll:true});n.click();};
  const pane=()=>one('.search-result-panel');
  const ready=()=>pane()?.getAttribute('aria-busy')==='false'&&!one('.search-results-enter-active,.search-results-leave-active,.page-enter-active,.page-leave-active');
  const rows=()=>all('.search-lyric-result'), extra=()=>rows()[0].querySelector('.search-lyric-extra');
  const toggle=()=>extra().querySelector('[data-result-action="expand"]'), copy=()=>extra().querySelector('[data-result-action="copy"]');
  const expected='第一行合成歌词\n在夜色中听见你的声音🎵\n每段旋律都是此刻的回响\n第四行 · 展开后的文字\n\n第六行 · 空行之后\n<img src=x onerror=alert(1)>\n第八行 · 合成歌词结束';
  let count=0,step='startup';
  async function capture(label){step=label;await sleep(180);assert(document.documentElement.scrollWidth<=innerWidth&&pane().scrollWidth<=pane().clientWidth+1,'horizontal containment');await emit(`ux_${String(++count).padStart(2,'0')}_${label}`,{width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,renderedLyrics:rows().length});await sleep(420);}
  async function choose(key){click(one(`[data-category="${key}"]`));await sleep(0);await wait(()=>ready()&&one(`[data-category="${key}"]`)?.getAttribute('aria-selected')==='true','category '+key);await sleep(180);}
  async function verifyClipboard(text){window.__searchClipboardAck=null;await emit('search-clipboard-check',{expected:text});await wait(()=>window.__searchClipboardAck!==null,'clipboard read');assert(window.__searchClipboardAck,'native clipboard content matches complete synthetic lyrics');}
  try {
    await wait(()=>one('.track-row'),'daily ready');window.__searchFixtureAck=false;await emit('search-fixture',{mode:'lyrics'});await wait(()=>window.__searchFixtureAck,'fixture');
    for(const [w,h,t] of [[1120,760,'light'],[1120,760,'dark'],[760,540,'light'],[760,540,'dark']]){
      await emit('resize',{width:w,height:h});await wait(()=>innerWidth===w&&innerHeight===h,'resize');click(button('外观主题'));await wait(()=>one('.action-menu'),'theme');click(button(t==='dark'?'深色':'浅色'));await wait(()=>!one('.action-menu'),'theme applied');
      const input=one('[aria-label="搜索音乐"]');input.value=`合成歌词${w}${t}`;input.dispatchEvent(new Event('input',{bubbles:true}));input.form.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));await sleep(0);await wait(()=>ready(),'search ready');await choose('lyrics');
      assert(rows().length===30,'bounded page');assert(extra().querySelector('.search-lyric-lines').textContent===expected.split('\n').slice(0,3).join('\n'),'three-line preview');assert(!one('.search-lyric-lines img'),'markup rendered as text');
      const style=getComputedStyle(extra().querySelector('.search-lyric-text'));assert(style.fontSize==='12px'&&style.lineHeight==='18px','official lyric typography');
      const controls=extra().querySelector('.search-lyric-buttons'),text=extra().querySelector('.search-lyric-text'),r=controls.getBoundingClientRect(),q=text.getBoundingClientRect();assert(w>1000?r.left>=q.right:r.bottom<=q.top,'responsive action placement');await capture(`collapsed_${w}_${t}`);
      const before=playCalls;toggle().dispatchEvent(new MouseEvent('dblclick',{bubbles:true}));copy().dispatchEvent(new MouseEvent('dblclick',{bubbles:true}));assert(playCalls===before,'lyric action double click never plays');
      click(toggle());await wait(()=>toggle().getAttribute('aria-expanded')==='true','expanded');assert(extra().querySelector('.search-lyric-lines').textContent===expected,'all returned lines visible');assert(document.activeElement===toggle(),'toggle keeps focus');const tr=toggle().getBoundingClientRect(),pr=pane().getBoundingClientRect();assert(tr.top>=pr.top&&tr.bottom<=pr.bottom,'expanded toggle remains fully visible');await capture(`expanded_${w}_${t}`);
      copy().scrollIntoView({block:'nearest'});click(copy());await wait(()=>extra().querySelector('[role="status"]').textContent==='复制成功','copy feedback');await verifyClipboard(expected);await capture(`copied_${w}_${t}`);
      const saved=pane().scrollTop;await choose('songs');await choose('lyrics');assert(toggle().getAttribute('aria-expanded')==='true','return retains expanded rows');assert(Math.abs(pane().scrollTop-saved)<2,'return restores expanded page position');await capture(`returned_${w}_${t}`);
      click(toggle());await wait(()=>toggle().getAttribute('aria-expanded')==='false','collapse');await capture(`collapsed_again_${w}_${t}`);
      const more=rows()[0].querySelector('.track-more');more.scrollIntoView({block:'nearest'});click(more);await wait(()=>one('.playlist-dialog[open]'),'operations');await capture(`menu_${w}_${t}`);click(button('关闭歌单操作'));await wait(()=>!one('.playlist-dialog'),'menu closed');assert(document.activeElement===more,'exact menu return focus');
      const short=rows()[1];short.scrollIntoView({block:'center'});assert(short.querySelector('[data-result-action="expand"]').disabled,'short text has no invalid expand action');await capture(`short_${w}_${t}`);
      const empty=rows()[2];empty.scrollIntoView({block:'center'});assert(empty.querySelector('[data-result-action="copy"]').disabled&&empty.textContent.includes('暂无匹配歌词'),'empty lyric feedback');await capture(`missing_${w}_${t}`);
      rows()[3].scrollIntoView({block:'start'});await capture(`long_line_${w}_${t}`);
    }
    pane().scrollTop=0;await sleep(200);holdCopy=true;failCopy=true;const before=copies;click(copy());await wait(()=>copy().getAttribute('aria-disabled')==='true','busy copy');click(copy());assert(copies===before+1,'copy repeat suppressed');await capture('copy_pending');await wait(()=>extra().querySelector('[role="status"]').textContent.includes('失败'),'copy error visible');await capture('copy_failed');failCopy=false;holdCopy=false;click(copy());await wait(()=>extra().querySelector('[role="status"]').textContent==='复制成功','copy recovery');await verifyClipboard(expected);await capture('copy_recovered');
    const truncated=rows()[4];truncated.scrollIntoView({block:'start'});assert(truncated.textContent.includes('仅显示部分内容'),'truncation visible');click(truncated.querySelector('[data-result-action="copy"]'));await wait(()=>truncated.textContent.includes('已复制显示部分'),'partial copy clearly labeled');await verifyClipboard(Array.from({length:512},(_,i)=>`合成歌词 ${i}`).join('\n'));await capture('truncated');
    pane().scrollTop=0;click(rows()[0].querySelector('.track-play'));await wait(()=>rows()[0].querySelector('.track-row.current'),'playing feedback');await capture('playing');document.documentElement.dataset.motion='reduced';click(toggle());await sleep(0);assert(extra().querySelector('.search-lyric-lines').getAnimations().length===0,'reduced motion disables reveal');await capture('reduced_motion');const epoch=(await invoke('account_snapshot')).epoch;assert((await invoke('player_queue',{epoch})).tracks.length===30,'lyric search plays whole returned page');
    await emit('complete',{ok:true,stageCount:count,lyricsLayout:true,clipboardVerified:true,clipboardFailureInjected:true,realMusicRequests:0,realAccountWrites:0});
  }catch(e){await emit('failed',{step,stageCount:count,message:String(e),stack:e?.stack,body:document.body.innerText.slice(0,2600)});}
})();
