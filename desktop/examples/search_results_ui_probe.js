// Real Tauri/WebView and production IPC; synthetic read responses, no account writes.
(async()=>{
  const one=s=>document.querySelector(s), all=s=>[...document.querySelectorAll(s)], sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const assert=(v,m)=>{if(!v)throw Error(m);};
  const wait=async(f,m)=>{for(let i=0;i<200;i++){if(f())return;await sleep(70);}throw Error(m);};
  const button=(label,root=document)=>[...root.querySelectorAll('button')].find(n=>n.getAttribute('aria-label')===label||n.textContent.trim()===label);
  const click=n=>{assert(n&&!n.disabled,'missing/disabled control '+n?.textContent);n.focus({preventScroll:true});n.click();};
  const emit=(stage,data={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...data}});
  const key=(n,k)=>n.dispatchEvent(new KeyboardEvent('keydown',{key:k,bubbles:true,cancelable:true}));
  let count=0,step='startup',holdCategory,holdKeyword,held=false,release,gate;
  const songReads=[], filteredMvReads=[];
  const fetch=window.fetch;
  window.fetch=async function(url,options){const query=String(url).endsWith('/search_results')?JSON.parse(options.body).query:null;if(window.__cumulusSearchSongWaterfallProof&&query?.category==='songs')songReads.push([query.offset,query.limit]);if(window.__cumulusSearchFilteredPaginationProof&&query?.category==='mvs')filteredMvReads.push([query.offset,query.limit]);const delayed=query?.category===holdCategory&&(!holdKeyword||query?.keyword===holdKeyword)?gate:null;const response=await fetch.call(window,url,options);if(delayed){held=true;await delayed;}return response;};
  const hold=(category,keyword)=>{held=false;holdCategory=category;holdKeyword=keyword;gate=new Promise(r=>{const timer=setTimeout(r,10000);release=()=>{clearTimeout(timer);holdCategory=undefined;gate=undefined;r();};});};
  const fixture=async mode=>{window.__searchFixtureAck=false;await emit('search-fixture',{mode});await wait(()=>window.__searchFixtureAck,'fixture acknowledgement');};
  const ready=()=>one('#search-result-panel')?.getAttribute('aria-busy')==='false' && !one('.search-results-leave-active');
  async function choose(category){click(one(`[data-category="${category}"]`));await wait(()=>one(`[data-category="${category}"]`)?.getAttribute('aria-selected')==='true'&&ready()&&(one('.search-result-section')||one('.search-result-panel .empty-state')),'category '+category);await sleep(180);}
  async function capture(label){
    step=label;await wait(()=>all('.search-result-panel,dialog').every(n=>n.getAnimations().every(a=>a.playState!=='running'&&!a.pending)),'settled animation');await sleep(160);
    const panel=one('.search-result-panel');
    const info={label,theme:document.documentElement.dataset.theme,width:innerWidth,height:innerHeight,noOverflow:document.documentElement.scrollWidth<=innerWidth,panelNoOverflow:!panel||panel.scrollWidth<=panel.clientWidth+1,cardCount:all('.search-result-card').length,renderedTracks:all('.track-row').length};
    assert(info.noOverflow&&info.panelNoOverflow,JSON.stringify(info));assert(info.cardCount<150&&info.renderedTracks<80,'bounded result DOM');
    if(one('[data-category="all"]')?.getAttribute('aria-selected')==='true') { assert(!one('.search-result-panel .track-table'),'comprehensive songs use cards'); assert(all('.search-track-card').length<=6,'bounded song previews');if(one('.search-result-tracks'))assert(one('.search-track-card'),'ready song section has previews'); }
    const first=one('.search-result-name'),category=one('[data-category][aria-selected="true"]')?.dataset.category;if(first&&category!=='all'&&panel.scrollTop===0&&!one('dialog[open]')) assert(first.getBoundingClientRect().bottom<=one('.playerbar').getBoundingClientRect().top,'first caption above player');
    for(const n of all('.search-result-actions button')){const r=n.getBoundingClientRect();assert(Math.abs(r.width-94)<1&&Math.abs(r.height-40)<1,'official 94x40 result action');}
    await emit(`ux_${String(++count).padStart(2,'0')}_${label}`,info);await sleep(500);
  }
  async function theme(w,h,t){await emit('resize',{width:w,height:h});await wait(()=>innerWidth===w&&innerHeight===h,'window resize');click(button('外观主题'));await wait(()=>one('.action-menu'),'theme menu');click(button(t==='dark'?'深色':'浅色'));await wait(()=>!one('.action-menu')&&document.documentElement.dataset.theme===t,'theme');}
  const submit=word=>{const input=one('[aria-label="搜索音乐"]');input.value=word;input.dispatchEvent(new Event('input',{bubbles:true}));input.form.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));};
  async function fresh(word){submit(word);await sleep(0);await wait(()=>one('.search-results-page h1')?.textContent===word&&one('[data-category="all"]')?.getAttribute('aria-selected')==='true'&&ready()&&!one('.page-leave-active,.page-enter-active'),'fresh search '+word);await sleep(200);}
  let profileWithoutFrames=false;
  async function userProfile(source, stall=false){
    const searchCard=source.closest('[data-result-key]').dataset.resultKey;
    const category=one('[data-category][aria-selected="true"]').dataset.category;
    if(stall) source.scrollIntoView({block:'nearest'});
    const position=one('.search-result-panel').scrollTop;
    if(stall) assert(position>0,'comprehensive profile starts from a scrolled panel');
    click(source);
    await wait(()=>one('.playlist-creator-page h1')?.textContent==='合成用户主页 501'&&one('.playlist-creator-page [data-library-open="30"]'),'verified user home and public playlists');
    assert(!one('.search-user-dialog'),'no legacy info modal');
    if(!stall) return;
    const frames=window.requestAnimationFrame;
    let blocked=0;
    window.requestAnimationFrame=()=>-(++blocked);
    try {
      click(button('返回上一页'));
      await wait(()=>ready()&&one(`[data-category="${category}"]`)?.getAttribute('aria-selected')==='true'&&document.activeElement.closest('[data-result-key]')?.dataset.resultKey===searchCard&&Math.abs(one('.search-result-panel').scrollTop-position)<=1,'Back restores exact scroll and search card without frames');
      assert(blocked>=2,'user-profile Back encountered frame stall');
      profileWithoutFrames=true;
    } finally { window.requestAnimationFrame=frames; }
  }
  try {
    if(window.__cumulusSearchFilteredPaginationProof){
      await wait(()=>one('.track-row'),'daily ready');
      const provides=one('#app').__vue_app__._context.provides;
      const core=provides[Reflect.ownKeys(provides).find(k=>typeof k==='symbol'&&k.description==='core-client')];
      const searchResults=core.searchResults;
      // Preserve the native fixture's real IPC cursor, but model its projection
      // filtering all rows on two pages without rebuilding the shared host.
      core.searchResults=async(...args)=>{
        const result=await searchResults(...args);
        const query=args[1];
        if(query.category!=='mvs'||query.offset>=26)return result;
        const consumed=result.value.nextOffset-query.offset;
        assert(consumed===13&&result.value.more,'native fixture supplies an advancing cursor');
        return {...result,value:{...result.value,sections:[],unavailableCount:consumed}};
      };
      await fresh('合成过滤结果');
      await choose('mvs');
      const panel=()=>one('#search-result-panel');
      const more=()=>button('加载更多',panel());
      const reads=()=>JSON.stringify(filteredMvReads);
      assert(more()&&panel().querySelector('.empty-state')?.textContent.includes('这一页暂无可展示的结果')&&
        !panel().querySelector('.search-result-card'),'filtered first page keeps a visible continuation');
      await capture('filtered_mv_first_page');
      assert(reads()==='[[0,13]]','empty first page does not auto-advance');
      click(more());
      await wait(()=>more()&&panel().textContent.includes('这一批结果暂无法展示，可以继续加载。'),
        'filtered append still offers continuation');
      assert(!panel().querySelector('.search-result-card'),'filtered append has no visible cards');
      await capture('filtered_mv_next_page');
      assert(reads()==='[[0,13],[13,13]]','filtered append consumes exactly one native cursor');
      click(more());
      await wait(()=>panel().querySelector('.search-result-card[data-result-key^="mv:"]'),
        'valid MVs appear after the filtered pages');
      assert(!panel().querySelector('.empty-state'),'visible MVs replace the empty state');
      await capture('filtered_mv_reaches_results');
      assert(reads()==='[[0,13],[13,13],[26,13]]','native cursors include filtered rows');
      core.searchResults=searchResults;
      await fixture('empty');
      const emptyWord='合成空结果'.repeat(8);
      await fresh(emptyWord);
      for(const [category,label] of [['artists','歌手'],['users','用户'],['mvs','MV']]){
        await choose(category);
        const keyword=panel().querySelector('.search-empty-keyword');
        assert(keyword?.textContent===emptyWord&&keyword.parentElement.textContent.includes('的任何'+label+'呀！'),
          'terminal empty result identifies the effective query and category');
        assert(keyword.clientWidth<=200&&keyword.scrollWidth>keyword.clientWidth&&!more(),
          'long query is visually bounded and an exhausted search cannot continue');
        await capture('empty_category_'+category);
      }
      await emit('complete',{ok:true,searchFilteredPaginationVerified:true,searchCategoryEmptyVerified:true,stageCount:count,filteredMvReads:filteredMvReads.slice(0,3),realMusicRequests:0,realAccountWrites:0});
      return;
    }
    if(window.__cumulusSearchSuggestionFocusProof){
      await wait(()=>one('.track-row'),'daily ready');
      const provides=one('#app').__vue_app__._context.provides;
      const core=provides[Reflect.ownKeys(provides).find(k=>typeof k==='symbol'&&k.description==='core-client')];
      const discovery=core.searchDiscovery;
      const input=one('[aria-label="搜索音乐"]'), submitButton=button('开始搜索');
      const exact=()=>one('.search-suggestion.search-exact');
      const choice=()=>one('.search-suggestion[data-search-suggestion]');
      let receipt, complete;
      core.searchDiscovery=async(...args)=>{
        if(args[1].kind!=='suggestions')return discovery(...args);
        const waitForRelease=new Promise(resolve=>{receipt=resolve;});
        const result=await discovery(...args);
        complete=true;
        await waitForRelease;
        return {...result,value:{...result.value,suggestions:[{keyword:args[1].keyword,label:args[1].keyword+'建议',tag:null}]}};
      };
      async function pending(word){
        receipt=undefined;complete=false;
        input.focus();input.click();input.value=word;
        input.dispatchEvent(new Event('input',{bubbles:true}));
        await wait(()=>complete&&exact(),'pending exact search has a delayed suggestion reply');
        key(input,'ArrowDown');
        assert(document.activeElement===exact(),'ArrowDown focuses exact search while the reply is pending');
      }
      try{
        await pending('合成焦点');
        receipt();
        await wait(()=>choice()&&!exact(),'suggestion replaces the temporary exact action');
        assert(document.activeElement===input,'temporary exact action returns its owned focus to the search input');
        key(input,'ArrowDown');
        assert(document.activeElement===choice(),'keyboard can continue directly to the new suggestion');
        await capture('suggestion_focus_handoff');
        await pending('合成不抢焦点');
        submitButton.focus();
        receipt();
        await wait(()=>choice()?.textContent.includes('不抢焦点')&&!exact(),'second suggestion reply');
        assert(document.activeElement===submitButton,'reply must not steal focus that already left the exact action');
        await capture('suggestion_focus_untouched');
      } finally{receipt?.();core.searchDiscovery=discovery;}
      await emit('complete',{ok:true,searchSuggestionFocusVerified:true,stageCount:count,realMusicRequests:0,realAccountWrites:0});
      return;
    }
    if(window.__cumulusSearchAlbumArtistProof){
      await wait(()=>one('.track-row'),'daily ready');
      await fixture('album_artist_search');await fresh('合成艺人');
      const card=()=>one('[data-result-key="album:201"]');
      const artist=()=>card()?.querySelector('[data-result-action="artist:101:1"]');
      for(const category of ['all','albums']){
        if(category!=='all')await choose(category);
        const target=artist();
        assert(card()?.querySelector('.search-card-open')&&card().querySelector('.search-card-play'),'album details and playback are independent');
        assert([...card().querySelectorAll('.search-album-artist')].map(node=>node.textContent.trim()).join(',')==='合成主艺人,合成合作艺人',
          'primary and credited artists are independent controls in order');
        assert(target?.getAttribute('aria-label')==='查看歌手 合成合作艺人'&&
          card().querySelector('.search-album-artist-text')?.textContent.trim()==='合成未标识艺人',
          'credit without native identity is plain text');
        click(target);
        await wait(()=>one('.artist-header h1')?.textContent.trim()==='相似歌手 101'&&one('.track-row'),
          'opens native details for the selected artist');
        click(button('返回上一页'));
        await wait(()=>ready()&&one(`[data-category="${category}"]`)?.getAttribute('aria-selected')==='true'&&
          document.activeElement===artist(),'Back restores the exact credited artist button');
        await capture('album_artist_'+category);
      }
      await emit('complete',{ok:true,searchAlbumArtistVerified:true,stageCount:count,realMusicRequests:0,realAccountWrites:0});
      return;
    }
    if(window.__cumulusSearchCollectionNextProof){
      await wait(()=>one('.track-row'),'daily ready');
      await fixture('good');await fresh('合成下一首合集');
      const invoke=(name,args={})=>window.__TAURI_INTERNALS__.invoke(name,args);
      const {epoch}=await invoke('account_snapshot');
      for(const [kind,category,id,total,feedback,done] of [
        ['playlist','all',3,40,'.playlist-action-feedback','已添加至播放列表'],
        ['album','albums',201,11,'.album-action-feedback','已设为下一首播放'],
      ]){
        if(category!=='all')await choose(category);
        await invoke('play_selection',{epoch,selection:{kind:'songs',ids:['91','92'],index:0,quality:'standard'}});
        await wait(()=>one('.playerbar')?.dataset.state==='playing','ordinary audio loaded');
        const paused=await invoke('player_action',{epoch,action:{kind:'pause',paused:true}});
        const before=await invoke('player_queue',{epoch});
        assert(paused.track?.id==='91'&&paused.state==='paused','ordinary selection paused');
        const card=one(`[data-result-key="${kind}:${id}"]`);
        const anchor=card?.querySelector('.search-card-open');
        assert(anchor&&!anchor.disabled,'collection opens from search');
        anchor.scrollIntoView({block:'nearest'});
        anchor.focus({preventScroll:true});
        anchor.dispatchEvent(new KeyboardEvent('keydown',{key:'F10',shiftKey:true,bubbles:true,cancelable:true}));
        await wait(()=>one('.action-menu [data-menu-key="next"]'),'collection next action');
        const next=one('.action-menu [data-menu-key="next"]');
        assert(next.textContent.trim()==='下一首播放'&&!next.disabled,'enabled next action');
        click(next);
        await wait(()=>one(feedback)?.textContent.includes(done),'collection queue feedback');
        const after=await invoke('player_snapshot',{epoch});
        const queue=await invoke('player_queue',{epoch});
        assert(after.playId===paused.playId&&after.state==='paused'&&Math.abs(after.position-paused.position)<.05,
          'collection next preserves the paused decoder');
        assert(queue.tracks[0].entryId===before.tracks[0].entryId&&queue.tracks.at(-1).entryId===before.tracks[1].entryId,
          'original queue entries remain at both ends');
        assert(queue.tracks.slice(1,-1).map(row=>row.track.id).join(',')===Array.from({length:total},(_,i)=>i+1).join(','),
          'collection tracks insert in authoritative order');
        assert(document.activeElement===anchor,'menu returns focus to the initiating result');
        await capture(kind+'_search_next_retains_decoder');
      }
      await emit('complete',{ok:true,searchCollectionNextVerified:true,stageCount:count,realMusicRequests:0,realAccountWrites:0});
      return;
    }
    if(window.__cumulusSearchPlaylistCreatorProof){
      await wait(()=>one('.track-row'),'daily ready');
      await fixture('playlist_creator_search');await fresh('合成作者入口');
      const card=id=>one(`[data-result-key="playlist:${id}"]`);
      const creator=()=>card(3)?.querySelector('.search-playlist-creator');
      for(const category of ['all','playlists']){
        if(category!=='all')await choose(category);
        const action=creator(), bad=card(4);
        assert(action?.getAttribute('aria-label')==='查看创建者 合成匹配作者 的主页','typed creator action');
        assert(card(3).querySelector('.search-card-open')&&card(3).querySelector('.search-card-play'),'playlist open and playback remain separate');
        assert(bad?.textContent.includes('合成不匹配作者')&&!bad.querySelector('.search-playlist-creator'),
          'mismatched creator remains plain text');
        click(action);
        await wait(()=>one('.playlist-creator-page h1')?.textContent==='合成用户主页 501'&&
          one('.playlist-creator-page [data-library-open="30"]'),'creator profile and public playlists');
        click(button('返回上一页'));
        await wait(()=>ready()&&one(`[data-category="${category}"]`)?.getAttribute('aria-selected')==='true'&&
          document.activeElement===creator(),'Back restores creator button, not the card opener');
        await capture('playlist_creator_'+category);
      }
      click(card(3).querySelector('.search-card-open'));
      await wait(()=>!one('.playlist-creator-page')&&one('.playlist-hero h1')?.textContent.trim()==='合成歌单 3'&&one('.track-row'),'playlist card still opens playlist');
      await emit('complete',{ok:true,searchPlaylistCreatorVerified:true,stageCount:count,realMusicRequests:0,realAccountWrites:0});
      return;
    }
    if(window.__cumulusSearchMvProof){
      await wait(()=>one('.track-row'),'daily ready');
      await fixture('good');await fresh('合成 MV');
      function checkMvMetadata(card){
        assert(card?.querySelector('.search-video-count')?.textContent.trim()==='1.2万'&&
          card?.querySelector('.search-video-duration')?.textContent.trim()==='04:00','MV badges display available source metadata');
        const description=card.querySelector('.search-card-open')?.getAttribute('aria-describedby');
        assert(document.getElementById(description)?.textContent.trim()==='播放次数：12,345，时长：04:00','play target describes full metadata');
      }
      checkMvMetadata(one('[data-result-key="mv:601"]'));
      await choose('mvs');
      const card=one('[data-result-key="mv:601"]');
      checkMvMetadata(card);
      const trigger=button('播放 MV 合成 MV 1',card);
      const cue=card?.querySelector('.search-video-play-glyph');
      const placeholder=card?.querySelector('.search-result-art > .icon');
      assert(trigger&&!trigger.disabled&&trigger.tabIndex===0&&cue?.getAttribute('aria-hidden')==='true'&&
        !card.querySelector('.search-card-play'),'one enabled card activation target and decorative cue');
      trigger.focus({preventScroll:true});
      await wait(()=>document.activeElement===trigger&&Number(getComputedStyle(cue).opacity)>.9,'MV cue visible at focus');
      assert(placeholder&&Number(getComputedStyle(placeholder).opacity)===0,'Play cue does not overlap the empty-cover icon');
      await capture('search_mv_play_target');
      click(trigger);
      const invoke=(name,args={})=>window.__TAURI_INTERNALS__.invoke(name,args);
      const {epoch}=await invoke('account_snapshot');
      let native;
      for(let i=0;i<170;i++){
        const view=await invoke('video_snapshot',{epoch});
        if(view.id==='601'&&view.state==='playing'&&view.position>.15&&Number(one('.video-canvas')?.dataset.renderedFrames)>1){native=view;break;}
        await sleep(70);
      }
      assert(native&&native.actualResolution===480,'search card reached advancing native video playback');
      const canvas=one('.video-canvas');
      const pixels=canvas.getContext('2d').getImageData(0,0,canvas.width,canvas.height).data;
      assert(pixels.some((value,index)=>index%4!==3&&value>20),'decoded frame reaches the canvas');
      await capture('search_mv_native_frame');
      click(button('关闭视频'));
      click(button('返回上一页'));
      await wait(()=>ready()&&one('[data-category="mvs"]')?.getAttribute('aria-selected')==='true','return to MV results');
      await fixture('unavailable');await fresh('合成不可播放视频');await choose('mvs');
      const invalid=one('.search-video-card');
      assert(invalid?.querySelector('.search-card-open')?.disabled&&!invalid.querySelector('.search-video-play-glyph'),
        'opaque non-MV result stays disabled with no Play cue');
      await capture('search_non_mv_target_disabled');
      await emit('complete',{ok:true,searchMvVerified:true,stageCount:count,realMusicRequests:0,realAccountWrites:0});
      return;
    }
    if(window.__cumulusSearchVoiceQueueProof){
      await wait(()=>one('.track-row'),'daily ready');
      await fixture('voice_queue');
      const invoke=(name,args={})=>window.__TAURI_INTERNALS__.invoke(name,args);
      const {epoch}=await invoke('account_snapshot');
      const readQueue=()=>invoke('player_queue',{epoch});
      async function playing(match){
        for(let i=0;i<180;i++){
          const state=await invoke('player_snapshot',{epoch});
          if(match(state)&&state.state==='playing'&&state.position>.15)return state;
          await sleep(70);
        }
        throw Error('native playback did not advance');
      }
      await invoke('play_selection',{epoch,selection:{kind:'songs',ids:['1','2','3'],index:0,quality:'standard'}});
      await playing(state=>state.resourceKind==='catalog'&&state.track?.id==='1');
      const original=await readQueue();
      await fresh('合成声音队列');await choose('voices');
      const card=()=>one('[data-result-key="voice:401"]');
      const play=()=>card()?.querySelector('.search-card-play');
      assert(card()?.querySelector('.search-voice-radio'),'selected episode has a known parent radio');
      click(play());
      const voice=await playing(state=>state.track?.voiceId==='401');
      const queue=await readQueue();
      assert(voice.track.id==='1'&&voice.entryId!==original.tracks[0].entryId,'episode is distinct from catalog song with same media ID');
      assert(queue.tracks.map(row=>row.track.voiceId??row.track.id).join(',')==='1,401,2,3','single episode joins existing queue');
      assert(queue.tracks.filter(row=>!row.track.voiceId).map(row=>row.entryId).join(',')===
        original.tracks.map(row=>row.entryId).join(','),'catalog entries preserve identity and order');
      await capture('search_episode_joins_ordinary_queue');
      await wait(()=>play()?.getAttribute('aria-label').startsWith('暂停 '),'current voice card offers pause');
      click(play());
      await wait(()=>one('.playerbar').dataset.state==='paused','same episode pauses from the card');
      const paused=await invoke('player_snapshot',{epoch});
      assert(paused.playId===voice.playId&&paused.queueRevision===voice.queueRevision,'pause keeps decoder and queue');
      await capture('search_episode_pauses_in_place');
      await choose('podcasts');
      click(one('[data-result-key="podcast:301"] .search-card-play'));
      await playing(state=>state.track?.voiceId==='401'&&state.origin?.kind==='podcast');
      const radio=await readQueue();
      assert(radio.tracks.map(row=>row.track.voiceId).join(',')==='401,402,403','explicit podcast card still plays its episode list');
      await capture('search_podcast_keeps_explicit_radio_play');
      await emit('complete',{ok:true,voiceQueueVerified:true,stageCount:count,realMusicRequests:0,realAccountWrites:0});
      return;
    }
    if(window.__cumulusSearchSongWaterfallProof){
      await wait(()=>one('.track-row'),'daily ready');
      await emit('resize',{width:760,height:540});
      await wait(()=>innerWidth===760&&innerHeight===540,'compact window');
      await fixture('song_waterfall_error');await fresh('合成连续单曲');await choose('songs');
      const table=()=>one('.search-song-panel .track-table');
      const scroll=()=>table().querySelector('.track-scroll');
      const loaded=()=>Number(table()?.getAttribute('aria-rowcount'))-1;
      const scrollEnd=()=>{assert(scroll().scrollHeight>scroll().clientHeight,'table owns the scrollbar');scroll().scrollTop=scroll().scrollHeight;};
      assert(loaded()===10&&!one('.search-result-pagination'),'continuous first ten rows');
      scrollEnd();
      await wait(()=>button('重试加载更多'),'append error exposes retry');
      assert(loaded()===10&&scroll().scrollTop>0&&button('使用分页查看'),'first rows and position survive error');
      await capture('song_append_failure_retains_first_chunk');
      const anchor=scroll().scrollTop;
      click(button('使用分页查看'));
      await wait(()=>loaded()===30&&one('#search-song-batch-scope')?.textContent.trim()==='仅限当前页的单曲','explicit numbered fallback');
      await capture('song_numbered_fallback');
      click(button('返回上一页'));
      await wait(()=>loaded()===10&&button('重试加载更多')&&Math.abs(scroll().scrollTop-anchor)<=1,'Back restores the continuous view');
      await fixture('song_waterfall');
      click(button('重试加载更多'));
      await wait(()=>loaded()===20,'append retry retains both chunks');
      assert(Math.abs(scroll().scrollTop-anchor)<=1,'appending does not reset the viewport');
      scrollEnd();
      await wait(()=>loaded()===30,'second scroll appends the third chunk');
      assert(JSON.stringify(songReads)===JSON.stringify([[0,10],[10,10],[0,30],[10,10],[20,10]]),'only the expected cursor reads: '+JSON.stringify(songReads));
      const loadedPanel=table(), loadedTop=scroll().scrollTop, completedReads=songReads.length;
      click(one('[data-category="songs"]'));
      await sleep(400);
      assert(table()===loadedPanel&&loaded()===30&&songReads.length===completedReads&&
        Math.abs(scroll().scrollTop-loadedTop)<=1,'current songs tab retains loaded rows and scroll without another read');
      await capture('song_append_retains_thirty');
      const invoke=(name,args={})=>window.__TAURI_INTERNALS__.invoke(name,args);
      const {epoch}=await invoke('account_snapshot');
      async function playing(id){
        for(let index=0;index<160;index++){
          const state=await invoke('player_snapshot',{epoch});
          if(state.track?.id===id&&state.state==='playing'&&state.position>.1)return state;
          await sleep(70);
        }
        throw Error('native player did not advance on '+id);
      }
      click(button('播放全部'));
      await playing('101');
      const queue=await invoke('player_queue',{epoch});
      const expected=Array.from({length:30},(_,index)=>String(index+101));
      assert(queue.tracks.map(row=>row.track.id).join(',')===expected.join(','),'Play All sends the exact loaded prefix');
      await capture('song_all_loaded_prefix_playing');
      scrollEnd();
      await wait(()=>one('.track-row[data-track-id="130"] .track-play'),'deep virtual row available');
      click(one('.track-row[data-track-id="130"] .track-play'));
      const selected=await playing('130');
      const deep=await invoke('player_queue',{epoch});
      assert(selected.entryId===queue.tracks[29].entryId&&
        deep.tracks.map(row=>row.entryId).sort().join(',')===queue.tracks.map(row=>row.entryId).sort().join(','),'deep row keeps all active queue entries');
      await capture('song_deep_result_keeps_queue');
      click(button('返回上一页'));
      await wait(()=>one('[data-category="all"]')?.getAttribute('aria-selected')==='true'&&ready(),'reselecting current songs did not add a Back step');

      await emit('complete',{ok:true,songWaterfallVerified:true,stageCount:count,songReads,realMusicRequests:0,realAccountWrites:0});
      return;
    }
    if(window.__cumulusSearchNextProof){
      await wait(()=>one('.track-row'),'daily ready');
      await fixture('good');
      const invoke=(name,args={})=>window.__TAURI_INTERNALS__.invoke(name,args);
      const {epoch}=await invoke('account_snapshot');
      const player=()=>invoke('player_snapshot',{epoch});
      const queue=()=>invoke('player_queue',{epoch});
      async function playing(id){
        for(let index=0;index<160;index++){
          const state=await player();
          if(state.track?.id===id&&state.state==='playing'&&state.position>.1)return state;
          await sleep(70);
        }
        throw Error('native player did not advance on '+id);
      }
      await invoke('play_selection',{epoch,selection:{kind:'songs',ids:['91','92','3'],index:0,quality:'standard'}});
      const original=await playing('91'), before=await queue();
      await fresh('合成下一首');await choose('songs');
      await fixture('result_play_trial');
      click(one('.track-row[data-track-id="3"] .track-more'));
      await wait(()=>one('.song-action-menu'),'search song menu');
      click(button('下一首播放',one('.song-action-menu')));
      await wait(()=>one('.track-action-feedback')?.textContent.trim()==='已设为下一首播放','queued search result');
      const inserted=await queue(), active=await player();
      assert(inserted.tracks.map(row=>row.track.id).join(',')==='91,3,92','existing result moves without duplicate');
      assert(inserted.tracks.map(row=>row.entryId).join(',')===
        [before.tracks[0],before.tracks[2],before.tracks[1]].map(row=>row.entryId).join(','),'preserved entry identities');
      assert(active.playId===original.playId&&active.state==='playing','insertion leaves native playback running');
      await capture('search_result_queued_without_interrupting');
      click(button('下一首',one('.playerbar')));
      const advanced=await playing('3');
      assert(advanced.entryId===before.tracks[2].entryId&&advanced.playId!==original.playId,'advanced to the reordered entry');
      await capture('search_next_uses_search_authorization');
      await emit('complete',{ok:true,searchNextVerified:true,stageCount:count,realMusicRequests:0,realAccountWrites:0});
      return;
    }
    if(window.__cumulusSearchPeopleCategoriesProof){
      await wait(()=>one('.track-row'),'daily ready');await fixture('good');await fresh('1');
      const card=kind=>one(`[data-result-key^="${kind}:"]`);
      assert(card('artist').textContent.includes('合成别名')&&!card('artist').querySelector('mark')&&
        !card('user').querySelector('mark'),'comprehensive keeps its separate people presentation');
      for(const [kind,category,subtitle] of [['artist','artists','专辑：12'],
        ['user','users','愿每一首歌，都能留下属于此刻的记忆。']]){
        await choose(category);
        const row=card(kind), title=row.querySelector('.search-result-name'), sub=row.querySelector('.search-result-sub');
        assert(title.querySelector('mark')?.textContent==='1'&&title.textContent.trim()===title.dataset.tooltip,
          'category marks the displayed name without changing it: '+kind);
        assert(sub?.textContent.trim()===subtitle&&!sub.querySelector('mark'),
          'category keeps official secondary text plain: '+kind);
        assert(!row.querySelector('.search-card-open').disabled,'catalog action remains available');
        await capture(kind+'_category_presentation');
      }
      await emit('complete',{ok:true,peopleCategoriesVerified:true,stageCount:count,realMusicRequests:0,realAccountWrites:0});
      return;
    }
    if(window.__cumulusSearchOriginalCorrectionProof){
      await wait(()=>one('.track-row'),'daily ready');await fixture('correction_parts');
      const provides=one('#app').__vue_app__._context.provides;
      const core=provides[Reflect.ownKeys(provides).find(k=>typeof k==='symbol'&&k.description==='core-client')];
      const discovery=core.searchDiscovery;
      const input=one('[aria-label="搜索音乐"]');
      const label='显示标签 · 合成错词';
      core.searchDiscovery=async(...args)=>{
        const result=await discovery(...args);
        if(args[1].kind!=='suggestions')return result;
        return {...result,value:{...result.value,suggestions:[{keyword:'合成错词',label,tag:null}]}};
      };
      try{
        input.focus();input.click();input.value='显示';input.dispatchEvent(new Event('input',{bubbles:true}));
        await wait(()=>one('.search-suggestion[title="'+label+'"]'),'synthetic display label suggestion');
        click(one('.search-suggestion[title="'+label+'"]'));
        await wait(()=>ready()&&one('.search-correction')?.textContent.includes('合成正词'),'corrected results');
        assert(input.value===label,'suggestion display text differs from canonical query');
        const fragments=one('.search-heading-copy .search-correction-fragments');
        assert(fragments&&[...fragments.children].map(node=>node.textContent.trim()).join('|')===
          '正在显示合成正词。<b>仍查</b>|合成错词|。','native correction fragments retain their order and literal text');
        assert(!fragments.querySelector('b')&&fragments.querySelectorAll('button').length===1&&
          fragments.children[1].tagName==='BUTTON'&&one('.search-heading-copy h1').classList.contains('sr-only'),
          'only the original word is interactive; plain text replaces the visible heading');
        await capture('correction_suggestion_label');
      }finally{core.searchDiscovery=discovery;}
      const {epoch}=await core.accountSnapshot();
      await core.searchHistory(epoch,{kind:'clear'});
      assert(!(await core.searchHistory(epoch,{kind:'read'})).value.length,'isolated native history empty');
      const original=button('仍然搜索“合成错词”');
      assert(original?.tabIndex===0,'original query action is in the keyboard tab order');
      click(original);original.click();
      await wait(()=>ready()&&!one('.search-correction')&&one('.search-result-panel').textContent.includes('按原词搜索结果'),
        'original results with native correction disabled');
      assert(input.value==='合成错词','input switches to the exact original term');
      let remembered=false;
      for(let i=0;i<80;i++){
        if((await core.searchHistory(epoch,{kind:'read'})).value[0]==='合成错词'){remembered=true;break;}
        await sleep(50);
      }
      assert(remembered,'native history remembers the explicit original search');
      await capture('correction_original_remembered');
      click(button('返回上一页'));
      await wait(()=>ready()&&one('.search-correction')?.textContent.includes('合成正词')&&input.value===label,
        'Back restores prior corrected view and its display label');
      await emit('complete',{ok:true,originalCorrectionVerified:true,correctionPartsVerified:true,stageCount:count,realMusicRequests:0,realAccountWrites:0});
      return;
    }
    if(window.__cumulusSearchCardMatchesProof){
      await wait(()=>one('.track-row'),'daily ready');
      await fixture('good');await fresh('合成');
      const check=(kind, expected)=>{
        const title=one(`[data-result-key^="${kind}:"] .search-result-name`);
        const text=title?.textContent.trim();
        assert(text&&title.dataset.tooltip===text,'title and tooltip keep identical text: '+kind);
        const marks=[...title.querySelectorAll('mark.search-match-text')];
        assert(marks.length===(expected?1:0)&&(!expected||marks[0].textContent==='合成'),
          'exact card title mark: '+kind);
        const subtitle=title.closest('.search-result-card').querySelector('.search-result-sub');
        const recommendation=subtitle?.querySelector('.search-playlist-recommendation');
        const comprehensive=one('[data-category="all"]')?.getAttribute('aria-selected')==='true';
        if(kind==='playlist'&&comprehensive){
          assert(recommendation?.textContent.includes('合成 推荐理由')&&!recommendation.querySelector('em'),
            'comprehensive playlist shows the bounded plain recommendation');
          const reasonMark=recommendation.querySelector('mark');
          assert(expected?reasonMark?.textContent==='合成':!reasonMark,'only matched recommendation text is marked');
        } else assert(!recommendation&&!subtitle?.querySelector('mark'),'other subtitles remain plain: '+kind);
      };
      for(const kind of ['playlist','podcast','voice','mv'])check(kind,true);
      const playlistCard=()=>one('[data-result-key^="playlist:"]');
      const playlistBadge=playlistCard()?.querySelector('.search-playlist-count');
      const description=playlistCard()?.querySelector('.search-card-open')?.getAttribute('aria-describedby');
      assert(playlistBadge?.textContent.includes('万')&&document.getElementById(description)?.textContent==='播放次数：12,345',
        'comprehensive shows compact count with exact accessible count');
      assert(!all('[data-result-key^="playlist:"]')[1]?.querySelector('.search-playlist-count'),'missing count shows no badge');
      const voiceFooter=()=>one('[data-result-key^="voice:"] .search-voice-footer');
      const checkVoiceListens=()=>{
        const footer=voiceFooter();
        assert(footer?.querySelector('.search-voice-listens')?.textContent.includes('播放：1.2万')&&
          footer.querySelector('.search-voice-duration')&&footer.querySelector('button.search-voice-radio'),
          'voice footer retains duration, listen count and parent link');
      };
      checkVoiceListens();
      await capture('comprehensive_card_title_matches');
      for(const [category,kind] of [['playlists','playlist'],['podcasts','podcast'],['voices','voice'],['mvs','mv']]){
        await choose(category);check(kind,true);
        if(kind==='playlist')assert(!playlistCard()?.querySelector('.search-playlist-count'),'dedicated playlist results retain their layout');
        if(kind==='voice')checkVoiceListens();
      }
      await capture('mv_category_card_title_matches');
      await fresh('不匹配结果');
      for(const kind of ['playlist','podcast','voice','mv'])check(kind,false);
      await emit('complete',{ok:true,cardTitleMatchesVerified:true,stageCount:count,realMusicRequests:0,realAccountWrites:0});
      return;
    }
    if(window.__cumulusSearchVoiceDirectProof){
      await wait(()=>one('.track-row'),'daily ready');
      await fixture('unavailable');await fresh('合成无所属播客的声音');await choose('voices');
      const card=one('[data-result-key="voice:401"]');
      const play=card?.querySelector('.search-card-play');
      assert(play&&!card.querySelector('.search-voice-radio'),'episode play is independent of the parent radio link');
      const invoke=(name,args={})=>window.__TAURI_INTERNALS__.invoke(name,args);
      const {epoch}=await invoke('account_snapshot');
      click(play);
      let first;
      for(let i=0;i<200;i++){
        const view=await invoke('player_snapshot',{epoch});
        if(view.track?.voiceId==='401'&&view.state==='playing'&&view.position>.2){first=view;break;}
        await sleep(70);
      }
      assert(first,'standalone episode reached actual native playback');
      await capture('voice_without_radio_playing');
      const oldTicket=await invoke('prepare_play_selection',{
        epoch,expectedQueueRevision:first.queueRevision,expectedPlayId:first.playId,
      });
      click(card.querySelector('.search-card-play'));
      await wait(()=>one('.playerbar').dataset.state==='paused','same episode pauses from the search card');
      const paused=await invoke('player_snapshot',{epoch});
      assert(paused.playId===first.playId&&paused.queueLen===1,'same episode and single-entry native queue');
      assert(paused.queueRevision===first.queueRevision,'pause leaves queue identity unchanged');
      const stale=await invoke('commit_prepared_selection',{
        epoch,ticket:oldTicket,selection:{kind:'songs',ids:['2'],index:0,quality:'standard'},
      }).then(()=>null,error=>error);
      assert(stale?.code==='stale','native pause invalidates the old ticket even without a queue or play ID change');
      const retained=await invoke('player_snapshot',{epoch});
      assert(retained.playId===paused.playId&&retained.queueRevision===paused.queueRevision&&retained.state==='paused',
        'stale commit does not replace the paused episode');
      await capture('voice_without_radio_paused');
      assert(retained.resourceKind==='voice'&&retained.track?.id==='1',
        'fixture episode uses the same backing media ID as catalog song 1');
      await choose('all');
      const song=one('[data-result-key="song:1"] .search-track-cover');
      assert(song&&!song.closest('.search-track-card').classList.contains('current')&&
        song.getAttribute('aria-label').startsWith('播放 '),'episode is not the current catalog song');
      click(song);
      let catalog;
      for(let i=0;i<180;i++){
        const view=await invoke('player_snapshot',{epoch});
        if(view.resourceKind==='catalog'&&view.track?.id==='1'&&view.state==='playing'){
          catalog=view;break;
        }
        await sleep(70);
      }
      assert(catalog&&catalog.playId!==paused.playId,'catalog click replaces the colliding episode');
      await wait(()=>song.getAttribute('aria-label').startsWith('暂停 '),'matching catalog song offers pause');
      click(song);
      await wait(()=>one('.playerbar').dataset.state==='paused','same catalog song pauses');
      const catalogPaused=await invoke('player_snapshot',{epoch});
      assert(catalogPaused.playId===catalog.playId&&catalogPaused.queueRevision===catalog.queueRevision,
        'same catalog song pauses without replacing its entry');
      await capture('catalog_song_not_episode');
      await emit('complete',{ok:true,voiceDirectVerified:true,preparedTicketFenceVerified:true,
        searchSongNamespaceVerified:true,stageCount:count,realMusicRequests:0,realAccountWrites:0});
      return;
    }
    if(window.__cumulusSearchV3Proof){
      await wait(()=>one('.track-row'),'daily ready');
      await fresh('合成 V3');
      const tabs=()=>all('[data-category][role="tab"]').map(node=>node.dataset.category).join(',');
      await wait(()=>tabs()==='all,mvs,songs'&&one('#search-result-panel').textContent.includes('综合 V3 合成歌曲'),'v3 tab order and projected song');
      assert(one('#search-result-panel').textContent.includes('合成热点内容'),'hot explanation');
      assert(!document.querySelector('[href*="example.invalid/private"]'),'no raw action');
      await capture('v3_comprehensive');
      key(one('[data-category="all"]'),'ArrowRight');
      await wait(()=>ready()&&one('[data-category="mvs"]')?.getAttribute('aria-selected')==='true','dynamic arrow navigation');
      assert(tabs()==='all,mvs,songs','category preserves service order');
      await capture('v3_category');
      click(button('返回上一页'));
      await wait(()=>ready()&&one('[data-category="all"]')?.getAttribute('aria-selected')==='true','Back restores comprehensive');
      assert(tabs()==='all,mvs,songs'&&one('#search-result-panel').textContent.includes('合成热点内容'),'Back preserves search snapshot');
      await capture('v3_back');
      await emit('complete',{ok:true,v3Verified:true,stageCount:count,realMusicRequests:0,realAccountWrites:0});
      return;
    }
    await wait(()=>one('.track-row'),'daily ready');submit('合成搜索结果');await wait(()=>one('.search-results-page')&&ready(),'search entry');
    for(const [w,h,t] of [[1120,760,'light'],[1120,760,'dark'],[760,540,'light'],[760,540,'dark']]){
      await theme(w,h,t);await fresh('合成搜索结果');
      for(const category of ['all','songs','playlists','artists','voices','podcasts','lyrics','albums','mvs','users']){
        await choose(category);await capture(`${category}_${w}_${t}`);
        if(category==='all'){const p=one('.search-result-panel');p.scrollTop=p.scrollHeight;await capture(`all_bottom_${w}_${t}`);p.scrollTop=0;
          if(w===1120&&t==='light'){
            const source=button('查看用户主页 合成用户 1');
            assert(source,'comprehensive user card exposes the same route');
            await userProfile(source,true);
          }
        }
        if(category==='songs'){
          assert(button('播放全部')&&button('收藏全部'),'official actions');click(button('收藏全部'));await wait(()=>one('.playlist-dialog[open]'),'collection dialog');assert(one('.playlist-collect-dialog .sr-only')?.textContent.includes('10首'),'collection is explicit about loaded songs');await capture(`collect_${w}_${t}`);click(button('关闭歌单操作'));await wait(()=>!one('.playlist-dialog'),'collect close');
        }
        if(category==='users'){
          const source=button('查看用户主页 合成用户 1');
          const unavailable=button('查看用户主页 合成用户 3');
          assert(unavailable?.disabled&&unavailable.closest('[data-result-key]').textContent.includes('此账号当前不可用'),'unavailable user stays disabled');
          await userProfile(source);
          await capture(`user_details_${w}_${t}`);
          click(button('返回上一页'));
          await wait(()=>ready()&&one('[data-category="users"]')?.getAttribute('aria-selected')==='true'&&document.activeElement===button('查看用户主页 合成用户 1'),'user category Back restores source focus');
        }
      }
    }
    await fixture('short_pages');await fresh('合成短分页');await choose('playlists');
    const onPage=(number,offset)=>ready()&&one('.search-result-pagination')?.textContent.includes(`第 ${number} 页`)&&one('.search-result-card')?.textContent.includes(`合成分页 ${offset}`);
    assert(onPage(1,0),'first short page');
    click(button('下一页'));await wait(()=>onPage(2,18),'second short page');
    assert(one('.search-result-panel').textContent.includes('有 1 项结果暂无法展示'),'short page keeps valid rows and counts malformed records');
    const numberedPanel=one('#search-result-panel');
    click(one('[data-category="playlists"]'));await sleep(400);
    assert(onPage(2,18)&&one('#search-result-panel')===numberedPanel,'current playlist tab retains the second page');
    await capture('playlists_page_two');
    click(button('下一页'));await wait(()=>onPage(3,29),'third short page consumes malformed record');
    assert(button('下一页').disabled,'last short page cannot advance');
    click(button('上一页'));await wait(()=>onPage(2,18),'previous returns to actual offset 18');
    click(button('返回上一页'));await wait(()=>onPage(3,29),'app back restores third page');
    click(button('上一页'));await wait(()=>onPage(2,18),'previous still available after app back');
    click(button('上一页'));await wait(()=>onPage(1,0),'first page restored');
    await fixture('mixed_results');await fresh('合成混合结果');
    const partial=()=>one('.search-result-panel').textContent.includes('有 1 项结果暂无法展示');
    assert(partial()&&one('.search-track-card')&&!button('重新加载'),'comprehensive mixed results preserve valid cards');
    await choose('songs');assert(partial()&&one('.track-row')&&!button('重新加载'),'category mixed results preserve valid tracks');
    await fixture('highlight');await fresh('A+B');
    const matches=root=>[...root.querySelectorAll('mark.search-match-text')].map(n=>n.textContent).join('|');
    const preview=one('.search-track-card');
    assert(matches(preview)==='A+B|a+b|A+B'&&preview.textContent.includes('<plain>')&&!preview.querySelector('plain'),'literal comprehensive title and artist marks');
    await choose('songs');assert(matches(one('.track-row'))==='A+B|a+b|A+B|a+b','song title, artist and album marks');
    await choose('lyrics');
    assert(matches(one('.search-lyric-result'))==='A+B|a+b|A+B|a+b'&&
      one('.search-lyric-lines')?.textContent.includes('A+B 原歌词')&&!one('.search-lyric-lines mark'),
      'lyric row labels marked while lyric body stays plain');
    await fixture('correction');await fresh('合成错词');
    assert(one('.search-correction')?.textContent.includes('合成正词'),'comprehensive shows rewritten term');
    assert(matches(one('.search-track-card'))==='合成错词','comprehensive highlights original term');
    await capture('correction_all');
    await choose('songs');assert(one('.track-row')&&one('.search-correction')?.textContent.includes('合成正词'),'songs use rewritten term and retain original action');
    assert(matches(one('.track-row'))==='合成正词','song category highlights rewritten term');
    await choose('playlists');click(button('下一页'));await wait(()=>ready()&&one('.search-result-card')&&one('.search-result-pagination')?.textContent.includes('第 2 页'),'corrected term carries to next page');
    await capture('correction_pagination');
    click(button('仍然搜索“合成错词”'));await wait(()=>ready()&&one('[data-category="all"]').getAttribute('aria-selected')==='true'&&!one('.search-correction')&&one('.search-result-panel').textContent.includes('按原词搜索结果'),'original term sent with correction disabled');
    await capture('correction_disabled');
    click(button('返回上一页'));await wait(()=>ready()&&one('[data-category="playlists"]').getAttribute('aria-selected')==='true'&&one('.search-correction')?.textContent.includes('合成正词'),'back restores corrected route');
    await fixture('empty');await fresh('合成空结果');await choose('lyrics');await capture('empty');assert(one('.search-result-panel').textContent.includes('没有找到'),'empty copy');
    await fixture('result_error');await fresh('合成失败结果');await choose('albums');await capture('error');
    const retryPanel=one('#search-result-panel');
    click(button('重新加载'));
    await wait(()=>button('重新加载')&&document.activeElement===button('重新加载'),'a repeated failure restores retry focus');
    await fixture('good');click(button('重新加载'));
    await wait(()=>ready()&&one('.search-result-card'),'retry recovery');
    assert(document.activeElement===retryPanel,'successful retry keeps focus in the results panel');
    await capture('recovered');
    hold('all');submit('合成延迟结果');await wait(()=>held,'held response');await capture('loading');await choose('songs');release();await sleep(220);assert(one('[data-category="songs"]').getAttribute('aria-selected')==='true'&&one('.search-result-actions'),'late result ignored');await capture('late_result');
    step='keyword_race';
    const oldKeyword='合成迟到关键词', newKeyword='合成新关键词';
    hold('all',oldKeyword);submit(oldKeyword);await wait(()=>held,'hold old keyword with populated response');
    await fixture('empty');await fresh(newKeyword);
    await wait(()=>one('#search-result-panel .empty-state')&&!one('.search-result-section'),'new keyword is empty');
    release();await sleep(350);
    assert(one('.search-results-page h1')?.textContent===newKeyword&&one('#search-result-panel .empty-state')&&!one('.search-result-section'),
      'old keyword cannot replace newer results in the same category');
    await fixture('good');await fresh('合成延迟结果');await choose('songs');
    step='category_without_frames';
    const songTab=one('[data-category="songs"]');songTab.focus();
    const motion=window.__motionProof;
    assert(motion,'isolated motion fixture');
    const systemReduced=window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    const frames=window.requestAnimationFrame;
    const withheld=new Set();
    let frameCount=0;
    try {
      // Some runners disable animations by default. Exercise the actual
      // animated branch deliberately, without changing the saved preference.
      motion.system(false);
      assert(document.documentElement.dataset.motion==='full','full motion fixture applied');
      window.requestAnimationFrame=()=>{
        for(const phase of ['leave','enter'])
          if(one(`.search-results-${phase}-active`))withheld.add(phase);
        return -(++frameCount);
      };
      key(songTab,'ArrowRight');
      await wait(()=>{
        const panel=one('.search-result-panel');
        return panel?.getAttribute('aria-labelledby')==='search-tab-playlists'&&ready()&&
          !one('.search-results-enter-active,.search-results-leave-active')&&
          getComputedStyle(panel).opacity==='1';
      },'keyboard tab switch with compositor frames withheld');
      assert(withheld.has('leave')&&withheld.has('enter'),`withheld category transition phases: ${[...withheld]}`);
    } finally {
      window.requestAnimationFrame=frames;
      motion.system(systemReduced);
    }
    assert(document.activeElement.dataset.category==='playlists','keyboard focus follows');await capture('keyboard_tabs');
    await choose('songs');
    const album=one('.track-album[data-tooltip]');assert(album&&!album.hasAttribute('title'),'native title replaced');album.dispatchEvent(new PointerEvent('pointerover',{bubbles:true}));await wait(()=>one('.content-tooltip')&&getComputedStyle(one('.content-tooltip')).visibility==='visible','content tooltip');await capture('content_tooltip');
    await choose('artists');assert(!one('.content-tooltip'),'route clears tooltip');await capture('tooltip_cleared');
    await fixture('unavailable');await fresh('合成不可用结果');await choose('voices');assert(one('.search-result-card .search-card-play')&&!one('.search-voice-radio'),'missing radio does not disable episode playback');await capture('voice_unavailable');await choose('mvs');assert(one('.search-card-open').disabled,'non-MV target disabled');await capture('mv_unavailable');await fixture('good');
    await choose('all');
    const playingTrack=id=>one('.playerbar')?.dataset.state==='playing'&&one('.now-playing')?.textContent.includes('合成歌曲 '+id+' ·');
    await fixture('result_play_default');click(button('播放全部搜索单曲'));
    await wait(()=>playingTrack(1),'comprehensive header play uses default trial');
    await fixture('result_play_trial');click(all('.search-track-cover')[1]);
    await wait(()=>playingTrack(2),'individual preview uses search trial');
    await choose('songs');click(button('播放全部'));
    await wait(()=>playingTrack(1),'song tab play all uses search trial');await capture('play');
    await choose('lyrics');click(one('.track-row[data-track-id="3"] .track-play'));
    await wait(()=>playingTrack(3),'lyric row uses search trial');
    await choose('songs');click(one('.track-row[data-track-id="2"] .track-play'));
    await wait(()=>playingTrack(2),'song row uses search trial');
    await fresh('合成搜索受限');
    await wait(()=>one('#search-result-panel .empty-state p')?.textContent==='部分结果暂不可展示'&&!button('重新加载'),'service explanation without generic retry');
    await capture('service_empty');
    await fresh('合成搜索结果');
    await wait(()=>one('.search-result-section')&&!one('#search-result-panel')?.textContent.includes('部分结果暂不可展示'),'next search clears explanation');
    await fixture('expired');click(one('[data-category="albums"]'));await wait(()=>button('重新登录')&&!one('.search-results-page'),'expired account');await capture('expired');
    await emit('complete',{ok:true,stageCount:count,responseTimingInjected:true,shortPageOffsetsVerified:true,searchResultPlaybackVerified:true,searchMatchHighlightVerified:true,searchServiceEmptyVerified:true,searchCategoryWithoutFramesVerified:true,searchKeywordRaceVerified:true,searchProfileWithoutFramesVerified:profileWithoutFrames,realMusicRequests:0,realAccountWrites:0});
  }catch(error){await emit('failed',{step,message:String(error),stack:error?.stack,stageCount:count,body:document.body.innerText.slice(0,3500)});}
})();
