// Explicit account reads only, using production pages and IPC. No remote writes.
(async()=>{
 const sleep=ms=>new Promise(r=>setTimeout(r,ms)),all=s=>[...document.querySelectorAll(s)],one=s=>document.querySelector(s);
 const assert=(ok,message)=>{if(!ok)throw Error(message);};
 const emit=(stage,rest={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...rest}});
 const wait=async(test,label)=>{for(let i=0;i<450;i++){if(test())return;await sleep(100);}throw Error(label);};
 const button=text=>all('button').find(n=>n.textContent.trim()===text);
 const click=n=>{assert(n&&!n.disabled,`unavailable ${n?.outerHTML}`);n.scrollIntoView({block:'nearest'});n.click();};
 const count=()=>Number(one('.collections-tools>.muted')?.textContent.match(/已加载 (\d+)/)?.[1]);
 const ready=()=>one('.collections-page')?.getAttribute('aria-busy')==='false';
 const back=()=>click(one('[aria-label="返回上一页"]'));
 let stageCount=0;
 async function capture(name){await sleep(600);assert(document.documentElement.scrollWidth<=innerWidth,'horizontal overflow');assert(all('.catalog-card,.collection-follow-card').length<=30,'unbounded collection cards');await emit(`ux_${String(++stageCount).padStart(2,'0')}_${name}`,{width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,loaded:count()||0,title:one('.catalog-heading h1')?.textContent});await sleep(1200);}
 try{
  await wait(()=>one('.track-row'),'account');click(button('专辑与关注'));await wait(()=>ready()&&one('.catalog-card'),'saved albums');await capture('live_saved');
  click(one('.catalog-card'));await wait(()=>one('.album-header')&&one('.catalog-page .track-row'),'saved album');await capture('live_saved_album');back();await wait(ready,'back saved');
  click(button('已购专辑'));await wait(()=>ready()&&one('.catalog-card'),'purchased albums');await capture('live_purchased');
  click(one('.catalog-card'));await wait(()=>one('.album-header')&&one('.catalog-page .track-row'),'purchased album');await capture('live_purchased_album');back();await wait(ready,'back purchased');
  click(button('关注歌手'));await wait(()=>ready()&&one('.collection-follow-card'),'followed artists');await capture('live_followed');
  for(let i=0;button('加载更多')&&i<500;i++){const before=count();click(button('加载更多'));await sleep(50);await wait(ready,'followed next');assert(!one('.catalog-error'),'followed read failed');assert(count()>before||!button('加载更多'),'followed page did not advance');}
  assert(ready()&&!button('加载更多')&&!button('正在加载…'),'followed page limit or unfinished request');one('.collection-follow-scroll').scrollTop=999999;await capture('live_followed_end');
  one('.collection-follow-scroll').scrollTop=0;await sleep(350);click(one('button.collection-follow-card'));await wait(()=>one('.artist-header'),'followed artist');await capture('live_artist');back();await wait(()=>ready()&&button('关注歌手')?.getAttribute('aria-current')==='page','back scope');
  click(one('[aria-label="外观主题"]'));await wait(()=>button('深色'),'theme');click(button('深色'));await wait(()=>document.documentElement.dataset.theme==='dark','dark');await emit('resize',{width:760,height:540});await wait(()=>innerWidth===760,'small');await capture('live_small_dark');
  await emit('complete',{ok:true,stageCount,explicitRemoteWrites:0,playbackVerified:false});
 }catch(error){await emit('failed',{ok:false,error:String(error),stageCount,message:one('.catalog-error')?.textContent});}
})();
