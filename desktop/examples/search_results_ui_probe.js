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
  async function capture(label,extra={}){
    step=label;await wait(()=>all('.search-result-panel,dialog').every(n=>n.getAnimations().every(a=>a.playState!=='running'&&!a.pending)),'settled animation');await sleep(160);
    const panel=one('.search-result-panel');
    const info={label,theme:document.documentElement.dataset.theme,width:innerWidth,height:innerHeight,noOverflow:document.documentElement.scrollWidth<=innerWidth,panelNoOverflow:!panel||panel.scrollWidth<=panel.clientWidth+1,cardCount:all('.search-result-card').length,renderedTracks:all('.track-row').length};
    assert(info.noOverflow&&info.panelNoOverflow,JSON.stringify(info));assert(info.cardCount<150&&info.renderedTracks<80,'bounded result DOM');
    if(one('[data-category="all"]')?.getAttribute('aria-selected')==='true') { assert(!one('.search-result-panel .track-table'),'comprehensive songs use cards'); assert(all('.search-track-card').length<=6,'bounded song previews');if(one('.search-result-tracks'))assert(one('.search-track-card'),'ready song section has previews'); }
    const first=one('.search-result-name'),category=one('[data-category][aria-selected="true"]')?.dataset.category;if(first&&category!=='all'&&panel.scrollTop===0&&!one('dialog[open]')) assert(first.getBoundingClientRect().bottom<=one('.playerbar').getBoundingClientRect().top,'first caption above player');
    for(const n of all('.search-result-actions button')){const r=n.getBoundingClientRect();assert(Math.abs(r.width-94)<1&&Math.abs(r.height-40)<1,'official 94x40 result action');}
    await emit(`ux_${String(++count).padStart(2,'0')}_${label}`,{...info,...extra});await sleep(500);
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
    if(window.__cumulusSearchHotCardProof){
      await wait(()=>one('.track-row'),'daily ready');
      await theme(1120,760,'light');
      await fresh('合成热点链接');
      const card=()=>one('.hot-explanation');
      const action=()=>card()?.querySelector('.hot-action');
      assert(card()?.querySelector('h2')?.textContent==='合成热点链接'&&card().querySelector('.hot-marker')?.textContent==='热点'&&card().querySelector('.hot-value')?.textContent.trim()==='25.3万','card displays title, corner marker and official heat');
      assert(action()?.textContent.trim()==='查看相关歌单'&&!one('#search-result-panel .empty-state'),'hot-only result is actionable without an empty result');
      await capture('hot_explanation_action');
      click(action());
      await wait(()=>one('.playlist-hero h1')?.textContent.includes('合成歌单 28'),'explicit typed target takes priority over related playlist');
      await capture('hot_explanation_target');
      click(button('返回上一页'));
      await wait(()=>ready()&&action()===document.activeElement,'Back restores hotspot action focus');
      await capture('hot_explanation_back');
      await theme(760,540,'dark');
      await fresh('合成热点资源');
      const cardBounds=card().getBoundingClientRect(), actionBounds=action().getBoundingClientRect();
      assert(actionBounds.left>cardBounds.left&&actionBounds.right<cardBounds.right&&actionBounds.bottom<=cardBounds.bottom,'narrow dark target is contained');
      await capture('hot_explanation_related_narrow');
      click(action());
      await wait(()=>one('.playlist-hero h1')?.textContent.includes('合成歌单 29'),'first related resource is the typed fallback');
      click(button('返回上一页'));
      await wait(()=>ready()&&action()===document.activeElement,'related target restores action focus');
      await fresh('合成热点外链');
      assert(card()?.querySelector('h2')?.textContent==='合成热点外链'&&!action()&&!card().querySelector('a[href]')&&!card().outerHTML.includes('PRIVATE'),'unsupported explicit URL stays inert rather than substituting the related action');
      await capture('hot_explanation_external_inert');
      await emit('complete',{ok:true,hotCardVerified:true,stageCount:count,realAccountWrites:0});
      return;
    }
    if(window.__cumulusSearchCarouselProof){
      await wait(()=>one('.track-row'),'daily ready');
      await theme(1120,760,'light');
      await fresh('合成轮播');
      const best=()=>one('.search-best-section');
      const third=()=>best()?.querySelector('[data-result-key="playlist:29"] .search-card-open');
      const keys=[...best().querySelectorAll('[data-result-key]')].map(node=>node.dataset.resultKey);
      assert(keys.join(',')==='artist:102,playlist:28,playlist:29'&&best().classList.contains('search-best-compact'),
        'expanded best match retains the first three supported results in order');
      assert(getComputedStyle(best().querySelector('.search-result-grid')).gridTemplateColumns.split(' ').length===3&&
        getComputedStyle(best().querySelector('.search-result-name')).fontSize==='14px'&&
        getComputedStyle(best().querySelector('.search-result-sub')).fontSize==='12px','three desktop cards use compact official styling');
      await capture('best_match_expanded');
      click(third());
      await wait(()=>one('.playlist-hero h1')?.textContent.includes('合成歌单 29'),'third best match opens its real playlist');
      click(button('返回上一页'));
      await wait(()=>third()===document.activeElement,'Back restores the third best-match card instead of its duplicate carousel entry');
      const carousel=()=>one('.search-carousel[aria-label="歌单预览"]');
      const artist=()=>one('.search-carousel[aria-label="歌手预览"]');
      const current=()=>Number(carousel()?.dataset.previewStart);
      const columns=()=>Number(carousel()?.dataset.previewColumns);
      const visible=()=>[...carousel().querySelectorAll('[data-grid-index]:not([inert])')].map(node=>Number(node.dataset.gridIndex));
      const next=()=>button('歌单预览下一页',carousel());
      const previous=()=>button('歌单预览上一页',carousel());
      const reveal=()=>carousel().scrollIntoView({block:'center'});
      const settled=()=>carousel().querySelector('.search-carousel-rail').getAnimations().every(a=>a.playState!=='running'&&!a.pending);
      assert(carousel()?.querySelectorAll('[data-grid-index]').length===10&&artist()&&columns()===6,'dense carousel keeps at most ten rows and uses official columns');
      assert(current()===0&&previous().getAttribute('aria-disabled')==='true'&&visible().join(',')==='0,1,2,3,4,5','first carousel page is bounded');
      reveal();
      await capture('carousel_first');
      click(next());
      await wait(()=>current()===4&&settled(),'last page aligns the partial tail');
      assert(visible().join(',')==='4,5,6,7,8,9'&&artist().dataset.previewStart==='0','adjacent carousel is independent');
      const offscreen=carousel().querySelector('[data-grid-index="0"] .search-card-open');
      offscreen.focus({preventScroll:true});
      assert(document.activeElement===next()&&next().getAttribute('aria-disabled')==='true','clipped controls are inert and end button retains focus');
      const firstVisible=carousel().querySelector('[data-grid-index="4"]').getBoundingClientRect();
      const finalVisible=carousel().querySelector('[data-grid-index="9"]').getBoundingClientRect();
      const viewport=carousel().querySelector('.search-carousel-viewport').getBoundingClientRect();
      assert(Math.abs(firstVisible.left-viewport.left)<1&&Math.abs(finalVisible.right-viewport.right)<1,'carousel rail actually reveals the requested window');
      await capture('carousel_final');
      const findSource=()=>carousel()?.querySelector('[data-result-key="playlist:28"] .search-card-open');
      const top=one('.search-result-panel').scrollTop;
      click(findSource());
      await wait(()=>one('.playlist-hero h1')?.textContent.includes('合成歌单 28'),'later carousel card opens its playlist');
      click(button('返回上一页'));
      await wait(()=>current()===4&&findSource()===document.activeElement&&Math.abs(one('.search-result-panel').scrollTop-top)<=1,'Back restores exact section, page, scroll, and later-card focus');
      await capture('carousel_back_focus');
      await theme(760,540,'dark');
      await wait(()=>columns()<6,'narrow carousel measures resized viewport');
      assert(best().querySelectorAll('[data-result-key]').length===3&&
        getComputedStyle(best().querySelector('.search-result-grid')).gridTemplateColumns.split(' ').length===1,'narrow best match stacks without dropping the third card');
      reveal();
      for(let attempt=0;attempt<10&&next().getAttribute('aria-disabled')!=='true';attempt++) { click(next()); await sleep(40); }
      await wait(settled,'narrow rail settles');
      assert(current()===10-columns()&&visible().at(-1)===9,'narrow last page shows the final item');
      await capture('carousel_narrow_dark');
      const last=current();
      click(previous());
      await wait(()=>current()<last&&settled(),'previous arrow returns to prior group');
      const preserved=current();
      click(button('查看全部歌单',carousel().closest('section')));
      await wait(()=>ready()&&one('[data-category="playlists"]')?.getAttribute('aria-selected')==='true'&&!one('.search-carousel'),'section heading opens regular complete category');
      click(button('返回上一页'));
      await wait(()=>current()===preserved&&settled(),'category Back restores the carousel independently of network');
      await capture('carousel_category_back');
      await emit('complete',{ok:true,searchCarouselVerified:true,searchBestThirdVerified:true,stageCount:count,realAccountWrites:0});
      return;
    }
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
      await theme(1120,760,'light');
      await fixture('album_artist_search');
      const provides=one('#app').__vue_app__._context.provides;
      const core=provides[Reflect.ownKeys(provides).find(key=>typeof key==='symbol'&&key.description==='core-client')];
      const read=core.searchResults;
      core.searchResults=async (...args)=>{
        const response=await read(...args);
        const query=args[1];
        if(query.category==='albums'&&query.keyword==='合成专辑续页'&&query.offset===10)
          response.value.more=false; // The proof reaches the last row of a finite, two-page list.
        for(const section of response.value.sections){
          // Simulate relevance order that is neither ID nor date order.
          if(query.category==='albums'&&query.keyword==='合成'&&section.items.length>=3){
            [section.items[1],section.items[2]]=[section.items[2],section.items[1]];
          }
          for(const item of section.items){
            if(item.kind==='album'&&item.album.id==='201'){
              item.album.publishedAt=new Date(2020,5,17,12).getTime();
              item.album.explicit=true;
              item.album.translations=['首个译名','合成译名'];
              item.album.aliases=['合成别名'];
            }
          }
        }
        return response;
      };
      await fresh('合成');
      const card=()=>one('[data-result-key="album:201"]');
      const artist=()=>card()?.querySelector('[data-result-action="artist:101:1"]');
      const link='https://music.163.com/album?id=201', copies=[], membershipWrites=[], albumReads=[];
      const fetchIPC=window.fetch;
      window.fetch=function(url,options){
        if(String(url).endsWith('/copy_text'))copies.push(JSON.parse(options.body).text);
        if(String(url).endsWith('/search_results')){
          const {query}=JSON.parse(options.body);
          if(query.category==='albums'&&query.keyword==='合成专辑续页')albumReads.push([query.offset,query.limit]);
        }
        if(String(url).endsWith('/catalog_membership')){
          const {resource,action}=JSON.parse(options.body);
          if(action.kind==='set')membershipWrites.push({resource,action});
        }
        return fetchIPC.call(window,url,options);
      };
      const menu=()=>one('.action-menu[role="menu"]:not([inert])');
      const share=()=>one('.track-share-dialog[open]:not([inert])');
      const membership=()=>one('.album-membership-dialog[open]:not([inert])');
      const table=()=>one('.search-album-table[role="table"]');
      const headers=()=>[...table().querySelectorAll('[role="columnheader"]')];
      const measures=nodes=>nodes.map(node=>node.getBoundingClientRect().width);
      const aligned=row=>measures(headers()).every((width,index)=>
        Math.abs(row.children[index].getBoundingClientRect().width-width)<1);
      const invoke=(name,args={})=>window.__TAURI_INTERNALS__.invoke(name,args);
      const {epoch}=await invoke('account_snapshot');
      async function albumMenu(anchor){
        anchor.scrollIntoView({block:'nearest'});
        anchor.focus({preventScroll:true});
        anchor.dispatchEvent(new KeyboardEvent('keydown',{key:'F10',shiftKey:true,bubbles:true,cancelable:true}));
        await wait(()=>menu()?.contains(document.activeElement)&&button('分享…',menu()),'album link actions');
      }
      for(const category of ['all','albums']){
        if(category!=='all')await choose(category);
        const target=artist();
        assert(card()?.querySelector('.search-card-open')&&card().querySelector('.search-card-play'),'album details and playback are independent');
        assert([...card().querySelectorAll('.search-card-artist')].map(node=>node.textContent.trim()).join(',')==='合成主艺人,合成合作艺人',
          'primary and credited artists are independent controls in order');
        assert(target?.getAttribute('aria-label')==='查看歌手 合成合作艺人'&&
          card().querySelector('.search-card-artist-text')?.textContent.trim()==='合成未标识艺人',
          'credit without native identity is plain text');
        const title=card().querySelector('.search-album-title-copy');
        const explicit=card().querySelector('.search-album-explicit');
        const date=card().querySelector('.search-album-date time');
        assert(title?.textContent.trim()==='合成专辑 1 · SyntheticLongTitle'&&
          title.querySelector('mark')?.textContent==='合成','album title keeps its literal keyword match');
        assert(explicit?.getAttribute('aria-label')==='含成人内容'&&
          explicit.getBoundingClientRect().width===12&&
          explicit.getBoundingClientRect().right<=title.parentElement.getBoundingClientRect().right+1,
          'explicit indicator stays visible within the title row');
        assert(date?.dateTime==='2020-06-17'&&date.textContent==='2020-06-17'&&
          date.getBoundingClientRect().right<=date.closest('.search-album-time-cell,.search-card-credit').getBoundingClientRect().right+1,
          'album date stays visible in local calendar format');
        const unknown=one('[data-result-key="album:202"]');
        assert(unknown&&!unknown.querySelector('.search-album-explicit,.search-album-date'),
          'albums without fields show neither fabricated badge nor date');
        const anchor=card().querySelector('.search-card-open');
        if(category==='all'){
          assert(!table()&&card().classList.contains('search-cover-card')&&!card().querySelector('.search-album-table-alias'),
            'comprehensive albums remain square cards');
        }else{
          const alias=card().querySelector('.search-album-table-alias');
          assert(headers().map(node=>node.textContent.trim()).join(',')==='#,标题,作者,时间'&&
            card().getAttribute('role')==='row'&&card().getAttribute('aria-rowindex')==='2'&&
            Math.abs(card().getBoundingClientRect().height-64)<1&&
            Math.abs(card().querySelector('.search-result-art').getBoundingClientRect().width-48)<1&&aligned(card()),
            'album category uses four aligned columns, 64px rows, and 48px cover art');
          assert(alias?.textContent.includes('合成译名')&&alias.querySelector('mark')?.textContent==='合成'&&
            date.closest('.search-album-time-cell')&&target.closest('.search-album-author-cell'),
            'alias match, artist link, and release date stay in separate official cells');
          const byIndex=index=>table().querySelector(`[data-grid-index="${index}"] .search-album-row`);
          assert([0,1,2].map(index=>byIndex(index)?.dataset.resultKey).join(',')==='album:201,album:203,album:202',
            'album rows follow returned relevance order');
          anchor.focus({preventScroll:true});key(anchor,'ArrowDown');
          await wait(()=>document.activeElement===byIndex(1).querySelector('.search-card-open'),
            'Down moves to the next album row, not to another grid column');
          const handles=all('.search-album-column-resizer');
          assert(handles.length===1,'only the useful title divider is actionable');
          const before=measures(headers());
          handles[0].focus({preventScroll:true});key(handles[0],'ArrowLeft');
          await wait(()=>Math.abs(measures(headers())[1]-before[1]+12)<1&&aligned(card()),
            'title divider gives width to the author without breaking row alignment');
          assert(Math.abs(measures(headers())[3]-128)<1,'publication date column stays fixed');
          await capture('album_table_columns');
        }
        await albumMenu(anchor);
        click(button('分享…',menu()));
        await wait(()=>share()?.querySelector('[aria-label="专辑分享链接"]')?.value===link,
          'album share offers the official album URL');
        await capture('album_share_'+category);
        click(button('关闭分享',share()));
        await wait(()=>!share()&&document.activeElement===anchor,'album share restores the exact opener');
        await albumMenu(anchor);
        click(button('复制链接',menu()));
        const feedback=()=>one('.album-action-feedback:not([inert])');
        await wait(()=>feedback()?.textContent.includes('链接已复制'),'album copies through the native clipboard');
        click(button('关闭',feedback()));
        await wait(()=>document.activeElement===anchor,'copy feedback restores the exact opener');
        await albumMenu(anchor);
        click(button('收藏',menu()));
        const confirm=()=>membership()?.querySelector('footer .primary:not(:disabled)');
        await wait(()=>confirm()&&membership()?.textContent.includes('合成专辑 1'),'album collection reads current membership');
        assert(membershipWrites.length===0,'opening confirmation cannot write membership');
        await capture('album_collect_'+category);
        click(category==='all'?button('取消',membership()):confirm());
        await wait(()=>!membership()&&document.activeElement===anchor,'album confirmation restores result focus');
        if(category==='all'){
          assert(membershipWrites.length===0,'album cancellation cannot write membership');
        }else{
          const write=membershipWrites[0];
          assert(membershipWrites.length===1&&write.resource.kind==='album'&&write.resource.id==='201'&&
            write.action.desired===true,'one explicit membership write targets the selected album');
          await albumMenu(anchor);
          assert(!button('收藏',menu()),'confirmed native membership removes the duplicate action');
          key(document.activeElement,'Escape');
          await wait(()=>!menu()&&document.activeElement===anchor,'updated menu restores focus');
        }
        click(target);
        await wait(()=>one('.artist-header h1')?.textContent.trim()==='相似歌手 101'&&one('.track-row'),
          'opens native details for the selected artist');
        click(button('返回上一页'));
        await wait(()=>ready()&&one(`[data-category="${category}"]`)?.getAttribute('aria-selected')==='true'&&
          document.activeElement===artist(),'Back restores the exact credited artist button');
        await capture('album_artist_'+category);
        if(category==='albums'){
          const play=()=>card().querySelector('.search-album-number .search-card-play');
          click(play());
          await wait(()=>one('.playerbar')?.dataset.state==='playing','index plays through the native album action');
          const native=await invoke('player_snapshot',{epoch});
          assert(native.origin?.kind==='album'&&native.origin.id==='201'&&native.queueLen>0,
            'album index plays the selected album, not its search preview or an artist');
          const center=node=>{const r=node.getBoundingClientRect();return [r.x+r.width/2,r.y+r.height/2];};
          assert(center(play()).every((value,index)=>
            Math.abs(value-center(play().parentElement)[index])<1),
            'album play stays centered over the index while focused and playing');
          await capture('album_table_playing');
          click(play());
          await wait(()=>one('.playerbar')?.dataset.state==='paused','the same album index pauses playback');
          assert((await invoke('player_snapshot',{epoch})).playId===native.playId,
            'the album index pauses without starting a new queue');
        }
      }
      assert(copies.length===2&&copies.every(value=>value===link),'clipboard receives only canonical album URLs');
      await theme(760,540,'dark');
      await fixture('good');await fresh('合成专辑续页');await choose('albums');
      const narrow=card();
      const narrowBadge=narrow?.querySelector('.search-album-explicit');
      const narrowDate=narrow?.querySelector('.search-album-date time');
      assert(narrowBadge?.getBoundingClientRect().width===12&&
        narrowBadge.getBoundingClientRect().right<=narrow.querySelector('.search-album-name').getBoundingClientRect().right+1&&
        narrowDate?.getBoundingClientRect().right<=narrow.querySelector('.search-album-time-cell').getBoundingClientRect().right+1&&aligned(narrow),
        'narrow album rows keep their badge and date in the corresponding cells');
      const results=()=>one('.search-result-panel');
      const grid=()=>results()?.querySelector('.search-result-grid');
      const tailRow=()=>grid()?.querySelector('[data-grid-index="19"] .search-album-row');
      const tail=()=>tailRow()?.querySelector('.search-card-open');
      assert(grid()?.dataset.loadedCount==='10'&&!one('.search-result-pagination')&&
        JSON.stringify(albumReads)==='[[0,10]]','album results begin with one ten-item waterfall page');
      hold('albums','合成专辑续页');
      results().scrollTop=results().scrollHeight;
      results().dispatchEvent(new Event('scroll'));
      await wait(()=>held&&one('.search-load-more')?.getAttribute('aria-busy')==='true'&&
        grid()?.dataset.loadedCount==='10','pending continuation keeps the first ten rows');
      await capture('album_table_loading');
      release();
      await wait(()=>grid()?.dataset.loadedCount==='20','scrolling appends the next album page');
      const visibleOpen=grid().querySelector('.search-card-open');
      visibleOpen.focus({preventScroll:true});key(visibleOpen,'End');
      await wait(()=>tail()&&document.activeElement===tail(),'End focuses the last virtual album row');
      assert(JSON.stringify(albumReads)==='[[0,10],[10,10]]'&&all('.search-result-card').length<20&&
        tailRow().getAttribute('aria-rowindex')==='21'&&tailRow().querySelector('.search-album-number').textContent.trim()==='20'&&
        one('.search-load-more').textContent.includes('没有更多可显示的结果')&&aligned(tailRow()),
        'native offsets advance by ten and the bounded table reaches row twenty');
      const rect=tail().getBoundingClientRect();
      assert(tail().contains(document.elementFromPoint(rect.x+rect.width/2,rect.y+rect.height/2)),
        'appended album is visibly reachable');
      const scrolled=results().scrollTop;
      await capture('album_waterfall_appended');
      click(tail());
      await wait(()=>one('.album-page')||one('.album-hero')||one('.album-header h1'),
        'appended album opens native details');
      click(button('返回上一页'));
      await wait(()=>ready()&&grid()?.dataset.loadedCount==='20'&&document.activeElement===tail()&&
        Math.abs(results().scrollTop-scrolled)<=1,'Back restores appended results, scroll and focused album');
      assert(JSON.stringify(albumReads)==='[[0,10],[10,10]]','Back does not replay album page requests');
      await capture('album_waterfall_back');
      core.searchResults=read;
      await emit('complete',{ok:true,searchAlbumArtistVerified:true,searchAlbumLinksVerified:true,searchAlbumCollectVerified:true,searchAlbumMetadataVerified:true,searchAlbumWaterfallVerified:true,searchAlbumTableVerified:true,albumReads,stageCount:count,realMusicRequests:0,realAccountWrites:0});
      return;
    }
    if(window.__cumulusSearchCollectionNextProof){
      await wait(()=>one('.track-row'),'daily ready');
      await fixture('good');await fresh('合成下一首合集');
      const invoke=(name,args={})=>window.__TAURI_INTERNALS__.invoke(name,args);
      const {epoch}=await invoke('account_snapshot');
      const playlistReads=[];
      let downloadCommands=0;
      const fetchIPC=window.fetch;
      window.fetch=function(url,options){
        if(String(url).endsWith('/playlist_page')){
          const {id,page}=JSON.parse(options.body);
          playlistReads.push([id,page.offset,page.limit]);
        }
        if(String(url).endsWith('/download_command'))downloadCommands++;
        return fetchIPC.call(window,url,options);
      };
      async function checkPlaylistDownload(category){
        const card=one('[data-result-key="playlist:3"]'), anchor=card?.querySelector('.search-card-open');
        assert(anchor&&!anchor.disabled,'playlist opens from search');
        anchor.scrollIntoView({block:'nearest'});
        anchor.focus({preventScroll:true});
        anchor.dispatchEvent(new KeyboardEvent('keydown',{key:'F10',shiftKey:true,bubbles:true,cancelable:true}));
        const action=()=>one('.action-menu:not([inert]) [data-menu-key="download-playlist"]');
        await wait(()=>action(),'search playlist download action');
        assert(action().textContent.trim()==='下载全部'&&!action().disabled,'complete download action is enabled');
        click(action());
        const dialog=()=>one('.download-dialog[open]:not([inert])');
        await wait(()=>dialog()?.querySelector('h2')?.textContent.trim()==='下载 40 首歌曲',
          'download confirmation reads all authoritative playlist tracks');
        assert(dialog().querySelector('.download-selection')?.textContent.includes('合成歌曲 1'),
          'confirmation uses native track details');
        await capture('playlist_search_download_'+category);
        click(button('关闭下载对话框',dialog()));
        await wait(()=>!dialog()&&document.activeElement===anchor,'download cancellation restores the playlist card');
      }
      await checkPlaylistDownload('all');
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
      await choose('playlists');
      await checkPlaylistDownload('playlists');
      assert(downloadCommands===0,'opening and cancelling the download confirmation cannot start a download');
      await fixture('playlist_playability');await fresh('合成空歌单与未知歌单');
      const menu=()=>one('.action-menu[role="menu"]:not([inert])');
      const entry=key=>menu()?.querySelector(`[data-menu-key="${key}"]`);
      async function openPlaylistMenu(id){
        const card=one(`[data-result-key="playlist:${id}"]`);
        const anchor=card?.querySelector('.search-card-open');
        assert(anchor&&!anchor.disabled,'playlist detail remains navigable');
        anchor.scrollIntoView({block:'nearest'});
        anchor.focus({preventScroll:true});
        anchor.dispatchEvent(new KeyboardEvent('keydown',{key:'F10',shiftKey:true,bubbles:true,cancelable:true}));
        await wait(()=>entry('next')&&menu()?.contains(document.activeElement),'playlist playback menu');
        return {card,anchor};
      }
      for(const category of ['all','playlists']){
        if(category!=='all')await choose(category);
        const empty=await openPlaylistMenu(4);
        assert(entry('play')?.disabled&&entry('next')?.disabled&&
          !empty.card.querySelector('.search-card-play'),'known empty blocks playback and next');
        await capture('playlist_known_empty_'+category);
        key(document.activeElement,'Escape');
        await wait(()=>!menu()&&document.activeElement===empty.anchor,'empty playlist menu restores focus');
        const unknown=await openPlaylistMenu(5);
        assert(!entry('play')?.disabled&&!entry('next').disabled&&
          unknown.card.querySelector('.search-card-play:not(:disabled)')&&
          !unknown.card.querySelector('.search-result-sub')?.textContent.includes('0 首'),
          'missing count remains available without displaying a false zero');
        if(category==='all'){
          key(document.activeElement,'Escape');
          await wait(()=>!menu(),'close unknown playlist menu');
        }else{
          click(entry('play'));
          await wait(()=>one('.playerbar')?.dataset.state==='playing','unknown-count playlist starts native playback');
          const view=await invoke('player_snapshot',{epoch});
          assert(view.origin?.kind==='playlist'&&view.origin.id==='5'&&view.queueLen===40,
            'native playlist, not the search preview, supplies the full queue');
        }
      }
      await emit('complete',{ok:true,searchCollectionNextVerified:true,searchPlaylistDownloadVerified:true,searchPlaylistPlayabilityVerified:true,playlistReads,downloadCommands,stageCount:count,realMusicRequests:0,realAccountWrites:0});
      return;
    }
    if(window.__cumulusSearchPlaylistReopenWidths){
      await wait(()=>one('.track-row'),'daily ready');
      await theme(1120,760,'light');
      await fixture('playlist_creator_search');await fresh('合成作者入口');await choose('playlists');
      const cells=()=>all('.search-playlist-columns [role="columnheader"]');
      const row=()=>one('.search-playlist-row');
      const matches=nodes=>nodes.length===5&&nodes.every((node,index)=>
        Math.abs(node.getBoundingClientRect().width-window.__cumulusSearchPlaylistReopenWidths[index])<1);
      await wait(()=>matches(cells())&&row()&&matches([...row().children]),
        'fresh WebView process loads the saved playlist column proportions');
      await capture('playlist_columns_reopened');
      await emit('complete',{ok:true,searchPlaylistResizeReopened:true,stageCount:count,realMusicRequests:0,realAccountWrites:0});
      return;
    }
    if(window.__cumulusSearchPlaylistCreatorProof){
      await wait(()=>one('.track-row'),'daily ready');
      await theme(1120,760,'light');
      await fixture('playlist_creator_search');await fresh('合成作者入口');
      const card=id=>one(`[data-result-key="playlist:${id}"]`);
      const creator=()=>card(3)?.querySelector('.search-playlist-creator');
      const link='https://music.163.com/#/playlist?id=3';
      const clipboard=[], mutations=[], playlistWaterfallReads=[];
      const fetchIPC=window.fetch;
      window.fetch=function(url,options){
        if(String(url).endsWith('/copy_text'))clipboard.push(JSON.parse(options.body).text);
        if(String(url).endsWith('/mutate_playlist'))mutations.push(JSON.parse(options.body).command);
        if(String(url).endsWith('/search_results')){
          const {query}=JSON.parse(options.body);
          if(query.category==='playlists'&&query.keyword==='合成歌单续页')playlistWaterfallReads.push([query.offset,query.limit]);
        }
        return fetchIPC.call(window,url,options);
      };
      const menu=()=>one('.action-menu[role="menu"]:not([inert])');
      const share=()=>one('.track-share-dialog[open]:not([inert])');
      const collect=()=>one('.playlist-mutation-dialog[open]:not([inert])');
      const menuItem=key=>menu()?.querySelector(`[data-menu-key="${key}"]`);
      const columns=()=>one('.search-playlist-columns');
      const handles=()=>[...columns().querySelectorAll('.search-playlist-column-resizer')];
      const widths=()=>[...columns().children].map(node=>node.getBoundingClientRect().width);
      const sameWidths=(left,right)=>left.every((value,index)=>Math.abs(value-right[index])<1);
      let initialWidths, savedWidths, trustedPointer=false;
      function assertAligned(row,expected){
        assert(sameWidths([...row.children].map(node=>node.getBoundingClientRect().width),expected),
          'header and visible virtual body use the same widths');
        assert(Math.abs(row.getBoundingClientRect().height-64)<1,
          'resizing keeps the recovered row height');
      }
      async function playlistMenu(id){
        const anchor=card(id)?.querySelector('.search-card-open');
        assert(anchor&&!anchor.disabled,'playlist card available');
        anchor.scrollIntoView({block:'nearest'});
        anchor.focus({preventScroll:true});
        anchor.dispatchEvent(new KeyboardEvent('keydown',{key:'F10',shiftKey:true,bubbles:true,cancelable:true}));
        await wait(()=>menuItem('share')&&menu()?.contains(document.activeElement),'playlist context menu with link actions');
        return anchor;
      }
      for(const category of ['all','playlists']){
        if(category!=='all')await choose(category);
        const action=creator(), bad=card(4);
        if(category==='all'){
          assert(!one('.search-playlist-table'),'comprehensive playlist preview stays a card');
        }else{
          const table=one('.search-playlist-table[role="table"]');
          const row=card(3);
          const headers=[...table.querySelectorAll('[role="columnheader"]')].map(node=>node.textContent.trim());
          assert(headers.join(',')==='#,标题,歌曲数,创建者,播放量'&&row.getAttribute('role')==='row'&&
            Math.abs(row.getBoundingClientRect().height-64)<1,'official five columns and fixed 64px rows');
          assert(row.querySelector('.search-playlist-number').textContent.trim()==='01'&&
            row.querySelector('.search-playlist-track-count').textContent.trim()==='40 首'&&
            row.querySelector('.search-playlist-play-count [aria-hidden="true"]').textContent==='12345'&&
            bad.querySelector('.search-playlist-play-count').textContent.trim()==='—',
            'row keeps visible counts separate and does not imply a private playlist count');
          await wait(()=>handles().length===3,'only title, song count and creator have resize handles');
          const [title,trackCount,owner]=handles();
          initialWidths=widths();
          assertAligned(row,initialWidths);
          assert(handles().map(node=>node.getAttribute('aria-label')).join(',')===
            '调整标题列宽,调整歌曲数列宽,调整创建者列宽'&&handles().every(node=>node.tabIndex===0)&&
            title.getAttribute('aria-valuemin')==='200'&&title.getAttribute('aria-orientation')==='vertical',
            'three resize handles have keyboard focus and expose their width');
          let pointer;
          if(window.__cumulusSearchTrustedPlaylistPointer){
            const rect=title.getBoundingClientRect(), ratio=devicePixelRatio;
            pointer={x:Math.floor((screenX+rect.left+rect.width/2)*ratio),
              y:Math.floor((screenY+rect.top+rect.height/2)*ratio),ratio};
            title.addEventListener('pointerdown',event=>{trustedPointer=event.isTrusted;},{once:true});
          }
          await capture('playlist_table_columns',{pointer});
          if(pointer){
            await wait(()=>trustedPointer&&Math.abs(widths()[1]-initialWidths[1]-24)<1,
              'trusted pointer moves title boundary 24 px after leaving the handle');
            assertAligned(row,widths());
          }
          trackCount.focus({preventScroll:true});
          const beforeKeys=widths();
          key(trackCount,'ArrowRight');
          await wait(()=>Math.abs(widths()[2]-beforeKeys[2]-12)<1,'keyboard grows song count');
          assert(Math.abs(widths()[3]-beforeKeys[3]+12)<1&&document.activeElement===trackCount,
            'right keyboard resize takes width from the next eligible column and keeps focus');
          assertAligned(row,widths());
          owner.focus({preventScroll:true});key(owner,'Home');
          await wait(()=>Math.abs(widths()[3]-128)<1&&widths()[4]>92,'creator can transfer its surplus to play count');
          title.focus({preventScroll:true});key(title,'End');
          await wait(()=>sameWidths(widths().slice(2),[79,128,92]),'title takes surplus across all right-side columns');
          assert(Number(title.getAttribute('aria-valuenow'))===Number(title.getAttribute('aria-valuemax')),
            'separator updates the accessible current width at the right clamp');
          key(title,'Home');
          await wait(()=>Math.abs(widths()[1]-200)<1&&widths()[2]>79,'left clamp restores the minimum title width');
          savedWidths=widths();
          assert(!sameWidths(savedWidths,initialWidths),'proof has adjusted the default widths');
          assertAligned(row,savedWidths);
          await capture('playlist_columns_resized');
          const opener=row.querySelector('.search-card-open');
          opener.focus({preventScroll:true});key(opener,'ArrowDown');
          await wait(()=>document.activeElement===bad.querySelector('.search-card-open'),'Down moves exactly one table row');
          const menuTrigger=row.querySelector('.menu-trigger');
          click(menuTrigger);
          await wait(()=>menuItem('share'),'inline title-cell menu uses shared playlist commands');
          key(document.activeElement,'Escape');
          await wait(()=>!menu()&&document.activeElement===menuTrigger,'inline menu returns focus');
        }
        assert(action?.getAttribute('aria-label')==='查看创建者 合成匹配作者 的主页','typed creator action');
        assert(card(3).querySelector('.search-card-open')&&card(3).querySelector('.search-card-play'),'playlist open and playback remain separate');
        assert(bad?.textContent.includes('合成不匹配作者')&&!bad.querySelector('.search-playlist-creator'),
          'mismatched creator remains plain text');
        const privateAnchor=await playlistMenu(4);
        assert(menuItem('share')?.disabled&&menuItem('copy')?.disabled&&
          menuItem('share').title==='私密歌单不能分享','private playlist cannot share or copy a link');
        key(document.activeElement,'Escape');
        await wait(()=>!menu()&&document.activeElement===privateAnchor,'private menu returns focus');
        const publicAnchor=await playlistMenu(3);
        assert(!menuItem('share').disabled&&!menuItem('copy')?.disabled,'public nonempty playlist can use links');
        assert(!menuItem('subscribe'),'playlist already in the current collection cannot be collected again');
        click(menuItem('share'));
        await wait(()=>share()?.querySelector('[aria-label="歌单分享链接"]')?.value===link,'share dialog uses the canonical public URL');
        await capture('playlist_search_share_'+category);
        click(button('关闭分享',share()));
        await wait(()=>!share()&&document.activeElement===publicAnchor,'share close restores playlist focus');
        await playlistMenu(3);
        click(menuItem('copy'));
        const feedback=()=>one('.playlist-action-feedback:not([inert])');
        await wait(()=>feedback()?.textContent.includes('链接已复制'),'playlist menu copies the canonical URL via native clipboard');
        assert(document.activeElement===publicAnchor,'copy feedback leaves focus on the original card');
        const collectAnchor=await playlistMenu(31);
        assert(menuItem('subscribe')?.textContent.trim()==='收藏'&&!menuItem('subscribe').disabled,
          'foreign playlist outside the current collection can be collected');
        click(menuItem('subscribe'));
        const confirm=()=>collect()?.querySelector('footer .primary[aria-disabled="false"]');
        await wait(()=>confirm()&&collect()?.textContent.includes('可收藏的合成歌单'),'account-bound playlist confirmation');
        assert(mutations.length===0,'opening confirmation does not mutate the account');
        await capture('playlist_search_collect_'+category);
        click(category==='all'?button('取消',collect()):confirm());
        await wait(()=>!collect()&&document.activeElement===collectAnchor,'collection dialog returns to the same result');
        if(category==='all'){
          assert(mutations.length===0,'cancel must not send any mutation');
        }else{
          assert(mutations.length===1&&mutations[0].kind==='subscribe'&&
            mutations[0].id==='31'&&mutations[0].subscribed===true,
            'explicit confirmation sends one subscription command for the selected playlist');
          await playlistMenu(31);
          assert(!menuItem('subscribe'),'native readback removes the duplicate collect action');
          key(document.activeElement,'Escape');
          await wait(()=>!menu()&&document.activeElement===collectAnchor,'updated menu returns to the same card');
        }
        click(action);
        await wait(()=>one('.playlist-creator-page h1')?.textContent==='合成用户主页 501'&&
          one('.playlist-creator-page [data-library-open="30"]'),'creator profile and public playlists');
        click(button('返回上一页'));
        await wait(()=>ready()&&one(`[data-category="${category}"]`)?.getAttribute('aria-selected')==='true'&&
          document.activeElement===creator(),'Back restores creator button, not the card opener');
        if(category==='playlists'){
          await wait(()=>sameWidths(widths(),savedWidths),'Back restores the adjusted playlist widths');
          assertAligned(card(3),savedWidths);
        }
        await capture('playlist_creator_'+category);
      }
      const invoke=(name,args={})=>window.__TAURI_INTERNALS__.invoke(name,args);
      const {epoch}=await invoke('account_snapshot');
      const beforeLink=await invoke('player_snapshot',{epoch});
      card(3).querySelector('.search-playlist-number').click();
      await wait(()=>!one('.playlist-creator-page')&&one('.playlist-hero h1')?.textContent.trim()==='合成歌单 3'&&one('.track-row'),'playlist card still opens playlist');
      const afterLink=await invoke('player_snapshot',{epoch});
      assert(afterLink.queueRevision===beforeLink.queueRevision&&afterLink.playId===beforeLink.playId,
        'plain row activation opens details without starting playback');
      assert(clipboard.length===2&&clipboard.every(text=>text===link),'no private or unexpected URL reaches the clipboard');
      await theme(760,540,'dark');
      await fixture('short_pages');await fresh('合成歌单续页');await choose('playlists');
      const panel=()=>one('.search-result-panel'), grid=()=>panel()?.querySelector('.search-result-grid');
      assert(!handles().length,'compact window has no ineffective keyboard or pointer resize handles');
      const scrollBottom=()=>{
        panel().scrollTop=panel().scrollHeight;
        panel().dispatchEvent(new Event('scroll'));
      };
      assert(grid()?.dataset.loadedCount==='18'&&!one('.search-result-pagination')&&
        JSON.stringify(playlistWaterfallReads)==='[[0,10]]','playlist starts in continuous ten-item mode');
      scrollBottom();
      await wait(()=>grid()?.dataset.loadedCount==='28'&&panel().textContent.includes('有 1 项结果暂无法展示'),
        'playlist continuation appends valid results without counting the malformed card');
      assert(JSON.stringify(playlistWaterfallReads)==='[[0,10],[18,10]]',
        'continuation advances by raw rows even when the service does not return ten');
      await capture('playlist_waterfall_appended');
      scrollBottom();
      await wait(()=>grid()?.dataset.loadedCount==='32'&&panel().textContent.includes('没有更多可显示的结果'),
        'playlist continuation reaches terminal page');
      assert(JSON.stringify(playlistWaterfallReads)==='[[0,10],[18,10],[29,10]]'&&
        all('.search-result-card').length<32&&button('使用分页查看'),
        'malformed rows do not shift the native offset and virtualization stays bounded');
      scrollBottom();
      const footer=one('.search-load-more');
      assert(footer.getBoundingClientRect().bottom<=panel().getBoundingClientRect().bottom+1,
        'the terminal status and numbered fallback are reachable');
      await capture('playlist_waterfall_complete');
      const lateRow=()=>grid()?.querySelector('[data-grid-index="29"] .search-playlist-row');
      const lateOpen=()=>lateRow()?.querySelector('.search-card-open');
      await wait(lateRow,'the virtual table keeps its late visible rows after reaching the end');
      const lateId=lateRow()?.dataset.resultKey.split(':')[1];
      assert(lateId&&lateRow().getAttribute('aria-rowindex')==='31'&&
        lateRow().querySelector('.search-playlist-number').textContent.trim()==='30',
        'virtualized table shows a later row with the correct displayed index');
      lateOpen().scrollIntoView({block:'center'});
      const top=panel().scrollTop;
      click(lateOpen());
      await wait(()=>one('.playlist-hero h1')?.textContent.includes('合成歌单 '+lateId),'late row opens actual playlist detail');
      click(button('返回上一页'));
      await wait(()=>ready()&&document.activeElement===lateOpen()&&Math.abs(panel().scrollTop-top)<=1,
        'late detail Back restores exact table viewport and focus').catch(error=>{
          throw Error(`${error.message}: ${JSON.stringify({before:top,after:panel()?.scrollTop,
            focus:document.activeElement?.getAttribute('aria-label'),wanted:lateOpen()?.getAttribute('aria-label')})}`);
        });
      assert(JSON.stringify(playlistWaterfallReads)==='[[0,10],[18,10],[29,10]]','Back reuses the loaded table, not a new request');
      await capture('playlist_table_late_row_back');
      await emit('complete',{ok:true,searchPlaylistCreatorVerified:true,searchPlaylistLinksVerified:true,searchPlaylistSubscribeVerified:true,searchPlaylistWaterfallVerified:true,searchPlaylistTableVerified:true,searchPlaylistResizeVerified:true,searchPlaylistTrustedPointer:trustedPointer,playlistWidths:savedWidths,playlistWaterfallReads,stageCount:count,realMusicRequests:0,realAccountWrites:0});
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
      const mvCard=()=>one('[data-result-key="mv:601"]');
      async function checkMvArtist(category){
        checkMvMetadata(mvCard());
        const artist=()=>mvCard()?.querySelector('[data-result-action="artist:100:0"]');
        const target=artist();
        assert(target?.getAttribute('aria-label')==='查看歌手 合成歌手','MV has an independent artist control');
        target.scrollIntoView({block:'nearest'});
        const rect=target.getBoundingClientRect();
        assert(rect.width>0&&target.contains(document.elementFromPoint(rect.x+rect.width/2,rect.y+rect.height/2)),
          'card playback overlay does not intercept artist');
        click(target);
        await wait(()=>one('.artist-header h1')?.textContent.trim()==='相似歌手 100'&&one('.track-row'),
          'MV credit opens its native artist details');
        click(button('返回上一页'));
        await wait(()=>ready()&&one(`[data-category="${category}"]`)?.getAttribute('aria-selected')==='true'&&
          document.activeElement===artist(),'Back restores the exact MV artist credit');
        await capture('search_mv_artist_'+category);
      }
      await checkMvArtist('all');
      await choose('mvs');
      await checkMvArtist('mvs');
      const card=mvCard();
      const trigger=button('播放 MV 合成 MV 1',card);
      const cue=card?.querySelector('.search-video-play-glyph');
      const placeholder=card?.querySelector('.search-result-art > .icon');
      assert(trigger&&!trigger.disabled&&trigger.tabIndex===0&&cue?.getAttribute('aria-hidden')==='true'&&
        !card.querySelector('.search-card-play'),'one enabled card activation target and decorative cue');
      trigger.focus({preventScroll:true});
      await wait(()=>document.activeElement===trigger&&Number(getComputedStyle(cue).opacity)>.9,'MV cue visible at focus');
      assert(placeholder&&Number(getComputedStyle(placeholder).opacity)===0,'Play cue does not overlap the empty-cover icon');
      const invoke=(name,args={})=>window.__TAURI_INTERNALS__.invoke(name,args);
      const {epoch}=await invoke('account_snapshot');
      async function nativeVideo(label){
        let native;
        for(let i=0;i<170;i++){
          const view=await invoke('video_snapshot',{epoch});
          if(view.id==='601'&&view.state==='playing'&&view.position>.15&&Number(one('.video-canvas')?.dataset.renderedFrames)>1){native=view;break;}
          await sleep(70);
        }
        assert(native&&native.actualResolution===480,label+' reached advancing native video playback');
        const canvas=one('.video-canvas');
        const pixels=canvas.getContext('2d').getImageData(0,0,canvas.width,canvas.height).data;
        assert(pixels.some((value,index)=>index%4!==3&&value>20),'decoded frame reaches the canvas');
      }
      await capture('search_mv_play_target');
      click(trigger);
      await nativeVideo('category card');
      await capture('search_mv_category_native_frame');
      click(button('关闭视频'));
      click(button('返回上一页'));
      await wait(()=>ready()&&one('[data-category="mvs"]')?.getAttribute('aria-selected')==='true','return to MV category');
      click(button('返回上一页'));
      await wait(()=>ready()&&one('[data-category="all"]')?.getAttribute('aria-selected')==='true','return to comprehensive song cards');
      const songCard=one('.search-track-card[data-result-key="song:1"]');
      const badge=songCard?.querySelector('.search-track-mv');
      assert(badge?.getAttribute('aria-label')?.startsWith('播放 MV 合成单曲 1')&&!badge.disabled&&badge.tabIndex===0,
        'song MV badge is independently keyboard accessible');
      assert(!one('.search-track-card[data-result-key="song:2"] .search-track-mv')&&
        !one('.search-track-card[data-result-key="song:3"] .search-track-mv'),'rights-restricted and invalid direct MV targets have no badge');
      badge.scrollIntoView({block:'nearest'});
      badge.focus({preventScroll:true});
      assert(document.activeElement===badge,'song badge receives keyboard focus');
      await capture('search_song_mv_badge');
      click(badge);
      await nativeVideo('song badge');
      await capture('search_mv_native_frame');
      click(button('关闭视频'));
      click(button('返回上一页'));
      await wait(()=>ready()&&one('[data-category="all"]')?.getAttribute('aria-selected')==='true','return to comprehensive after MV');
      await fixture('unavailable');await fresh('合成不可播放视频');await choose('mvs');
      const invalid=one('.search-video-card');
      assert(invalid?.querySelector('.search-card-open')?.disabled&&!invalid.querySelector('.search-video-play-glyph'),
        'opaque non-MV result stays disabled with no Play cue');
      await capture('search_non_mv_target_disabled');
      await emit('complete',{ok:true,searchMvVerified:true,searchMvArtistVerified:true,stageCount:count,realMusicRequests:0,realAccountWrites:0});
      return;
    }
    if(window.__cumulusSearchVoiceQueueProof){
      await wait(()=>one('.track-row'),'daily ready');
      await theme(1120,760,'light');
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
      await fresh('合成声音队列');
      assert(one('.search-voice-card .search-voice-footer')&&!one('.search-voice-table'),
        'comprehensive keeps compact voice cards');
      await choose('voices');
      const card=()=>one('[data-result-key="voice:401"]');
      const play=()=>card()?.querySelector('.search-card-play');
      const headers=()=>all('.search-voice-table [role="columnheader"]');
      const widths=nodes=>nodes.map(node=>node.getBoundingClientRect().width);
      const aligned=row=>widths(headers()).every((width,index)=>
        Math.abs(row.children[index].getBoundingClientRect().width-width)<1);
      assert(card()?.querySelector('.search-voice-radio'),'selected episode has a known parent radio');
      const title=card().querySelector('.search-voice-title-copy');
      title.click();
      assert(one('.search-voice-table')&&(await readQueue()).revision===original.revision,
        'a single title click neither opens the radio nor starts the episode');
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
      assert(headers().map(node=>node.textContent.trim()).join(',')==='#,标题,播放量,时长'&&
        card().getAttribute('aria-rowindex')==='2'&&aligned(card())&&
        Math.abs(card().getBoundingClientRect().height-64)<1&&
        Math.abs(card().querySelector('.search-result-art').getBoundingClientRect().width-48)<1,
        'official four-column search voice table uses aligned 64px rows and 48px art');
      assert(card().querySelector('.search-voice-duration')?.textContent.trim()==='06:00'&&
        card().querySelector('.search-voice-listens')?.textContent.trim()==='1.2万'&&
        card().querySelector('[data-result-action="voice-download"]')&&
        card().querySelector('[data-result-action="voice-comments"]')&&card().querySelector('.menu-trigger'),
        'duration, listener count, and inline episode actions belong to the row');
      assert(Math.abs(title.querySelector('.search-result-name').getBoundingClientRect().left-
        title.querySelector('.search-voice-attribution').getBoundingClientRect().left)<1,
        'voice title aligns with its author and parent link');
      const handles=all('.search-voice-column-resizer'), before=widths(headers());
      assert(handles.length===2,'voice table has title and listener count handles');
      handles[0].focus({preventScroll:true});key(handles[0],'ArrowLeft');
      await wait(()=>Math.abs(widths(headers())[2]-before[2]-12)<1&&aligned(card()),
        'title gives width to the listener count without breaking alignment');
      handles[1].focus({preventScroll:true});key(handles[1],'ArrowLeft');
      await wait(()=>Math.abs(widths(headers())[3]-before[3]-12)<1&&aligned(card()),
        'listener count gives its remaining surplus to the duration');
      title.dispatchEvent(new MouseEvent('dblclick',{bubbles:true}));
      const resumed=await playing(state=>state.track?.voiceId==='401');
      assert(resumed.playId===voice.playId&&resumed.queueRevision===voice.queueRevision,
        'double-click on row text resumes the same voice without opening its radio');
      await capture('search_voice_table_columns');
      const comment=()=>card()?.querySelector('[data-result-action="voice-comments"]');
      const commentReads=[], beforeComments=window.fetch;
      window.fetch=function(url,options){
        if(String(url).endsWith('/comments_page'))commentReads.push(JSON.parse(options.body).resource);
        return beforeComments.call(window,url,options);
      };
      click(comment());
      await wait(()=>one('.comments-page')&&commentReads.some(resource=>resource.kind==='voice'&&resource.id==='401'),
        'inline comments open the exact episode through native IPC');
      click(button('返回上一页'));
      await wait(()=>ready()&&document.activeElement===comment(),'comments Back restores the inline button');
      window.fetch=beforeComments;
      const afterComments=await invoke('player_snapshot',{epoch});
      assert(afterComments.playId===resumed.playId&&afterComments.queueRevision===resumed.queueRevision,
        'comment navigation leaves the voice and queue intact');
      await choose('podcasts');
      click(one('[data-result-key="podcast:301"] .search-card-play'));
      await playing(state=>state.track?.voiceId==='401'&&state.origin?.kind==='podcast');
      const radio=await readQueue();
      assert(radio.tracks.map(row=>row.track.voiceId).join(',')==='401,402,403','explicit podcast card still plays its episode list');
      await capture('search_podcast_keeps_explicit_radio_play');
      const audioReads=[], fetchIPC=window.fetch;
      window.fetch=function(url,options){
        if(String(url).endsWith('/search_results')){
          const {query}=JSON.parse(options.body);
          if(['voices','podcasts'].includes(query.category)&&query.keyword==='合成长音频续页')
            audioReads.push([query.category,query.offset,query.limit]);
        }
        return fetchIPC.call(window,url,options);
      };
      const radioPlaying=await invoke('player_snapshot',{epoch});
      await theme(760,540,'dark');
      await fresh('合成长音频续页');
      const panel=()=>one('.search-result-panel'), grid=()=>panel()?.querySelector('.search-result-grid');
      for(const category of ['voices','podcasts']){
        await choose(category);
        assert(grid()?.dataset.loadedCount==='10'&&!one('.search-result-pagination'),
          'long audio category initially displays ten continuous results');
        if(category==='voices'){
          const first=grid().querySelector('[data-grid-index="0"] .search-card-play');
          first.focus({preventScroll:true});key(first,'ArrowDown');
          await wait(()=>document.activeElement===grid()?.querySelector('[data-grid-index="1"] .search-card-play'),
            'voice play controls navigate one table row at a time');
        }
        if(category==='voices')hold(category,'合成长音频续页');
        panel().scrollTop=panel().scrollHeight;
        panel().dispatchEvent(new Event('scroll'));
        if(category==='voices'){
          await wait(()=>held&&one('.search-load-more')?.getAttribute('aria-busy')==='true'&&
            grid()?.dataset.loadedCount==='10','waiting for the next voice page keeps current rows');
          await capture('search_voice_table_loading');
          release();
        }
        const opener=()=>grid()?.querySelector(`[data-grid-index="10"] ${category==='voices'?'.search-voice-radio':'.search-card-open'}`);
        await wait(()=>grid()?.dataset.loadedCount==='20'&&opener(),'scroll appends ten long-audio results');
        if(category==='voices'){
          const row=opener().closest('.search-voice-row');
          assert(aligned(row)&&row.getAttribute('aria-rowindex')==='12'&&all('.search-voice-row').length<20,
            'late narrow voice row keeps aligned columns, ordinal and bounded rendering');
        }
        opener().scrollIntoView({block:'nearest'});
        const rect=opener().getBoundingClientRect(), position=panel().scrollTop;
        assert(opener().contains(document.elementFromPoint(rect.x+rect.width/2,rect.y+rect.height/2)),
          'appended audio result offers a visible navigation control');
        click(opener());
        await wait(()=>one('.podcast-cover h2')?.textContent.includes('合成播客'),'appended result opens native podcast details');
        click(button('返回上一页'));
        await wait(()=>ready()&&grid()?.dataset.loadedCount==='20'&&document.activeElement===opener()&&
          Math.abs(panel().scrollTop-position)<=1,'Back restores results, scroll and exact audio result control');
        const restored=await invoke('player_snapshot',{epoch});
        assert(restored.playId===radioPlaying.playId&&restored.queueRevision===radioPlaying.queueRevision,
          'appending or opening details cannot alter the existing native queue');
        await capture('search_'+category+'_waterfall_back');
      }
      await emit('complete',{ok:true,voiceQueueVerified:true,voiceTableVerified:true,audioWaterfallVerified:true,audioReads,stageCount:count,realMusicRequests:0,realAccountWrites:0});
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
      const menu=()=>one('.action-menu[role="menu"]:not([inert])');
      const membership=()=>one('.artist-membership-dialog[open]:not([inert])');
      async function artistMenu(keyboard){
        const row=card('artist'), opener=row.querySelector('.search-card-open');
        opener.scrollIntoView({block:'nearest'});
        if(keyboard){
          opener.focus({preventScroll:true});
          opener.dispatchEvent(new KeyboardEvent('keydown',{key:'F10',shiftKey:true,bubbles:true,cancelable:true}));
        }else{
          const rect=opener.getBoundingClientRect();
          const x=rect.x+rect.width/2,y=rect.y+rect.height/2;
          const hit=document.elementFromPoint(x,y);
          assert(row.contains(hit),'artist card is hit at the context menu pointer');
          hit.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,cancelable:true,button:2,clientX:x,clientY:y}));
        }
        await wait(()=>menu()?.contains(document.activeElement),'artist context menu has focus');
        assert([...menu().querySelectorAll('button')].map(n=>n.textContent.trim()).join(',')==='查看歌手,收藏,复制链接',
          'artist menu matches the recovered actions, with playback separate');
        assert(row.querySelector('.search-card-play:not(:disabled)'),'artist avatar retains an independent play control');
        return opener;
      }
      assert(card('artist').textContent.includes('合成别名')&&!card('artist').querySelector('mark')&&
        !card('user').querySelector('mark'),'comprehensive keeps its separate people presentation');
      const comprehensiveArtist=await artistMenu(false);
      await capture('artist_search_context_menu');
      click(button('复制链接',menu()));
      await wait(()=>one('.artist-action-feedback')?.textContent.includes('链接已复制'),'search artist uses the native clipboard flow');
      click(button('关闭',one('.artist-action-feedback')));
      await wait(()=>document.activeElement===comprehensiveArtist,'copy feedback returns to the original artist card');
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
        if(kind==='artist'){
          const artist=await artistMenu(true);
          click(button('收藏',menu()));
          await wait(()=>membership()?.querySelector('.album-membership-name')?.textContent.trim()===title.textContent.trim()&&
            button('确认',membership())&&!button('确认',membership()).disabled,'artist menu opens account-bound follow confirmation');
          await capture('artist_search_collect_confirmation');
          click(button('取消',membership()));
          await wait(()=>!membership()&&document.activeElement===artist,'cancel returns to the exact category card');
        }
      }
      await emit('complete',{ok:true,peopleCategoriesVerified:true,searchArtistMenuVerified:true,stageCount:count,realMusicRequests:0,realAccountWrites:0});
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
        const card=one(`[data-result-key^="${kind}:"]`);
        const title=card?.querySelector(card.classList.contains('search-playlist-row')
          ? '.search-card-open' : '.search-result-name');
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
        } else if(kind==='mv'){
          const artist=subtitle?.querySelector('.search-card-artist');
          const artistMarks=[...(subtitle?.querySelectorAll('mark.search-match-text')??[])];
          assert(!recommendation&&artist?.textContent.trim()==='合成歌手','MV keeps the native artist credit');
          assert(artistMarks.length===(expected?1:0)&&
            (!expected||(artist.contains(artistMarks[0])&&artistMarks[0].textContent==='合成')),
            'MV marks only the matching artist credit');
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
        if(kind==='voice'){
          const row=one('.search-voice-row');
          assert(row?.querySelector('.search-voice-listens[role="cell"]')?.textContent.trim()==='1.2万'&&
            row.querySelector('.search-voice-duration[role="cell"]')&&row.querySelector('button.search-voice-radio'),
            'dedicated voice table places metrics in cells and keeps the parent link');
        }
      }
      await capture('mv_category_card_title_matches');
      await fresh('不匹配结果');
      for(const kind of ['playlist','podcast','voice','mv'])check(kind,false);
      await emit('complete',{ok:true,cardTitleMatchesVerified:true,mvArtistMatchesVerified:true,stageCount:count,realMusicRequests:0,realAccountWrites:0});
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
      const controlBest=one('.search-best-section');
      assert(controlBest&&[...controlBest.querySelectorAll('[data-result-key]')].map(node=>node.dataset.resultKey).join(',')==='artist:100,artist:101'&&
        !controlBest.classList.contains('search-best-expanded'),'v3 without expansion keeps the first two hot-core cards');
      assert(!document.querySelector('[href*="example.invalid/private"]'),'no raw action');
      assert(one('.search-track-mv')?.getAttribute('aria-label')==='播放 MV 综合 V3 合成歌曲','v3 extInfo MV projected to song badge');
      const verifyHints=()=>{
        const cards=all('.search-track-card');
        const hints=cards.map((card)=>card.querySelector('.search-track-hint'));
        assert(hints.length===3&&!hints[2]&&hints[0]?.dataset.tooltip==='🎵合成歌词片段'&&
          hints[0]?.querySelector('mark')?.textContent==='合成'&&
          hints[1]?.textContent.trim()==='前缀合成 V3别名'&&
          hints[1]?.querySelector('mark')?.textContent==='合成 V3',
          'v3 displays UTF-16 lyric match and plain alias fallback with extInfo precedence');
        const explicit=cards[0]?.querySelector('.search-track-explicit');
        assert(explicit?.textContent==='E'&&explicit.getAttribute('aria-label')==='含成人内容'&&
          Math.abs(explicit.getBoundingClientRect().width-12)<1&&
          !cards[1]?.querySelector('.search-track-explicit'),
          'v3 projects direct explicit mark and honors nonzero unflagged song over album fallback');
        assert(cards[0]?.querySelector('.search-original-tag')?.textContent==='原唱'&&
          !cards[1]?.querySelector('.search-original-tag'),
          'only numeric original recording value shows original badge');
        assert(cards.every((card)=>Math.abs(card.getBoundingClientRect().height-84)<1),'match hints keep 84px cards');
      };
      verifyHints();
      await capture('v3_comprehensive');
      await theme(760,540,'dark');
      verifyHints();
      await capture('v3_track_hints_small_dark');
      key(one('[data-category="all"]'),'ArrowRight');
      await wait(()=>ready()&&one('[data-category="mvs"]')?.getAttribute('aria-selected')==='true','dynamic arrow navigation');
      assert(tabs()==='all,mvs,songs','category preserves service order');
      await capture('v3_category');
      click(button('返回上一页'));
      await wait(()=>ready()&&one('[data-category="all"]')?.getAttribute('aria-selected')==='true','Back restores comprehensive');
      assert(tabs()==='all,mvs,songs'&&one('#search-result-panel').textContent.includes('合成热点内容'),'Back preserves search snapshot');
      await capture('v3_back');
      const tags=()=>one('.search-track-reasons');
      assert(tags()?.querySelectorAll('button').length===2&&tags()?.querySelector('span')?.textContent==='不可跳转','only supported tags navigate');
      const clickReason=async (label)=>{
        const target=button(label,tags());
        target.scrollIntoView({block:'nearest'});
        target.focus({preventScroll:true});
        await sleep(160);
        const rect=target.getBoundingClientRect();
        assert(rect.width>0&&target.contains(document.elementFromPoint(rect.x+rect.width/2,rect.y+rect.height/2)),'visible reason chip hit '+label);
        click(target);
      };
      await clickReason('合成推荐 ›');
      await wait(()=>one('.search-reason-page h1')?.textContent==='合成推荐详情'&&one('.search-reason-page .track-row')?.textContent.includes('合成推荐曲目'),'recommendation tag uses native song detail');
      await capture('v3_recommendation_tag');
      click(button('返回上一页'));
      await wait(()=>ready()&&tags()&&document.activeElement?.dataset.resultAction==='reason:0','back restores exact reason focus');
      await clickReason('合成城市 ›');
      await wait(()=>one('.search-reason-page h1')?.textContent==='合成城市详情'&&one('.search-reason-page .track-row')?.textContent.includes('合成城市曲目'),'city tag uses chart and song routes');
      const filter=one('[aria-label="搜索推荐歌曲"]');
      filter.value='不匹配';
      filter.dispatchEvent(new Event('input',{bubbles:true}));
      await wait(()=>one('.search-reason-page')?.textContent.includes('没有匹配的歌曲'),'reason page filter');
      const reasonPage=one('.search-reason-page');
      const download=()=>one('.download-dialog[open]');
      const batch=()=>one('.reason-track-batch[open]');
      const batchButton=button('批量操作',reasonPage);
      click(button('下载',reasonPage));
      await wait(()=>download()?.querySelector('.download-selection')?.textContent.includes('合成城市曲目'),
        'reason download-all opens confirmation for the source list even with a filter');
      click(button('关闭下载对话框',download()));
      await wait(()=>!download()&&document.activeElement===button('下载',reasonPage),
        'download-all cancellation restores page focus');
      click(batchButton);
      await wait(()=>batch()?.querySelector('[aria-label="选择 合成城市曲目"]'),'reason batch shows source rows');
      assert(button('播放所选歌曲',batch())?.disabled&&button('下载所选歌曲',batch())?.disabled&&
        !button('添加到歌单',batch()),'reason batch only exposes the two recovered actions');
      click(batch().querySelector('[aria-label="选择 合成城市曲目"]'));
      await wait(()=>batch()?.querySelector('footer [role="status"]')?.textContent.includes('已选 1 首'),
        'select the reason song');
      await capture('v3_reason_batch');
      click(button('下载所选歌曲',batch()));
      await wait(()=>!batch()&&download()?.querySelector('.download-selection')?.textContent.includes('合成城市曲目'),
        'reason batch hands selected track to existing confirmation');
      click(button('关闭下载对话框',download()));
      await wait(()=>!download()&&document.activeElement===batchButton,'batch download restores batch entry focus');
      filter.value='';
      filter.dispatchEvent(new Event('input',{bubbles:true}));
      await wait(()=>one('.search-reason-page .track-row'),'reason rows return');
      click(batchButton);
      await wait(()=>batch()?.querySelector('[aria-label="全选歌曲"]'),'reopen reason batch');
      click(batch().querySelector('[aria-label="全选歌曲"]'));
      await wait(()=>!button('播放所选歌曲',batch())?.disabled,'batch select all enables playback');
      click(button('播放所选歌曲',batch()));
      await wait(()=>one('.playerbar').dataset.state==='playing','reason selection starts native playback');
      const invoke=(name,args)=>window.__TAURI_INTERNALS__.invoke(name,args);
      const {epoch}=await invoke('account_snapshot');
      const player=await invoke('player_snapshot',{epoch});
      const queue=await invoke('player_queue',{epoch});
      assert(player.track?.id==='19'&&queue.tracks.length===1&&queue.tracks[0]?.track.id==='19','reason plays the displayed chart song, not a playlist ID');
      await capture('v3_city_tag');
      const currentRow=()=>one('.search-reason-page .track-row');
      const rowControl=label=>button(`${label} 合成城市曲目`,currentRow()??reasonPage);
      currentRow().scrollIntoView({block:'center'});
      const visibleControl=label=>{
        const control=rowControl(label), rect=control?.getBoundingClientRect();
        return !!rect&&rect.width>0&&control.contains(document.elementFromPoint(rect.x+rect.width/2,rect.y+rect.height/2));
      };
      await wait(()=>visibleControl('暂停')&&Number(one('[aria-label="播放进度"]')?.value)>.25,'reason row is visible during playback');
      click(rowControl('暂停'));
      await wait(()=>one('.playerbar').dataset.state==='paused'&&visibleControl('继续播放')&&document.activeElement===rowControl('继续播放'),'reason row pauses and keeps focus');
      const paused=await invoke('player_snapshot',{epoch});
      assert(paused.playId===player.playId&&paused.queueRevision===player.queueRevision&&paused.position>.25,'reason row preserves native identity and progress');
      await capture('v3_city_row_paused');
      click(rowControl('继续播放'));
      await wait(()=>one('.playerbar').dataset.state==='playing'&&visibleControl('暂停')&&document.activeElement===rowControl('暂停'),'reason row resumes and keeps focus');
      const resumed=await invoke('player_snapshot',{epoch});
      assert(resumed.playId===paused.playId&&resumed.queueRevision===paused.queueRevision&&resumed.position>=paused.position,'reason row resumes original media');
      click(button('返回上一页'));
      await wait(()=>ready()&&one('[data-category="all"]')?.getAttribute('aria-selected')==='true','Back from chart');
      await choose('songs');
      const songRows=all('#search-result-panel .track-row');
      const original=songRows[0]?.querySelector('.search-original-tag');
      assert(original?.textContent==='原唱'&&!songRows[1]?.querySelector('.search-original-tag'),
        'song category honors original vs cover');
      const originalRect=original.getBoundingClientRect();
      assert(originalRect.width>0&&original.contains(document.elementFromPoint(
        originalRect.x+originalRect.width/2,originalRect.y+originalRect.height/2)),
        'original label remains visible beside the long narrow title');
      await capture('v3_original_recording_category');

      await emit('complete',{ok:true,v3Verified:true,v3TrackHintsVerified:true,v3ExplicitVerified:true,v3ReasonRoutesVerified:true,v3ReasonPlaybackVerified:true,v3ReasonBatchVerified:true,v3ReasonCurrentRowVerified:true,v3OriginalRecordingVerified:true,stageCount:count,realMusicRequests:0,realAccountWrites:0});
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
    click(button('使用分页查看'));await wait(()=>onPage(1,0),'first short page in explicit numbered mode');
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
    await choose('playlists');click(button('加载更多'));await wait(()=>ready()&&
      one('.search-result-grid')?.dataset.loadedCount==='20','corrected term carries to the next playlist batch');
    await capture('correction_waterfall');
    click(button('仍然搜索“合成错词”'));await wait(()=>ready()&&one('[data-category="all"]').getAttribute('aria-selected')==='true'&&!one('.search-correction')&&one('.search-result-panel').textContent.includes('按原词搜索结果'),'original term sent with correction disabled');
    await capture('correction_disabled');
    click(button('返回上一页'));await wait(()=>ready()&&one('[data-category="playlists"]').getAttribute('aria-selected')==='true'&&one('.search-correction')?.textContent.includes('合成正词')&&one('.search-result-grid')?.dataset.loadedCount==='20','back restores corrected route and its appended results');
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
    const native=(command,args)=>window.__TAURI_INTERNALS__.invoke(command,args);
    const {epoch: playbackEpoch}=await native('account_snapshot');
    async function toggleCurrentRow(id,label) {
      const row=()=>one(`.track-row[data-track-id="${id}"] .track-play`);
      await wait(()=>row()?.getAttribute('aria-label').startsWith('暂停 ')&&!row().disabled&&Number(one('.playerbar').dataset.position)>.2,'current result row offers pause');
      row().scrollIntoView({block:'center'});
      await wait(()=>{
        const target=row();
        if(!target) return false;
        const rect=target.getBoundingClientRect();
        return target.contains(document.elementFromPoint(rect.x+rect.width/2,rect.y+rect.height/2));
      },'current result row is visibly reachable');
      const before=await native('player_snapshot',{epoch:playbackEpoch});
      click(row());
      await wait(()=>row()?.getAttribute('aria-label').startsWith('继续播放 ')&&!row().disabled&&document.activeElement===row(),'current result row pauses and retains focus');
      const paused=await native('player_snapshot',{epoch:playbackEpoch});
      assert(paused.state==='paused'&&paused.playId===before.playId&&paused.queueRevision===before.queueRevision&&paused.position>=before.position-.1,'result row pause retains native selection and position');
      await capture(label);
      click(row());
      await wait(()=>playingTrack(id)&&!row().disabled,'current result row resumes');
      const resumed=await native('player_snapshot',{epoch:playbackEpoch});
      assert(resumed.playId===before.playId&&resumed.position>=paused.position-.1,'result row resume retains native selection and position');
    }
    await fixture('result_play_default');click(button('播放全部搜索单曲'));
    await wait(()=>playingTrack(1),'comprehensive header play uses default trial');
    await fixture('result_play_trial');click(all('.search-track-cover')[1]);
    await wait(()=>playingTrack(2),'individual preview uses search trial');
    await choose('songs');click(button('播放全部'));
    await wait(()=>playingTrack(1),'song tab play all uses search trial');await capture('play');
    await choose('lyrics');click(one('.track-row[data-track-id="3"] .track-play'));
    await wait(()=>playingTrack(3),'lyric row uses search trial');
    await toggleCurrentRow(3,'current_lyric_paused');
    await choose('songs');click(one('.track-row[data-track-id="2"] .track-play'));
    await wait(()=>playingTrack(2),'song row uses search trial');
    await toggleCurrentRow(2,'current_song_paused');
    await fresh('合成搜索受限');
    await wait(()=>one('#search-result-panel .empty-state p')?.textContent==='部分结果暂不可展示'&&!button('重新加载'),'service explanation without generic retry');
    await capture('service_empty');
    await fresh('合成搜索结果');
    await wait(()=>one('.search-result-section')&&!one('#search-result-panel')?.textContent.includes('部分结果暂不可展示'),'next search clears explanation');
    await fixture('expired');click(one('[data-category="albums"]'));await wait(()=>button('重新登录')&&!one('.search-results-page'),'expired account');await capture('expired');
    await emit('complete',{ok:true,stageCount:count,responseTimingInjected:true,shortPageOffsetsVerified:true,searchResultPlaybackVerified:true,searchCurrentRowsVerified:true,searchMatchHighlightVerified:true,searchServiceEmptyVerified:true,searchCategoryWithoutFramesVerified:true,searchKeywordRaceVerified:true,searchProfileWithoutFramesVerified:profileWithoutFrames,realMusicRequests:0,realAccountWrites:0});
  }catch(error){await emit('failed',{step,message:String(error),stack:error?.stack,stageCount:count,body:document.body.innerText.slice(0,3500)});}
})();
