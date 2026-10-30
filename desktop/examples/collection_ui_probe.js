(async()=>{
 const sleep=ms=>new Promise(r=>setTimeout(r,ms)),all=s=>[...document.querySelectorAll(s)],one=s=>document.querySelector(s);
 const assert=(ok,message)=>{if(!ok)throw Error(message);};
 const emit=(stage,rest={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...rest}});
 const wait=async(test,label)=>{for(let i=0;i<200;i++){if(test())return;await sleep(100);}throw Error(label);};
 const button=text=>all('button').find(n=>n.textContent.trim()===text);
 const click=n=>{assert(n&&!n.disabled,`unavailable ${n?.outerHTML}`);n.scrollIntoView({block:'nearest'});n.focus({preventScroll:true});n.click();};
 const count=()=>Number(one('.collections-tools>.muted')?.textContent.match(/已加载 (\d+)/)?.[1]);
 const ready=()=>one('.collections-page')?.getAttribute('aria-busy')==='false';
 const mode=async value=>{await emit('fixture',{mode:value});await sleep(150);};
 const filter=async value=>{const n=one('[aria-label="筛选收藏列表"]');n.value=value;n.dispatchEvent(new Event('input',{bubbles:true}));await sleep(100);};
 const refresh=()=>click(one('[aria-label="刷新收藏列表"]'));
 const back=()=>click(one('[aria-label="返回上一页"]'));
 let stageCount=0;
 async function capture(name){await sleep(500);const cards=all('.catalog-card,.collection-follow-card').length;assert(cards<=30,'unbounded collection cards');assert(document.documentElement.scrollWidth<=innerWidth,'horizontal overflow');const p=one('.collections-page');assert(!p||p.getBoundingClientRect().bottom<=one('.main-content').getBoundingClientRect().bottom+1,'clipped collection page');if(p){assert(p.scrollHeight<=p.clientHeight+1,'unexpected outer collection scroll');const more=button('加载更多')||button('正在加载…');if(more){const box=more.getBoundingClientRect();assert(box.top>=p.getBoundingClientRect().top&&box.bottom<=p.getBoundingClientRect().bottom+1,'load more inaccessible');}}await emit(`ux_${String(++stageCount).padStart(2,'0')}_${name}`,{width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,cards,loaded:count()||0});await sleep(1100);}
 try{
  await wait(()=>one('.track-row'),'account ready');click(one('[aria-label="外观主题"]'));await wait(()=>button('浅色'),'theme');click(button('浅色'));
  click(button('专辑与关注'));await wait(()=>ready()&&count()===50,'saved first page');await capture('saved_first');
  await mode('fail_more');click(button('加载更多'));await wait(()=>ready()&&one('.catalog-error'),'more failure');assert(count()===50,'failure removed saved data');await capture('saved_more_error');
  await mode('none');click(button('重试'));await wait(()=>ready()&&count()===100,'more retry');await capture('saved_more_recovered');
  refresh();await wait(()=>ready()&&count()===50,'reset before filter');await filter('台湾 a [live]');await wait(()=>ready()&&count()===100&&one('.collections-tools>.muted')?.textContent.includes('匹配 1'),'normalized search on later page');await capture('saved_search');
  await filter('');click(button('加载更多'));await wait(()=>ready()&&count()===123&&!button('加载更多'),'saved last page');one('.catalog-card-scroll').scrollTop=999999;await capture('saved_scrolled');
  await mode('fail_saved');refresh();await wait(()=>ready()&&one('.catalog-error'),'refresh failure');assert(count()===123,'refresh erased previous pages');await capture('saved_refresh_error');
  await mode('none');click(button('重试'));await wait(()=>ready()&&count()===50&&!one('.catalog-error'),'refresh retry must replace first page');await capture('saved_refreshed');
  click(one('.catalog-card'));await wait(()=>one('.album-header'),'saved album navigation');await capture('saved_album');back();await wait(()=>ready()&&count()===50,'back from album');
  click(button('已购专辑'));await wait(()=>ready()&&count()===2,'purchased albums');await capture('purchased');
  await mode('empty_purchase');refresh();await wait(()=>ready()&&one('.empty-state')?.textContent.includes('还没有已购专辑'),'empty purchases');await capture('purchased_empty');
  await mode('fail_purchase');refresh();await wait(()=>ready()&&one('.catalog-error'),'purchase error');assert(!one('.collections-page .empty-state'),'error shown as empty purchases');await capture('purchased_error');
  await mode('none');click(button('重试'));await wait(()=>ready()&&count()===2,'purchase retry');await capture('purchased_recovered');
  click(button('关注歌手'));await wait(()=>ready()&&count()===20,'followed first page');await capture('followed_first');
  for(let i=0;button('加载更多')&&i<20;i++){const before=count();click(button('加载更多'));await wait(()=>ready()&&count()>before,'followed continuation');}
  assert(count()===205&&!button('加载更多'),'followed list end');one('.collection-follow-scroll').scrollTop=999999;await sleep(350);assert(one('article.collection-follow-card'),'user-only record not mistaken for artist');await capture('followed_scrolled');
  one('.collection-follow-scroll').scrollTop=0;await sleep(350);click(one('button.collection-follow-card'));await wait(()=>one('.artist-header'),'followed artist navigation');await capture('followed_artist');
  back();await wait(()=>ready()&&button('关注歌手')?.getAttribute('aria-current')==='page','back retains followed tab');await capture('followed_back');
  click(one('[aria-label="外观主题"]'));await wait(()=>button('深色'),'theme menu');click(button('深色'));await wait(()=>document.documentElement.dataset.theme==='dark','dark');await capture('followed_dark');
  await emit('resize',{width:760,height:540});await wait(()=>innerWidth===760,'small window');await capture('followed_small');
  await emit('complete',{ok:true,stageCount,realMusicRequests:0,collectionReadOnly:true,playbackVerified:false});
 }catch(error){await emit('failed',{ok:false,error:String(error),stageCount,loaded:count()||0});}
})();
