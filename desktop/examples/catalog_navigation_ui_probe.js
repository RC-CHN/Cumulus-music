// Real production navigation, IPC and native playback; synthetic account/services.
(async () => {
  const one = s => document.querySelector(s), all = s => [...document.querySelectorAll(s)];
  const sleep = ms => new Promise(done => setTimeout(done, ms));
  const assert = (ok, message) => { if (!ok) throw Error(message); };
  const emit = (stage, fields = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', { event:'ux-proof', payload:{stage,...fields} });
  const wait = async (fn, message) => { for (let i=0;i<220;i++) { if (fn()) return; await sleep(70); } throw Error(message); };
  const button = (label, scope=document) => [...scope.querySelectorAll('button')].find(n => n.getAttribute('aria-label') === label || n.textContent.trim() === label);
  const click = n => { assert(n&&!n.disabled, 'unavailable control '+n?.outerHTML); n.scrollIntoView({block:'nearest',behavior:'instant'}); n.focus({preventScroll:true}); n.click(); };
  const settled = () => !one('.page-enter-active,.page-leave-active,.catalog-tab-enter-active,.catalog-tab-leave-active,.menu-enter-active,.menu-leave-active');
  const frames = () => new Promise(done => requestAnimationFrame(() => requestAnimationFrame(done)));
  const artistTab = key => one(`[data-catalog-tab="${key}"]`);
  const albumTab = key => one(`.album-tabs [id$="-${key}"]`);
  const pick = async key => { click(artistTab(key)); await wait(()=>artistTab(key)?.getAttribute('aria-selected')==='true'&&settled(),'artist tab '+key); };
  const back = async () => { click(one('.back-button')); await wait(settled,'back settled'); };
  const mode = async value => { await emit('fixture',{mode:value}); await wait(()=>window.__CUMULUS_ALBUM_MODE__===value,'fixture acknowledgement'); };
  const selectedArtist = () => one('.artist-tabs [aria-selected="true"]')?.dataset.catalogTab;
  const playing = () => one('.playerbar')?.dataset.state === 'playing' && Number(one('.playerbar')?.dataset.position)>.2;
  let count=0, lastResize=performance.now();
  window.addEventListener('resize',()=>{lastResize=performance.now();});
  function samples() {
    const page=one('.catalog-page'),grid=one('.catalog-card-scroll');
    if (!page||!grid) return [];
    const outer=page.getBoundingClientRect(),bounds=grid.getBoundingClientRect();
    return all('.catalog-card:not(.artist) .catalog-card-placeholder').flatMap(node=>{
      const r=node.getBoundingClientRect(),x=r.left+12,y=Math.max(r.top+12,bounds.top+12,outer.top+12);
      return x<bounds.right&&y<Math.min(r.bottom,bounds.bottom,outer.bottom)-12
        ? [{x,y,color:getComputedStyle(node).backgroundColor}]:[];
    }).slice(0,2);
  }
  function focusSamples() {
    const node=document.activeElement;
    if(!(node instanceof HTMLElement)||!node.matches(':focus-visible')
      ||!node.matches('.catalog-card,.catalog-tabs button'))return [];
    const r=node.getBoundingClientRect(),style=getComputedStyle(node);
    const point=node.matches('.catalog-card') ? {x:r.right-1,y:r.top+r.height/2}
      : {x:r.left+r.width/2,y:r.top+1};
    assert(point.x>=0&&point.y>=0&&point.x<innerWidth&&point.y<one('.playerbar').getBoundingClientRect().top,'focus outline sample outside view');
    return [{...point,color:style.outlineColor}];
  }
  async function capture(name, active=false) {
    await wait(settled,'capture transition');await sleep(180);
    assert(document.documentElement.scrollWidth<=innerWidth,'document overflow');
    assert(all('.track-row').length<=30&&all('.catalog-card').length<=30,'unbounded catalog');
    const page=one('.catalog-page');
    if(page) assert(page.getBoundingClientRect().bottom<=one('.playerbar').getBoundingClientRect().top+1,'page overlaps player');
    const stage=`ux_${String(++count).padStart(2,'0')}_${name}`;
    window.__CUMULUS_ALBUM_CAPTURE_ACK__='';
    await emit(stage,{width:innerWidth,height:innerHeight,ratio:devicePixelRatio,screenX,screenY,
      theme:document.documentElement.dataset.theme,visible:document.visibilityState==='visible',focused:document.hasFocus(),
      title:one('.catalog-heading h1')?.textContent,artistTab:selectedArtist(),
      albumTab:one('.album-tabs [aria-selected="true"]')?.id.split('-').at(-1),
      listScroll:one('.track-scroll,.catalog-card-scroll')?.scrollTop??0,outerScroll:page?.scrollTop??0,
      albumTotal:Number(one('.catalog-card-scroll')?.dataset.catalogTotal??0),
      focusedCard:document.activeElement?.dataset?.catalogKey,paintSamples:[...samples(),...focusSamples()],
      updating:!!one('[data-catalog-updating]'),
      followDisabled:one('.artist-actions button[aria-pressed]')?.disabled,
      cardTop:one('[data-catalog-key="album:46"]')?.getBoundingClientRect().top,
      commentIds:all('[data-group="page"] .comment-card').map(n=>n.dataset.commentId),
      draft:one('[aria-label="评论内容"]')?.value??'',locator:one('[aria-label="评论编号"]')?.value??'',
      playing:active,expectedQueue:active?50:0});
    await wait(()=>window.__CUMULUS_ALBUM_CAPTURE_ACK__===stage,'capture confirmation');
  }
  async function albumsRestored(top) {
    await wait(()=>selectedArtist()==='albums'&&one('.catalog-card-scroll')?.dataset.catalogTotal==='40'
      &&Math.abs(one('.catalog-card-scroll').scrollTop-top)<1&&settled(),'artist albums restored');
  }
  async function allSongs() {
    click(button('查看全部歌曲 →'));await wait(()=>one('.artist-all-title')&&settled(),'all songs');
  }
  try {
    await wait(()=>one('.track-credit-links button'),'bootstrap');
    for(const [width,height,color] of [[1120,760,'light'],[760,540,'light'],[760,540,'dark'],[1120,760,'dark']]) {
      await emit('resize',{width,height});await wait(()=>innerWidth===width&&innerHeight===height&&performance.now()-lastResize>=500,'resize settled');
      click(one('[aria-label="外观主题"]'));await wait(()=>one('.action-menu'),'theme menu');click(button(color==='dark'?'深色':'浅色'));
      await wait(()=>document.documentElement.dataset.theme===color&&!one('.action-menu'),'theme applied');
      const suffix=`${width}_${color}`;
      click(button('每日推荐'));await frames();await wait(()=>one('.track-credit-links button')&&settled(),'daily');
      click(one('.track-credit-links button'));await wait(()=>one('.artist-header')&&settled(),'artist');
      await pick('albums');await wait(()=>one('.catalog-card')&&button('加载更多'),'album page');
      click(button('加载更多'));await wait(()=>one('.catalog-card-scroll')?.dataset.catalogTotal==='40','album continuation');
      let grid=one('.catalog-card-scroll');grid.scrollTop=grid.scrollHeight;grid.dispatchEvent(new Event('scroll'));
      await wait(()=>all('.catalog-card strong').at(-1)?.textContent==='合成专辑 46','album last');
      const albumTop=grid.scrollTop;await capture(`albums40_${suffix}`);
      click(one('[data-catalog-key="album:46"]'));await wait(()=>one('.album-header')&&settled(),'album child');
      click(albumTab('details'));await wait(()=>one('.album-description')&&settled(),'album detail tab');await capture(`album_details_${suffix}`);
      await mode('catalog-hold');await back();await albumsRestored(albumTop);
      await wait(()=>document.activeElement?.dataset.catalogKey==='album:46','resource back focus restored after virtual render');
      assert(document.activeElement?.dataset.catalogKey==='album:46','resource back focus');await capture(`artist_return_${suffix}`);
      assert(one('[data-catalog-updating]'),'header read was not held');
      assert(one('.artist-actions button[aria-pressed]')?.disabled,'stale follow state remained writable');
      const heldCardTop=one('[data-catalog-key="album:46"]').getBoundingClientRect().top;
      await mode('normal');await wait(()=>!one('[data-catalog-updating]'),'header revalidation ended');
      assert(Math.abs(one('[data-catalog-key="album:46"]').getBoundingClientRect().top-heldCardTop)<1,'header revalidation shifted reading position');
      click(button('播放全部',one('.artist-actions')));await wait(playing,'native artist play');await capture(`playing_${suffix}`,true);
      await pick('intro');await wait(()=>one('.catalog-description h2'),'introduction');await capture(`intro_${suffix}`,true);
      await back();await albumsRestored(albumTop);await capture(`history_albums_${suffix}`,true);
      await back();await wait(()=>selectedArtist()==='songs'&&one('.track-row')&&settled(),'artist tab history songs');await capture(`history_songs_${suffix}`,true);
      await allSongs();
      while(button('加载更多')) { const n=one('.catalog-track-actions .muted').textContent;click(button('加载更多'));await wait(()=>one('.catalog-track-actions .muted').textContent!==n,'songs continuation'); }
      assert(one('.catalog-track-actions .muted').textContent.includes('55 首'),'loaded songs prefix');
      const songs=one('.track-scroll');songs.scrollTop=songs.scrollHeight;songs.dispatchEvent(new Event('scroll'));await sleep(100);
      const songTop=songs.scrollTop;await capture(`all55_${suffix}`,true);
      await back();await wait(()=>one('.artist-header')&&settled(),'all history back');await allSongs();
      await wait(()=>one('.catalog-track-actions .muted')?.textContent.includes('55 首')&&Math.abs(one('.track-scroll').scrollTop-songTop)<1,'all prefix/position');await capture(`all_return_${suffix}`,true);
      click(button('返回热门歌曲'));await wait(()=>selectedArtist()==='songs'&&one('.track-album-link')&&settled(),'explicit all back');
      click(one('.track-album-link'));await wait(()=>one('.album-header')&&settled(),'album comments resource');
      click(albumTab('comments'));await wait(()=>all('[data-group="page"] .comment-card').length===20&&settled(),'comments page');
      const text=`导航草稿 ${suffix}`,draft=one('[aria-label="评论内容"]'),locator=one('[aria-label="评论编号"]');
      draft.value=text;draft.dispatchEvent(new Event('input',{bubbles:true}));locator.value='101';locator.dispatchEvent(new Event('input',{bubbles:true}));
      click(button('下一页',one('.comment-pagination')));await wait(()=>one('[data-group="page"] .comment-card')?.dataset.commentId==='21','comment page two');
      const page=one('.catalog-page');page.scrollTop=page.scrollHeight;page.dispatchEvent(new Event('scroll'));const commentTop=page.scrollTop;
      await capture(`comments_before_${suffix}`,true);
      click(button('每日推荐'));await wait(()=>one('.track-credit-links button')&&settled(),'leave comments');await capture(`comments_daily_${suffix}`,true);
      await back();await wait(()=>one('.comments-embedded')&&one('[data-group="page"] .comment-card')?.dataset.commentId==='21'&&settled(),'return comments');
      await wait(()=>Math.abs(one('.catalog-page').scrollTop-commentTop)<1,'comment viewport restored');
      assert(one('[aria-label="评论内容"]').value===text&&one('[aria-label="评论编号"]').value==='101','comment draft/locator lost');
      await capture(`comments_back_${suffix}`,true);
      one('[aria-label="评论内容"]').scrollIntoView({block:'center',behavior:'instant'});await capture(`comments_visible_${suffix}`,true);
      click(one('[aria-label="停止播放"]'));await wait(()=>!playing(),'stop');
    }
    await emit('complete',{ok:true,stageCount:count,navigation:true,realMusicRequests:0,realAccountWrites:0,
      playbackVerified:true,decoder:'libmpv',audioOutput:'null'});
  } catch(error) { await emit('failed',{ok:false,stageCount:count,message:String(error),stack:error.stack,
    artistTab:selectedArtist(),title:one('.catalog-heading h1')?.textContent,
    scroll:one('.track-scroll,.catalog-card-scroll')?.scrollTop,outer:one('.catalog-page')?.scrollTop}); }
})();
