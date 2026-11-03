// Real Tauri/WebView and production IPC; synthetic read responses, no account writes.
(async()=>{
  const one=s=>document.querySelector(s), all=s=>[...document.querySelectorAll(s)], sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const assert=(v,m)=>{if(!v)throw Error(m);};
  const wait=async(f,m)=>{for(let i=0;i<200;i++){if(f())return;await sleep(70);}throw Error(m);};
  const button=(label,root=document)=>[...root.querySelectorAll('button')].find(n=>n.getAttribute('aria-label')===label||n.textContent.trim()===label);
  const click=n=>{assert(n&&!n.disabled,'missing/disabled control '+n?.textContent);n.focus({preventScroll:true});n.click();};
  const emit=(stage,data={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'ux-proof',payload:{stage,...data}});
  const key=(n,k)=>n.dispatchEvent(new KeyboardEvent('keydown',{key:k,bubbles:true,cancelable:true}));
  let count=0,step='startup',holdCategory,held=false,release,gate;
  const fetch=window.fetch;
  window.fetch=async function(url,options){const query=String(url).endsWith('/search_results')?JSON.parse(options.body).query:null;const delayed=query?.category===holdCategory?gate:null;const response=await fetch.call(window,url,options);if(delayed){held=true;await delayed;}return response;};
  const hold=category=>{held=false;holdCategory=category;gate=new Promise(r=>{const timer=setTimeout(r,10000);release=()=>{clearTimeout(timer);holdCategory=undefined;gate=undefined;r();};});};
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
          assert(button('播放全部')&&button('收藏全部'),'official actions');click(button('收藏全部'));await wait(()=>one('.playlist-dialog[open]'),'collection dialog');assert(one('.playlist-collect-dialog .sr-only')?.textContent.includes('30首'),'collection is explicit about current page');await capture(`collect_${w}_${t}`);click(button('关闭歌单操作'));await wait(()=>!one('.playlist-dialog'),'collect close');
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
    await fixture('result_error');await fresh('合成失败结果');await choose('albums');await capture('error');assert(button('重新加载'),'retry action');await fixture('good');click(button('重新加载'));await wait(()=>ready()&&one('.search-result-card'),'retry recovery');await capture('recovered');
    hold('all');submit('合成延迟结果');await wait(()=>held,'held response');await capture('loading');await choose('songs');release();await sleep(220);assert(one('[data-category="songs"]').getAttribute('aria-selected')==='true'&&one('.search-result-actions'),'late result ignored');await capture('late_result');
    const songTab=one('[data-category="songs"]');songTab.focus();
    const frames=window.requestAnimationFrame;let withheldFrames=0;
    try {
      window.requestAnimationFrame=()=>-(++withheldFrames);
      key(songTab,'ArrowRight');
      await wait(()=>{
        const panel=one('.search-result-panel');
        return panel?.getAttribute('aria-labelledby')==='search-tab-playlists'&&ready()&&
          !one('.search-results-enter-active,.search-results-leave-active')&&
          getComputedStyle(panel).opacity==='1';
      },'keyboard tab switch with compositor frames withheld');
    } finally { window.requestAnimationFrame=frames; }
    assert(withheldFrames>=2,'both transitions encountered withheld frames');
    assert(document.activeElement.dataset.category==='playlists','keyboard focus follows');await capture('keyboard_tabs');
    await choose('songs');
    const album=one('.track-album[data-tooltip]');assert(album&&!album.hasAttribute('title'),'native title replaced');album.dispatchEvent(new PointerEvent('pointerover',{bubbles:true}));await wait(()=>one('.content-tooltip')&&getComputedStyle(one('.content-tooltip')).visibility==='visible','content tooltip');await capture('content_tooltip');
    await choose('artists');assert(!one('.content-tooltip'),'route clears tooltip');await capture('tooltip_cleared');
    await fixture('unavailable');await fresh('合成不可用结果');await choose('voices');assert(!one('.search-result-card .search-card-play')&&one('.search-result-card').textContent.includes('暂不可打开'),'missing radio target disabled');await capture('voice_unavailable');await choose('mvs');assert(one('.search-card-open').disabled,'non-MV target disabled');await capture('mv_unavailable');await fixture('good');
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
    await fixture('expired');click(one('[data-category="albums"]'));await wait(()=>button('重新登录')&&!one('.search-results-page'),'expired account');await capture('expired');
    await emit('complete',{ok:true,stageCount:count,responseTimingInjected:true,shortPageOffsetsVerified:true,searchResultPlaybackVerified:true,searchMatchHighlightVerified:true,searchCategoryWithoutFramesVerified:true,searchProfileWithoutFramesVerified:profileWithoutFrames,realMusicRequests:0,realAccountWrites:0});
  }catch(error){await emit('failed',{step,message:String(error),stack:error?.stack,stageCount:count,body:document.body.innerText.slice(0,3500)});}
})();
