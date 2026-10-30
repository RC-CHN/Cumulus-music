(async()=>{
  const sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const all=s=>[...document.querySelectorAll(s)],one=s=>document.querySelector(s);
  const assert=(ok,message)=>{if(!ok)throw Error(message);};
  const emit=(stage,rest={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...rest}});
  const wait=async(test,label)=>{for(let i=0;i<150;i++){if(test())return;await sleep(100);}throw Error(label);};
  const button=text=>all('button').find(n=>n.textContent.trim()===text);
  const click=node=>{assert(node&&!node.disabled,`unavailable ${node?.outerHTML}`);node.scrollIntoView({block:'nearest'});node.focus({preventScroll:true});node.click();};
  let stageCount=0;
  async function capture(name){await sleep(500);assert(document.documentElement.scrollWidth<=innerWidth,'horizontal overflow');assert(!one('.catalog-page')||one('.catalog-page').getBoundingClientRect().bottom<=one('.main-content').getBoundingClientRect().bottom+1,'catalog page is clipped');assert(all('.track-row').length<=30,'unbounded song rows');assert(all('.catalog-card').length<=30,'unbounded cards');await emit(`ux_${String(++stageCount).padStart(2,'0')}_${name}`,{width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,rows:all('.track-row').length,cards:all('.catalog-card').length,title:one('.catalog-heading h1')?.textContent});await sleep(1300);}
  async function tab(label,selector){const keys={'专辑':'albums','视频':'videos','歌手详情':'intro','相似歌手':'similar','演出':'concerts','歌曲':'songs'};click(one('[data-catalog-tab="'+keys[label]+'"]'));await wait(()=>one(selector),'tab '+label);await sleep(300);}
  async function collected(verb='收藏'){await wait(()=>button(verb)&&!button(verb).disabled,'collection known');click(button(verb));await wait(()=>button('已'+verb)&&!button('已'+verb).disabled,'collection confirmation');}
  async function theme(value='深色'){click(one('[aria-label="外观主题"]'));await wait(()=>button('深色'),'theme menu');click(button(value));await wait(()=>document.documentElement.dataset.theme===(value==='深色'?'dark':'light'),'theme change');}
  try {
    await wait(()=>one('.track-credit-links button'),'daily references');await theme('浅色');
    click(one('.track-credit-links button'));await wait(()=>one('.catalog-heading h1')?.textContent==='测试歌手'&&one('.catalog-page .track-row'),'artist');
    await capture('artist_hot');await collected('关注');await capture('artist_collected');
    click(button('查看全部歌曲 →'));await wait(()=>one('.artist-all-title')?.textContent.includes('全部歌曲')&&button('加载更多'),'all songs');
    await capture('artist_all_20');click(button('加载更多'));await wait(()=>one('.catalog-track-actions .muted')?.textContent.includes('40 首'),'second songs');
    click(button('加载更多'));await wait(()=>one('.catalog-track-actions .muted')?.textContent.includes('55 首')&&!button('加载更多'),'last songs');
    const tracks=one('.track-scroll');assert(tracks,'song scroller');tracks.scrollTop=tracks.scrollHeight;await sleep(300);await capture('artist_all_55');
    click(button('返回热门歌曲'));await wait(()=>one('.artist-tabs'),'return artist');
    await tab('专辑','.catalog-card');await capture('artist_albums');
    click(button('加载更多'));await wait(()=>!button('正在加载…')&&!one('.catalog-loading'),'album second page');
    const grid=one('.catalog-card-scroll');grid.scrollTop=grid.scrollHeight;await sleep(350);await capture('artist_albums_scrolled');grid.scrollTop=0;await sleep(350);
    click(one('.catalog-card'));await wait(()=>one('.catalog-heading h1')?.textContent==='测试专辑','album');await capture('album');
    await collected();await capture('album_collected');
    click(button('下载'));await wait(()=>one('dialog[open]'),'download options');await capture('album_download_options');
    const close=one('dialog[open] [aria-label*="关闭"]');click(close);await wait(()=>!one('dialog[open]'),'download close');
    click(one('.catalog-credits button'));await wait(()=>one('.catalog-section-heading'),'artist from album');
    await tab('歌手详情','.catalog-description h2');await capture('artist_introduction');
    await tab('相似歌手','.catalog-card.artist');await capture('similar_artists');click(one('.catalog-card.artist'));await wait(()=>one('.catalog-heading h1')?.textContent==='相似歌手 13','similar navigation');await capture('similar_open');
    await tab('视频','.catalog-video-card');assert(all('.catalog-video-card button').filter(n=>n.disabled).length===1,'video kind classification');await capture('artist_videos');
    await tab('演出','.catalog-concert-card');await capture('artist_concert');
    await theme();await capture('artist_dark');
    await emit('resize',{width:760,height:540});await wait(()=>innerWidth===760,'small window');await capture('artist_small');
    await tab('歌曲','.catalog-page .track-row');await capture('artist_small_songs');
    click(one('.catalog-page .track-album-link'));await wait(()=>one('.catalog-heading h1')?.textContent==='测试专辑','album song link');await capture('album_small_dark');
    await emit('complete',{ok:true,stageCount,realMusicRequests:0,syntheticMembershipWrites:true,playbackVerified:false});
  } catch(error){await emit('failed',{ok:false,error:String(error),stageCount});}
})();
