// Official comment resource cards, synthetic metadata and actual libmpv/null audio.
(async () => {
  const invoke = (name, args = {}) =>
    window.__TAURI_INTERNALS__.invoke(name, args);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (value, label) => {
    if (!value) throw Error(label);
  };
  const all = (selector, root = document) =>
    [...root.querySelectorAll(selector)].filter(
      (node) => !node.closest(".page-leave-active,.catalog-tab-leave-active"),
    );
  const one = (selector) => all(selector)[0];
  const button = (label, root = document) =>
    all("button", root).find(
      (node) =>
        node.textContent.trim() === label ||
        node.getAttribute("aria-label") === label,
    );
  const emit = (stage, value = {}) =>
    invoke("plugin:event|emit", {
      event: "ux-proof",
      payload: { stage, ...value },
    });
  const source = Object.getOwnPropertyDescriptor(
    HTMLImageElement.prototype,
    "src",
  );
  Object.defineProperty(HTMLImageElement.prototype, "src", {
    ...source,
    set(value) {
      source.set.call(
        this,
        String(value).startsWith("https://")
          ? String(value).includes("synthetic-missing")
            ? "data:image/png;base64,AAAA"
            : "data:image/svg+xml," +
              encodeURIComponent(
                '<svg xmlns="http://www.w3.org/2000/svg" width="320" height="320"><rect width="320" height="320" fill="#66528f"/><circle cx="160" cy="160" r="95" fill="#ce9aac"/></svg>',
              )
          : value,
      );
    },
  });
  window.__commentReadAck = {};
  let epoch,
    count = 0,
    command = 0,
    last = "boot";
  async function wait(test, label) {
    for (let i = 0; i < 600; i++) {
      if (await test()) return;
      await sleep(40);
    }
    throw Error(label);
  }
  async function settle() {
    await wait(async () => {
      await new Promise((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(resolve)),
      );
      return !document.getAnimations().some((animation) => {
        const end = animation.effect?.getComputedTiming().endTime;
        return (
          Number.isFinite(end) &&
          (animation.pending || animation.playState === "running")
        );
      });
    }, "finite transitions");
  }
  function bounds(node) {
    if (!(node instanceof HTMLElement) || !node.isConnected) return null;
    const rect = node.getBoundingClientRect();
    let left = Math.max(0, rect.left),
      right = Math.min(innerWidth, rect.right),
      top = Math.max(0, rect.top),
      bottom = Math.min(innerHeight, rect.bottom);
    for (let parent = node; parent; parent = parent.parentElement) {
      const style = getComputedStyle(parent),
        box = parent.getBoundingClientRect();
      if (
        style.display === "none" ||
        style.visibility === "hidden" ||
        Number(style.opacity) === 0
      )
        return null;
      if (["auto", "scroll", "hidden", "clip"].includes(style.overflowX)) {
        left = Math.max(left, box.left);
        right = Math.min(right, box.right);
      }
      if (["auto", "scroll", "hidden", "clip"].includes(style.overflowY)) {
        top = Math.max(top, box.top);
        bottom = Math.min(bottom, box.bottom);
      }
    }
    return right - left > 1 && bottom - top > 1
      ? { left, right, top, bottom }
      : null;
  }
  function click(node) {
    assert(node?.isConnected && !node.disabled, "enabled control");
    if (!bounds(node)) node.scrollIntoView({ block: "nearest" });
    node.focus({ preventScroll: true });
    node.click();
  }
  async function mode(value) {
    const id = ++command;
    await emit("comment-read-fixture", { id, mode: value });
    await wait(() => window.__commentReadAck[id], "fixture ACK");
    return window.__commentReadAck[id];
  }
  async function theme(value) {
    localStorage.setItem("cumulus.appearance.v1", value);
    dispatchEvent(
      new StorageEvent("storage", {
        key: "cumulus.appearance.v1",
        newValue: value,
      }),
    );
    await wait(() => document.documentElement.dataset.theme === value, "theme");
    await settle();
  }
  async function resize(width, height) {
    await emit("resize", { width, height, scale: devicePixelRatio });
    await wait(
      () => innerWidth === width && innerHeight === height,
      "CSS viewport",
    );
    await settle();
  }
  const header=()=>one('.comment-resource-header');
  const page=()=>one('.comments-page');
  const play=()=>one('.comment-resource-play');
  const name=()=>one('[data-comment-resource-name]');
  const snapshot=()=>invoke('player_snapshot',{epoch});
  function input(label,value){const node=one(`[aria-label="${label}"]`);assert(node,'input '+label);node.value=value;node.dispatchEvent(new Event('input',{bubbles:true}));}
  async function card(kind,id){await wait(()=>header()?.dataset.resourceKind===kind&&header()?.dataset.resourceId===id&&header().getAttribute('aria-busy')==='false'&&page()?.getAttribute('aria-busy')==='false'&&one('.comment-resource-type'),'resource '+kind);await settle();name().focus();}
  async function comments(id='1') { const row=all('.track-row').find(row=>row.dataset.trackId===id)??all('.track-row')[Number(id)-1];assert(row,'song '+id);click(row.querySelector('.track-more'));await wait(()=>one('[role="menu"]'),'menu');click(button('查看评论',one('[role="menu"]')));await wait(()=>page()?.getAttribute('aria-busy')==='false','comment list');await settle(); }
  async function native(state,kind,id){await wait(async()=>{const v=await snapshot();return v.state===state&&v.resourceKind===kind&&v.track?.id===id&&one('.playerbar')?.dataset.state===state&&!play()?.disabled&&(state!=='playing'||v.position>.2);},'native '+state);await settle();return snapshot();}
  async function capture(label,extra={}){
    last=label;await settle();const active=document.activeElement;
    const controls=all('button',header()??page()??document).filter(node=>bounds(node)&&!node.disabled).map(node=>{const r=bounds(node);const x=r.left+Math.min(6,(r.right-r.left)/2),y=(r.top+r.bottom)/2;const hit=document.elementFromPoint(x,y);return {label:node.getAttribute('aria-label')??node.textContent.trim(),hit:node===hit||node.contains(hit)};});
    const value={visible:document.visibilityState==='visible',focused:document.hasFocus(),focusValid:active!==document.body&&!!bounds(active),active:active?.outerHTML?.slice(0,600),width:innerWidth,height:innerHeight,ratio:devicePixelRatio,captureTitle:'Cumulus',theme:document.documentElement.dataset.theme,countFont:page()?.querySelector('h1')?getComputedStyle(page().querySelector('h1')).fontSize:null,controls,overflow:document.documentElement.scrollWidth>innerWidth||!!page()&&page().scrollWidth>page().clientWidth+1,mvTitleOverflow:!!one('.mv-detail:not(.mv-mini) .mv-info')&&one('.mv-detail:not(.mv-mini) .mv-info').scrollWidth>one('.mv-detail:not(.mv-mini) .mv-info').clientWidth+1,cards:all('.comment-card').length,kind:header()?.dataset.resourceKind??null,resourceId:header()?.dataset.resourceId??null,title:name()?.textContent,coverWidth:one('.comment-resource-cover')?.getBoundingClientRect().width??null,coverHeight:one('.comment-resource-cover')?.getBoundingClientRect().height??null,metadataError:one('.comment-resource-error')?.textContent??'',cached:one('.comment-resource-header .form-warning')?.textContent??'',draft:one('[aria-label="评论内容"]')?.value??null,playback:await snapshot(),...extra};
    if(!extra.diagnostic){assert(value.visible&&value.focused&&value.focusValid&&!value.overflow,'visible focused resource card');assert(controls.every(c=>c.hit),'resource controls hit');assert(value.cards<=64,'bounded comments');assert(!value.mvTitleOverflow,'MV title remains within content bounds');}
    const stage=`ux_${String(++count).padStart(2,'0')}_${label}`;window.__CUMULUS_ALBUM_CAPTURE_ACK__=null;await emit(stage,value);await wait(()=>window.__CUMULUS_ALBUM_CAPTURE_ACK__===stage,'capture ACK');
  }
  try {
    await wait(()=>one('.track-row'),'startup');epoch=(await invoke('account_snapshot')).epoch;await resize(1120,760);await theme('light');await comments();await card('track','1');await capture('track_initial_light');
    click(play());await native('playing','catalog','1');await capture('track_playing_light');await theme('dark');click(play());const paused=await native('paused','catalog','1');await capture('track_paused_dark');click(play());const resumed=await native('playing','catalog','1');assert(paused.playId===resumed.playId&&paused.entryId===resumed.entryId&&paused.queueRevision===resumed.queueRevision,'track resume keeps identity');await capture('track_resumed_dark');
    click(button('停止播放',one('.playerbar')));await wait(async()=>(await snapshot()).state==='idle','stop');await capture('track_stopped_dark');click(play());await native('playing','catalog','1');await capture('track_restarted_dark');
    click(button('合成专辑 · A long album name',header()));await wait(()=>one('.album-header'),'album navigation');await settle();await capture('album_link_dark');click(button('返回上一页'));await card('track','1');await capture('album_return_dark');
    await theme('light');click(button('测试歌手',header()));await wait(()=>one('.artist-header'),'artist navigation');await settle();await capture('artist_link_light');click(button('返回上一页'));await card('track','1');await capture('artist_return_light');
    click(button('每日推荐'));await wait(()=>one('.track-row')&&!header(),'daily');await mode('resource-offline');await comments();await wait(()=>one('.comment-resource-error')&&one('.comment-resource-header .form-warning'),'cached metadata');name().focus();await capture('resource_cached_failure_light');
    input('评论内容','合成草稿：资源重试不得修改');await mode('none');click(button('重试资源信息',header()));await wait(()=>header()?.getAttribute('aria-busy')==='false'&&!one('.comment-resource-error'),'metadata retry');await settle();assert(one('[aria-label="评论内容"]').value==='合成草稿：资源重试不得修改','metadata retry preserves draft');await capture('resource_cache_retry_light');
    click(button('每日推荐'));await wait(()=>one('.track-row')&&!header(),'daily2');await mode('resource-offline');await comments('2');await wait(()=>one('.comment-resource-error')&&!play(),'empty metadata failure');name().focus();await capture('resource_empty_failure_light');await mode('none');click(button('重试资源信息',header()));await card('track','2');await wait(()=>one('.comment-resource-cover > .icon'),'bad cover fallback');await capture('resource_empty_retry_light');
    click(button('每日推荐'));await wait(()=>one('.track-row')&&!header(),'held origin');await theme('dark');await mode('hold-resource');await comments('2');await wait(async()=>(await mode('read')).pending,'metadata inflight');name().focus();await capture('held_resource_dark');click(button('每日推荐'));await wait(()=>one('.track-row')&&!header(),'leave held resource');await capture('left_resource_dark');await mode('none');await wait(async()=>(await mode('read')).delayedCompleted===1,'late metadata released');assert(!header(),'late metadata cannot reopen page');await capture('late_resource_dark');
    click(button('我的播客'));await wait(()=>one('[aria-label="播客编号或链接"]'),'podcast page');input('播客编号或链接','71');await wait(()=>!button('打开播客').disabled,'radio input');click(button('打开播客'));await wait(()=>one('[data-voice-id="81"]'),'radio episodes');click(one('[data-voice-id="81"] [aria-label="单集评论"]'));await card('voice','81');await capture('voice_header_dark');click(play());const voice=await native('playing','voice','181');assert(voice.track.voiceId==='81','program id distinct from song id');await capture('voice_playing_dark');click(play());const vp=await native('paused','voice','181');assert(vp.playId===voice.playId,'voice pause keeps identity');await capture('voice_paused_dark');click(button('停止播放',one('.playerbar')));await wait(async()=>(await snapshot()).state==='idle','voice stop');click(play());await native('playing','voice','181');await capture('voice_restarted_dark');
    const beforeCreator = await snapshot();
    click(button('合成主播',header()));
    await wait(()=>one('.playlist-creator-page h1')?.textContent==='合成主播主页','host account profile');
    await settle();
    click(button('返回上一页'));await card('voice','81');
    const afterCreator = await snapshot();
    assert(afterCreator.playId===beforeCreator.playId && afterCreator.queueRevision===beforeCreator.queueRevision,'host profile navigation preserves playback');
    await capture('creator_return_dark');
    click(button('合成播单 · A radio with a long title',header()));await wait(()=>one('.podcast-page [data-voice-id="81"]'),'radio link');await settle();await capture('radio_link_dark');click(button('返回上一页'));await card('voice','81');await capture('radio_return_dark');await resize(760,540);await theme('light');name().focus();await capture('small_voice_light');
    click(button('收藏的 MV'));await wait(()=>one('[aria-label="MV编号或链接"]'),'MV list');input('MV编号或链接','123');await settle();click(button('打开 MV'));await wait(()=>one('.mv-detail')&&button('MV 评论')&&!button('MV 评论').disabled,'MV metadata');click(button('MV 评论'));await card('mv','123');await capture('mv_mini_error_light');const mini=one('.mv-mini');assert(mini,'failed video retained for explicit dismissal');click(button('关闭视频',mini));await wait(()=>!one('.mv-mini'),'dismiss failed mini video');name().focus();await capture('mv_header_light');await theme('dark');play().focus();await capture('small_mv_dark');click(play());await wait(()=>one('.mv-detail')&&!header(),'MV destination');await settle();await capture('mv_link_dark');
    await emit('complete',{ok:true,commentResources:true,stageCount:count,realAccountWrites:0,physicalAudioOutputVerified:false});
  }catch(error){const failedAt=last;await capture('diagnostic',{diagnostic:true}).catch(()=>{});await mode('none').catch(()=>{});await emit('failed',{ok:false,commentResources:true,error:String(error),stack:error?.stack,last:failedAt,body:document.body.innerText.slice(-3200),active:document.activeElement?.outerHTML?.slice(0,600)});}
})();
