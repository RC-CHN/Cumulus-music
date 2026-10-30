// Executed in the real bundled WebKit view by the standalone Rust probe.
// Only DOM interactions and bounded aggregate reports; no credentials logged.
(async () => {
  if(window.__cumulusProofStarted) return;
  window.__cumulusProofStarted=true;
  const sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const report=(stage,detail={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'ui-proof',payload:{stage,...detail}});
  const wait=async(test,label)=>{
    const until=Date.now()+65000;
    while(Date.now()<until){if(test()) return;await sleep(200);}
    throw new Error(label);
  };
  const stage=async(name,detail={})=>{await sleep(700);await report(name,detail);await sleep(1500);};
  try {
    await wait(()=>document.querySelectorAll('.track-row').length>0,'daily_render');
    await stage('daily',{visibleRows:document.querySelectorAll('.track-row').length,sidebarPlaylists:document.querySelectorAll('.playlist-nav').length});
    if(window.__runCloudProof){
      const result=await window.__runCloudProof({wait,stage,sleep});
      await report('complete',{ok:true,...result});return;
    }
    if(window.__runDownloadProof){
      const result=await window.__runDownloadProof({wait,stage,sleep});
      await report('complete',{ok:true,...result});return;
    }
    document.querySelectorAll('.nav-main button')[1].click();
    await wait(()=>document.querySelectorAll('.playlist-card').length>0,'library_render');
    await stage('library',{playlists:document.querySelectorAll('.playlist-card').length});
    document.querySelectorAll('.nav-main button')[2].click();
    await wait(()=>document.querySelector('.playlist-hero') && document.querySelectorAll('.track-row').length>0,'playlist_render');
    const more=document.querySelector('.load-more');
    if(!more) throw new Error('expected_large_favorite_playlist');
    more.click();
    await wait(()=>document.querySelector('.track-table')?.getAttribute('aria-rowcount')==='201','playlist_second_page');
    const scroll=document.querySelector('.track-scroll');scroll.scrollTop=56*150;scroll.dispatchEvent(new Event('scroll'));
    await wait(()=>Number(document.querySelector('.track-row')?.getAttribute('aria-rowindex'))>100,'virtual_scroll');
    await stage('playlist',{loadedRows:200,renderedRows:document.querySelectorAll('.track-row').length,virtualized:document.querySelectorAll('.track-row').length<40});
    const input=document.querySelector('.searchbox input');input.value='音乐';input.dispatchEvent(new Event('input',{bubbles:true}));
    document.querySelector('.searchbox').dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));
    await wait(()=>document.querySelector('.search-heading-copy h1')?.textContent==='音乐' && document.querySelectorAll('.search-track-card').length>0,'search_render');
    if(document.documentElement.scrollWidth>innerWidth) throw new Error('horizontal_overflow');
    await stage('search',{visibleRows:document.querySelectorAll('.search-track-card').length,windowWidth:innerWidth,windowHeight:innerHeight,horizontalOverflow:false});
    document.querySelector('.account-button').click();
    await wait(() => document.querySelector('.account-add'), 'account chooser');
    document.querySelector('.account-add').click();
    await wait(()=>document.querySelector('.qr-frame svg path')?.getAttribute('d')?.length>100,'qr_render');
    await stage('qr',{dialogOpen:document.querySelector('dialog').open});
    document.querySelector('.dialog-close').click();
    await wait(()=>!document.querySelector('dialog'),'qr_cancel');
    const account=await window.__TAURI_INTERNALS__.invoke('account_snapshot');
    if(account.status!=='authenticated') throw new Error('cancel_replaced_account');
    if(window.__runPlayerProof) await window.__runPlayerProof({wait,stage,sleep});
    const playlistProof=window.__runPlaylistProof?await window.__runPlaylistProof({wait,stage,sleep}):undefined;
    await report('complete',{ok:true,...(playlistProof?playlistProof:{remoteMutations:0}),qrCancelled:true,accountPreserved:true});
  } catch(error) {
    await report('failed',{ok:false,step:error instanceof Error ? error.message : 'unknown',player:window.__playerProofFacts?.(),playlist:window.__playlistProofFacts?.(),download:window.__downloadProofFacts?.(),cloud:window.__cloudProofFacts?.()});
  }
})();
