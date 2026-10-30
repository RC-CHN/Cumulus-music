// Real WebView scroll/layout and production IPC, with a synthetic paginated backend.
(async () => {
  const one=s=>document.querySelector(s), all=s=>[...document.querySelectorAll(s)], sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const assert=(v,m)=>{if(!v)throw Error(m);};
  const wait=async(f,m)=>{for(let i=0;i<220;i++){if(f())return;await sleep(60);}throw Error(m);};
  const emit=(stage,data={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...data}});
  const button=name=>all('button').find(n=>n.getAttribute('aria-label')===name||n.textContent.trim()===name);
  const click=n=>{assert(n&&!n.disabled,'missing control');n.focus({preventScroll:true});n.click();};
  const pane=()=>one('.search-result-panel'), grid=()=>one('.search-result-grid');
  const loaded=()=>Number(grid()?.dataset.loadedCount??0);
  const ready=()=>pane()?.getAttribute('aria-busy')==='false'&&!one('.search-results-leave-active,.search-results-enter-active');
  let count=0, step='start', serial=0, calls=[], held=false, holdQuery, release;
  const original=window.fetch;
  window.fetch=async function(url,options){
    const q=String(url).endsWith('/search_results')?JSON.parse(options.body).query:null;
    if(q)calls.push(q);
    const gate=q&&holdQuery?.(q)?new Promise(r=>{const timer=setTimeout(r,12000);release=()=>{clearTimeout(timer);holdQuery=undefined;r();};}):null;
    const response=await original.call(window,url,options);
    if(gate){held=true;await gate;}
    return response;
  };
  async function fixture(mode){window.__searchFixtureAck=false;await emit('search-fixture',{mode});await wait(()=>window.__searchFixtureAck,'fixture');}
  async function submit(){const input=one('[aria-label="搜索音乐"]');input.value='连续浏览 '+(++serial);input.dispatchEvent(new Event('input',{bubbles:true}));input.form.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));await wait(()=>ready()&&one('[data-category="all"]')?.getAttribute('aria-selected')==='true','new search');await sleep(200);}
  async function choose(category){click(one(`[data-category="${category}"]`));await sleep(0);await wait(()=>ready()&&one(`[data-category="${category}"]`)?.getAttribute('aria-selected')==='true','category '+category);await sleep(200);}
  async function capture(label){step=label;await sleep(200);assert(document.documentElement.scrollWidth<=innerWidth&&pane().scrollWidth<=pane().clientWidth+1,'no horizontal overflow');assert(all('.search-result-card').length<80,'bounded card DOM');await emit(`ux_${String(++count).padStart(2,'0')}_${label}`,{width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,loaded:loaded(),rendered:all('.search-result-card').length,scrollTop:pane().scrollTop,requestCount:calls.length});await sleep(420);}
  async function append(){const before=loaded();pane().scrollTop=pane().scrollHeight;await wait(()=>loaded()>before,'append page');await sleep(180);}
  try {
    await wait(()=>one('.track-row'),'daily ready');await fixture('scroll');
    for(const [width,height,theme] of [[1120,760,'light'],[1120,760,'dark'],[760,540,'light'],[760,540,'dark']]) {
      await emit('resize',{width,height});await wait(()=>innerWidth===width&&innerHeight===height,'resize');click(button('外观主题'));await wait(()=>one('.action-menu'),'theme menu');click(button(theme==='dark'?'深色':'浅色'));await wait(()=>!one('.action-menu'),'theme applied');await submit();await choose('artists');
      assert(!one('.search-result-pagination'),'waterfall replaces page buttons');
      const initial=loaded();held=false;holdQuery=q=>q.category==='artists'&&q.offset===initial;
      pane().scrollTop=pane().scrollHeight;await wait(()=>held&&one('.search-load-more')?.getAttribute('aria-busy')==='true','append loading');const top=pane().scrollTop;await capture(`loading_${width}_${theme}`);
      release();await wait(()=>loaded()>initial,'append finished');assert(Math.abs(pane().scrollTop-top)<2,'append keeps viewport');await capture(`append_${width}_${theme}`);
      while(loaded()<104)await append();
      assert(loaded()>=104&&all('.search-result-card').length<50,'long grid stays virtual');await capture(`deep_${width}_${theme}`);
      const saved=pane().scrollTop, total=loaded(), requests=calls.length;await choose('users');await choose('artists');assert(loaded()===total&&Math.abs(pane().scrollTop-saved)<2,'category scroll restored');assert(calls.length===requests+1,'cached category does not re-fetch');await capture(`category_return_${width}_${theme}`);
      const rect=pane().getBoundingClientRect();const card=all('.search-result-card').find(n=>{const r=n.getBoundingClientRect();return r.top+r.height/2>=rect.top&&r.top+r.height/2<=rect.bottom;});assert(card,'visible detail card');const identity=card.dataset.resultKey;click(card.querySelector('.search-card-open'));await wait(()=>!one('.search-results-page'),'detail entered');click(button('返回上一页'));await wait(()=>ready()&&grid(),'return results');await sleep(350);assert(loaded()===total&&Math.abs(pane().scrollTop-saved)<2,'detail return scroll');assert(document.activeElement.closest('[data-result-key]')?.dataset.resultKey===identity,'detail return focus');await capture(`detail_return_${width}_${theme}`);
      const last=all('.search-card-open').find(n=>n.getBoundingClientRect().top>=pane().getBoundingClientRect().top);last.focus({preventScroll:true});last.dispatchEvent(new KeyboardEvent('keydown',{key:'End',bubbles:true,cancelable:true}));await sleep(300);assert(document.activeElement.closest('[data-grid-index]')?.dataset.gridIndex===String(total-1),'keyboard reaches virtualized item');await capture(`keyboard_end_${width}_${theme}`);
      await choose('songs');const inner=one('.track-scroll');inner.scrollTop=560;await sleep(150);const songTop=inner.scrollTop;await choose('playlists');await choose('songs');assert(Math.abs(one('.track-scroll').scrollTop-songTop)<2,'song viewport restored');await capture(`song_return_${width}_${theme}`);
    }
    await fixture('scroll_error');await submit();await choose('artists');pane().scrollTop=pane().scrollHeight;await wait(()=>button('重试加载更多'),'append error');assert(loaded()===13,'failed append preserves records');await capture('append_error');await choose('users');await choose('artists');assert(button('重试加载更多')&&loaded()===13,'return preserves explicit retry');await capture('error_return');const requests=calls.length;await sleep(600);assert(calls.length===requests,'no automatic error loop');await fixture('scroll');const retry=button('重试加载更多');click(retry);await wait(()=>loaded()>13,'append retry');assert(document.activeElement===retry&&retry.isConnected,'retry preserves button focus');await capture('append_retry');
    await fixture('scroll_duplicates');await submit();await choose('users');held=false;holdQuery=q=>q.category==='users'&&q.offset===13;pane().scrollTop=pane().scrollHeight;await wait(()=>held&&button('正在加载更多…'),'duplicate loading');click(button('正在加载更多…'));release();await wait(()=>pane().textContent.includes('没有更多可显示'),'duplicate stop');assert(document.activeElement===one('.search-load-more'),'exhausted list retains footer focus');const footer=one('.search-load-more'),box=footer.getBoundingClientRect(),area=pane().getBoundingClientRect();assert(box.top>=area.top-1&&box.bottom<=area.bottom+1,'focused footer remains inside scroll viewport');const style=getComputedStyle(footer);assert(style.outlineWidth==='2px'&&style.outlineOffset==='-2px'&&style.outlineColor===getComputedStyle(one('.search-result-tabs [aria-selected="true"]'),'::after').backgroundColor,'footer uses inset theme focus ring');assert(loaded()===13&&!button('加载更多'),'duplicate records stop automatic requests');await capture('duplicate_stop');
    await fixture('scroll');await submit();held=false;holdQuery=q=>q.category==='mvs'&&q.offset===0;click(one('[data-category="mvs"]'));await wait(()=>held,'hold old category');await choose('artists');release();await sleep(250);assert(one('[data-category="artists"]').getAttribute('aria-selected')==='true'&&grid().classList.contains('search-person-grid'),'late category ignored');await capture('late_category');
    await choose('mvs');await append();await capture('mv_waterfall');
    await choose('artists');while(loaded()<104)await append();
    const visibleIndex=()=>Number(all('[data-grid-index]').find(n=>n.getBoundingClientRect().bottom>pane().getBoundingClientRect().top)?.dataset.gridIndex);
    const anchor=visibleIndex();await capture('before_resize');await emit('resize',{width:1120,height:760});await wait(()=>innerWidth===1120,'live resize');await sleep(350);assert(Math.abs(visibleIndex()-anchor)<=4,'resize preserves visible item');await capture('after_resize');
    await emit('complete',{ok:true,stageCount:count,scrollRestored:true,virtualKeyboard:true,responseTimingInjected:true,realMusicRequests:0,realAccountWrites:0});
  }catch(e){await emit('failed',{step,message:String(e),stack:e?.stack,stageCount:count,requests:calls.slice(-15),loaded:loaded(),scrollTop:pane()?.scrollTop,active:document.activeElement?.outerHTML,body:document.body.innerText.slice(0,2500)});}
})();
