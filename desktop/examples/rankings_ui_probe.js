(async()=>{
 const sleep=ms=>new Promise(r=>setTimeout(r,ms)),el=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)];
 const report=(stage,data={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'ranking-proof',payload:{stage,...data}});
 const wait=async(fn,label)=>{const end=Date.now()+40000;while(Date.now()<end){if(fn())return;await sleep(100);}throw Error(label);};
 const click=async n=>{if(!n)throw Error('missing_control');await wait(()=>n.isConnected&&!n.disabled,'disabled_control');n.scrollIntoView({block:'center'});n.focus({preventScroll:true});n.click();await sleep(150);};
 const text=t=>all('button').find(b=>b.textContent.trim()===t);
 const settled=()=>el('.rankings-page')?.getAttribute('aria-busy')==='false'||!!el('.catalog-page .album-header[aria-busy="false"]');
 const home=async()=>{await click(text('排行榜'));await wait(()=>settled()&&el('.ranking-groups'),'home');};
 async function stage(name,data={}){await sleep(450);const main=el('.main-content');if(name==='rank_official')el('.with-preview').closest('section').scrollIntoView({block:'start'});else main.scrollTo(0,0);await sleep(200);if(document.documentElement.scrollWidth>innerWidth||main.scrollWidth>main.clientWidth+1)throw Error('layout_overflow');window.__rankAck='';await report(name,{overflow:false,width:innerWidth,height:innerHeight,...data});await wait(()=>window.__rankAck===name,'native_ack');await sleep(900);}
 let step='boot';
 try{
  await wait(()=>el('.account-button')&&!el('.account-button').disabled&&!el('.welcome'),'account');
  await home();await stage('rank_home',{groups:all('.ranking-group').length,cards:all('.ranking-card').length});
  await stage('rank_official');
  step='album_sales';await click(el('[data-rank-id="albumRank"] .rank-open'));await wait(()=>settled()&&el('[aria-label="商品类型"]')&&el('.sale-album'),'album_sales');await stage('rank_album_daily',{rows:all('.rank-sale-row').length});
  await click(text('数字单曲'));await stage('rank_album_singles',{rows:all('.rank-sale-row').length});
  await click(el('[aria-label="榜单年份"]'));await wait(()=>el('.ranking-year-menu'),'year_menu');await click(el('.ranking-year-menu [data-menu-key="2018"]'));await wait(()=>settled()&&el('.sale-album'),'historical_year');await stage('rank_album_year',{year:2018,rows:all('.rank-sale-row').length});
  step='album';await click(el('.sale-album'));await wait(()=>settled()&&el('.album-header')&&el('.track-table'),'album_detail');await stage('rank_album_detail',{tracks:Number(el('.track-table').getAttribute('aria-rowcount'))-1});
  step='album_play';await click(text('播放全部'));await wait(()=>el('.playerbar [aria-label="暂停"]')&&Number(el('[aria-label="播放进度"]')?.value)>.3,'actual_album_playback');await stage('rank_album_playing',{tracks:Number(el('.track-table').getAttribute('aria-rowcount'))-1,audioOutput:'null'});await click(el('.playerbar [aria-label="暂停"]'));
  step='song_sales';await home();await click(el('[data-rank-id="songIndexRank"] .rank-open'));await wait(()=>settled()&&el('.song-sales-list'),'song_sales');await stage('rank_song_sales',{rows:all('.song-sales-list .rank-sale-row').length});
  step='play';await home();const card=el('[data-rank-id="19723756"]')||all('.with-preview .ranking-card').find(n=>n.querySelector('.rank-play'));await click(card.querySelector('.rank-play'));await wait(()=>el('.playerbar [aria-label="暂停"]')&&Number(el('[aria-label="播放进度"]')?.value)>.3,'actual_rank_playback');await stage('rank_playing',{playlist:card.dataset.rankId,audioOutput:'null'});
  await click(el('.playerbar [aria-label="暂停"]'));await report('complete',{ok:true,remoteMutations:0,audioOutput:'null',screenshots:9});
 }catch(e){await report('failed',{ok:false,step,error:e instanceof Error?e.message:'unknown'});}
})();
