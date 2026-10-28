// Explicit account reads + native mpv/null playback. No collection/download write.
(async()=>{
 const sleep=ms=>new Promise(r=>setTimeout(r,ms)),all=s=>[...document.querySelectorAll(s)],one=s=>document.querySelector(s);
 const assert=(ok,message)=>{if(!ok)throw Error(message);};
 const emit=(stage,rest={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...rest}});
 const wait=async(test,label)=>{for(let i=0;i<450;i++){if(test())return;await sleep(100);}throw Error(label);};
 const button=text=>all('button').find(n=>n.textContent.trim()===text);
 const click=node=>{assert(node&&!node.disabled,`unavailable ${node?.outerHTML}`);node.scrollIntoView({block:'nearest'});node.click();};
 let stageCount=0,recoveredFromUnavailable=false;
 async function capture(name,playing=false){await sleep(700);assert(document.documentElement.scrollWidth<=innerWidth,'horizontal overflow');await emit(`ux_${String(++stageCount).padStart(2,'0')}_${name}`,{playing,expectedQueue:playing?parseInt(one('.catalog-track-actions .muted')?.textContent??'',10):null,width:innerWidth,height:innerHeight,title:one('.catalog-heading h1')?.textContent,playerState:one('.playerbar')?.dataset.state,playerMessage:one('.playback-message')?.textContent});await sleep(1500);}
 async function playing(){await wait(()=>one('.playerbar')?.dataset.state==='playing'&&Number(one('.playerbar')?.dataset.position)>1,'native playback');}
 async function play(){await wait(()=>button('播放全部')&&!button('播放全部').disabled,'play ready');click(button('播放全部'));await sleep(100);await wait(()=>!button('播放全部').disabled,'selection completed');await playing();}
 try{
   await wait(()=>one('.track-row'),'account/daily');
   const input=one('.searchbox input');assert(input,'search input');input.value='周杰伦';input.dispatchEvent(new Event('input',{bubbles:true}));input.form.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));
   await wait(()=>all('.track-credit-links button').some(n=>n.textContent==='周杰伦'),'search artist reference');await capture('live_search');
   click(all('.track-credit-links button').find(n=>n.textContent==='周杰伦'));await wait(()=>one('.catalog-heading h1')?.textContent==='周杰伦'&&one('.catalog-page .track-row'),'artist details');await capture('live_artist');
   await play();await capture('live_artist_playing',true);
   click(button('查看全部歌曲 →'));await wait(()=>button('加载更多')&&!button('加载更多').disabled,'artist all songs');click(button('加载更多'));await wait(()=>one('.catalog-track-actions .muted')?.textContent.includes('40 首'),'artist second page');
   await wait(()=>button('播放已加载歌曲')&&!button('播放已加载歌曲').disabled,'loaded playback ready');click(button('播放已加载歌曲'));await sleep(100);await wait(()=>!button('播放已加载歌曲').disabled,'paged selection completed');await wait(()=>['playing','error'].includes(one('.playerbar')?.dataset.state),'paged playback result');await capture('live_artist_all_result');if(one('.playerbar')?.dataset.state==='error'){recoveredFromUnavailable=true;click(one('[aria-label="下一首"]'));}await playing();await capture('live_artist_all_playing',true);
   click(button('专辑'));await wait(()=>one('.catalog-card'),'album cards');await capture('live_albums');click(button('歌曲'));await wait(()=>one('.catalog-page .track-album-link'),'hot album link');click(one('.catalog-page .track-album-link'));await wait(()=>one('.catalog-label')?.textContent==='专辑'&&one('.catalog-page .track-row'),'album detail');await capture('live_album');
   await play();await capture('live_album_playing',true);
   const pause=one('[aria-label="暂停"]');if(pause)click(pause);
   click(one('[aria-label="外观主题"]'));await wait(()=>button('深色'),'theme');click(button('深色'));await wait(()=>document.documentElement.dataset.theme==='dark','dark');
   await emit('resize',{width:760,height:540});await wait(()=>innerWidth===760,'small');await capture('live_album_small_dark');
   await emit('complete',{ok:true,stageCount,nativeAudio:true,nullOutput:true,explicitRemoteWrites:0,recoveredFromUnavailable});
 }catch(error){await emit('failed',{ok:false,error:String(error),stageCount,playerState:one('.playerbar')?.dataset.state,playerMessage:one('.playback-message')?.textContent,position:one('.playerbar')?.dataset.position});}
})();
