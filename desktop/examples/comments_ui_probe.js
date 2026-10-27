(async()=>{
 if(window.__commentsStarted)return;window.__commentsStarted=true;
 const sleep=ms=>new Promise(r=>setTimeout(r,ms)),el=s=>document.querySelector(s),all=s=>[...document.querySelectorAll(s)];
 const invoke=(name,args={})=>window.__TAURI_INTERNALS__.invoke(name,args);
 const report=(stage,detail={})=>invoke('plugin:event|emit',{event:'comments-proof',payload:{stage,...detail}});
 const wait=async(test,label,ms=65000)=>{const end=Date.now()+ms;while(Date.now()<end){const value=await test();if(value)return value;await sleep(100);}throw Error(label);};
 const button=(text,scope=document)=>[...scope.querySelectorAll('button')].find(n=>n.textContent.trim()===text);
 const click=async n=>{await sleep(40);if(!n||n.disabled)throw Error('control_unavailable_'+(n?.getAttribute('aria-label')||n?.textContent.trim()||'missing'));n.scrollIntoView({block:'nearest'});n.click();await sleep(100);};
 const input=(label,value)=>{const n=el(`[aria-label="${label}"]`);if(!n)throw Error('input_missing_'+label);n.value=value;n.dispatchEvent(new Event('input',{bubbles:true}));};
 const stage=async(name,data={})=>{await sleep(400);await report(name,{...data,horizontalOverflow:document.documentElement.scrollWidth>innerWidth});await sleep(900);};
 const ids=()=>all('[data-group="page"] .comment-card').map(n=>n.dataset.commentId);
 const sample=window.__commentsProof.sample;let epoch,step='start',restored=!window.__commentsProof.mutate;
 try {
  await wait(()=>all('.track-row').length,'daily_bootstrap');epoch=(await invoke('account_snapshot')).epoch;
  input('搜索音乐',sample.query);el('[aria-label="搜索音乐"]').closest('form').dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));
  await wait(()=>el('.page-heading h1')?.textContent.includes('搜索')&&all('.track-row').length,'search_results');
  const results=(await invoke('search_songs',{keyword:sample.query,page:{offset:0,limit:100}})).value.tracks,index=results.findIndex(t=>t.id===sample.resource.id);if(index<0)throw Error('explicit_track_not_in_search');
  const scroll=el('.track-scroll');scroll.scrollTop=index*56;scroll.dispatchEvent(new Event('scroll'));
  const row=await wait(()=>el(`.track-row[aria-rowindex="${index+2}"]`),'explicit_track_visible');await click(row.querySelector('.track-more'));await wait(()=>button('查看歌曲评论'),'track_menu');await click(button('查看歌曲评论'));
  step='latest';await wait(()=>el('.comments-page')&&ids().length,'comment_page');const first=ids();await stage('comments_latest',{count:first.length,hotCount:all('[data-group="hot"] .comment-card').length});
  step='next';await click(button('下一页'));await wait(()=>ids().length&&ids()[0]!==first[0],'next_page');if(ids().some(id=>first.includes(id)))throw Error('page_overlap');await stage('comments_next',{count:ids().length,overlap:0});await click(button('上一页'));await wait(()=>ids()[0]===first[0],'previous_page');
  step='hot';await click(button('精彩评论',el('[role="tablist"]')));await wait(()=>button('精彩评论',el('[role="tablist"]'))?.getAttribute('aria-selected')==='true'&&ids().length,'hot_page');await stage('comments_hot',{count:ids().length});
  step='target';input('评论编号',sample.comment_id);el('[aria-label="评论编号"]').closest('form').dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));await wait(()=>el(`[data-group="current"] [data-comment-id="${sample.comment_id}"]`),'target_comment');await stage('comments_target',{targetMatched:true});
  if(window.__commentsProof.mutate){
   step='like';const target=el('[data-group="current"]');const like=target.querySelector('.comment-like');if(like.getAttribute('aria-pressed')!=='false')throw Error('preserve_already_liked_comment');await click(like);await wait(()=>el('[data-group="current"] .comment-like')?.getAttribute('aria-pressed')==='true'&&!el('[data-group="current"] .comment-like').disabled,'like_verified');await stage('comments_liked',{liked:true});
   step='restore';await click(el('[data-group="current"] .comment-like'));await wait(()=>el('[data-group="current"] .comment-like')?.getAttribute('aria-pressed')==='false'&&!el('[data-group="current"] .comment-like').disabled,'unlike_verified');
   const verify=(await invoke('comments_page',{epoch,resource:sample.resource,query:{sort:'latest',limit:20,offset:0,beforeTime:0,commentId:sample.comment_id},refresh:true})).value;
   if(verify.currentComment?.liked!==false||verify.operations.length)throw Error('comment_baseline_not_restored');restored=true;await stage('comments_restored',{liked:false,pendingOperations:0});
  }
  step='playlist';await click(button('我的歌单'));await wait(()=>button('打开歌单链接'),'playlist_library');await click(button('打开歌单链接'));await wait(()=>el('[aria-label="歌单分享链接"]'),'playlist_link');input('歌单分享链接',sample.playlist_id);await click(el('.playlist-dialog button.primary'));await wait(()=>button('歌单评论'),'playlist_detail');await click(button('歌单评论'));await wait(()=>el('.comments-page')&&ids().length,'playlist_comments');await stage('comments_playlist',{count:ids().length});
  step='mv';await click(button('收藏的 MV'));await wait(()=>el('[aria-label="MV编号或链接"]'),'mv_collection');input('MV编号或链接',sample.mv_id);el('[aria-label="MV编号或链接"]').closest('form').dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));await wait(()=>el('[aria-label="MV 评论"]')&&!el('[aria-label="MV 评论"]').disabled,'mv_detail');await click(el('[aria-label="MV 评论"]'));await wait(()=>el('.comments-page')&&ids().length,'mv_comments');await stage('comments_mv',{count:ids().length});
  if(el('.mv-mini [aria-label="关闭视频"]'))await click(el('.mv-mini [aria-label="关闭视频"]'));
  await report('complete',{ok:true,latestAndHot:true,pagination:true,targetLookup:true,songPlaylistMvEntrances:true,likeRoundtrip:window.__commentsProof.mutate,baselinePreserved:restored,commentsPublished:0,messagesSent:0,remoteMutations:window.__commentsProof.mutate?2:0,width:innerWidth,height:innerHeight});
 }catch(e){await report('failed',{ok:false,step,error:e instanceof Error?e.message:'native_operation_failed',baselinePreserved:restored});}
})();
