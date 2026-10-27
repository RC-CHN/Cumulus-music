(async()=>{
 const sleep=ms=>new Promise(r=>setTimeout(r,ms)),el=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)];
 const button=t=>all('button').find(n=>n.textContent.trim()===t);
 const wait=async(fn,label)=>{const end=Date.now()+45000;while(Date.now()<end){if(fn())return;await sleep(100);}throw Error(label);};
 const click=async n=>{if(!n)throw Error('missing_control');await wait(()=>n.isConnected&&!n.disabled,'disabled_control');n.scrollIntoView({block:'nearest'});n.click();await sleep(200);};
 const state=()=>el('.playerbar')?.dataset.state,position=()=>Number(el('.playerbar')?.dataset.position),title=()=>el('.now-copy button')?.textContent.trim();
 const report=(stage,data={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'member-proof',payload:{stage,...data}});
 let query={kind:'home'};let step='boot';
 async function stage(name,data={}){await sleep(500);if(el('.member-error'))throw Error('member_ui_error');if(document.documentElement.scrollWidth>innerWidth||document.documentElement.scrollHeight>innerHeight)throw Error('layout_overflow');window.__memberAck='';await report(name,{query,width:innerWidth,height:innerHeight,overflow:false,playerTitle:title(),...data});await wait(()=>window.__memberAck===name,'native_ack');await sleep(1000);}
 async function pause(){if(state()==='playing')await click(el('.playerbar [aria-label="暂停"]'));await wait(()=>state()==='paused','paused');}
 async function seek(v){const slider=el('[aria-label="播放进度"]');await wait(()=>slider&&!slider.disabled,'seek_enabled');slider.value=String(v);slider.dispatchEvent(new Event('input',{bubbles:true}));slider.dispatchEvent(new Event('change',{bubbles:true}));let stable=0;await wait(()=>{stable=Math.abs(position()-v)<.6?stable+1:0;return stable>=8;},'native_seek');}
 try{
  await wait(()=>el('.account-button')&&!el('.account-button').disabled&&!el('.welcome'),'account');
  await click(button('会员中心'));await wait(()=>all('[data-entitlement]').length===5&&all('.member-track-play').length>0&&el('.member-page')?.getAttribute('aria-busy')==='false','member_loaded');
  if(window.__memberLinks){
   async function home(){await click(button('会员中心'));await wait(()=>el('.member-track-play')&&el('.member-page')?.getAttribute('aria-busy')==='false','home_return');}
   step='album_link';const album=el('[data-member-kind="albums"] .member-card');const albumId=album.dataset.memberId;await click(album);await wait(()=>el('.rankings-page .track-row')&&!el('.rankings-page [role="status"]'),'album_loaded');await stage('member_link_album',{id:albumId,title:el('.rankings-page h1').textContent.trim()});
   await home();step='playlist_link';const playlist=el('[data-member-kind="playlists"] .member-card');const playlistId=playlist.dataset.memberId;await click(playlist);await wait(()=>el('.playlist-hero')&&el('.track-row'),'playlist_loaded');await stage('member_link_playlist',{id:playlistId,title:el('.playlist-hero h1').textContent.trim()});
   await home();step='book_link';const book=el('[data-member-kind="audio_books"] .member-card');const radioId=book.dataset.memberId;await click(book);await wait(()=>el('.podcast-summary h2')&&el('.episode-row'),'book_loaded');await stage('member_link_book',{id:radioId,title:el('.podcast-summary h2').textContent.trim()});
   await home();step='comment_link';const track=el('.member-track');const trackId=track.dataset.trackId;const trackName=track.querySelector('strong').textContent.trim();await click(track.querySelector('[aria-label^="评论 "]'));await wait(()=>el('.comments-page h1 small')&&!el('.comments-page .spinner'),'comments_loaded');if(el('.comments-page .page-heading p').textContent.trim()!==trackName)throw Error('wrong_comments');await stage('member_link_comments',{id:trackId,title:trackName});
   await report('complete',{ok:true,query,screenshots:4,links:true,remoteMutations:0,paidPlaybackAttempted:false});return;
  }
  if(window.__memberReopen){
   await wait(()=>state()==='paused'&&Math.abs(position()-12)<.7,'restored_position');await stage('member_reopened');
   await click(el('.playerbar [aria-label="播放"]'));await wait(()=>state()==='playing'&&position()>12.3,'resumed_audio');await stage('member_resumed');await pause();await report('complete',{ok:true,query,screenshots:2,remoteMutations:0,audioOutput:'libmpv/null'});return;
  }
  step='home';el('.member-page').scrollTo(0,0);await stage('member_home',{recommendations:all('.member-track-play').length,rights:all('[data-entitlement]').map(n=>({kind:n.dataset.entitlement,active:n.dataset.active==='true'}))});
  step='recommend_play';const rec=all('.member-track-play')[1]??el('.member-track-play');const recTitle=rec.querySelector('strong').textContent.trim();await click(rec);await wait(()=>state()==='playing'&&title()===recTitle&&position()>.4,'member_play');await stage('member_recommend_play');
  step='history';await click(button('历史日推'));await wait(()=>all('.member-dates button').length>0&&el('.member-page')?.getAttribute('aria-busy')==='false','history_loaded');const first=el('.member-dates [aria-pressed="true"]').textContent.trim();query={kind:'history',date:first};await stage('member_history');
  step='history_date';const second=all('.member-dates button').find(n=>n.textContent.trim()!==first);if(!second)throw Error('second_date_unavailable');const secondDate=second.textContent.trim();await click(second);await wait(()=>el('.member-dates [aria-pressed="true"]')?.textContent.trim()===secondDate&&el('.member-page')?.getAttribute('aria-busy')==='false','history_date_changed');query={kind:'history',date:secondDate};await stage('member_history_date');
  step='history_play';await click(el('.member-history-actions .primary'));await wait(()=>state()==='playing'&&position()>.5&&title()!==recTitle,'history_play');await stage('member_history_play');
  step='download_entry';await click(button('下载全部'));await wait(()=>el('.download-dialog')&&el('.download-selection'),'download_dialog');await stage('member_download_dialog',{downloadTitle:el('#download-dialog-title').textContent.trim(),downloadSelection:el('.download-selection').textContent.trim()});await click(el('[aria-label="关闭下载对话框"]'));await wait(()=>!el('.download-dialog'),'download_cancel');
  step='save';await pause();await seek(12);await stage('member_saved');await report('complete',{ok:true,query,screenshots:7,remoteMutations:0,audioOutput:'libmpv/null',downloadStarted:false});
 }catch(e){await report('failed',{ok:false,step,query,error:e instanceof Error?e.message:'unknown',uiErrors:all('.member-error,.error-state').map(n=>n.textContent.trim())});}
})();
