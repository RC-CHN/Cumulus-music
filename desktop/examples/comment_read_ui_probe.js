// Production comment reads, synthetic APIs/artwork and native libmpv/null audio.
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
  const page = () => one('.comments-page');
  const floor = () => one('.comment-floor-panel');
  const rows = () => all('[data-group="page"] .comment-card');
  const ids = () => rows().map(node => node.dataset.commentId);
  const floorIds = () => all('[data-floor-group="replies"] .comment-card').map(node => node.dataset.commentId);
  const ready = () => page() && page().getAttribute('aria-busy') === 'false';
  const floorReady = () => floor()?.getAttribute('aria-busy') === 'false';
  const pagination = () => one('.comment-pagination');
  function input(label, value) { const node=one(`[aria-label="${label}"]`);assert(node,'input '+label);node.value=value;node.dispatchEvent(new Event('input',{bubbles:true})); }
  async function comments() {
    click(one('.track-row .track-more'));await wait(()=>one('[role="menu"]'),'song menu');
    click(button('查看评论',one('[role="menu"]')));await wait(()=>ready()&&ids()[0]==='101','comments first page');await settle();
  }
  async function locate(value) {
    input('评论编号',value);await wait(()=>!button('定位评论',page())?.disabled,'location input committed');click(button('定位评论',page()));await wait(ready,'location settled');await settle();
  }
  async function read(buttonNode,test,label) {click(buttonNode);await wait(test,label);await settle();}
  async function openFloor(id=101) {
    const row=rows().find(node=>node.dataset.commentId===String(id));assert(row,'floor source');
    click(button('查看回复',row));await wait(floorReady,'floor finished');await settle();
  }
  function fullControl(node) {const b=bounds(node),r=node?.getBoundingClientRect();return !!b&&b.bottom-b.top>=r.height-1&&b.right-b.left>=r.width-1;}
  async function capture(label, extra={}) {
    last=label;await settle();
    const dialog=one('dialog[open]'),root=dialog??floor()??page()??one('.welcome')??one('.page-view');
    const active=document.activeElement,paint=dialog??floor()??one('.playerbar'),box=paint.getBoundingClientRect(),paintBox=bounds(paint);
    const controls=all('button',root).filter(node=>!node.disabled&&bounds(node)).map(node=>{
      const b=bounds(node),hit=document.elementFromPoint((b.left+b.right)/2,(b.top+b.bottom)/2);
      return {label:node.getAttribute('aria-label')??node.textContent.trim(),hit:node===hit||node.contains(hit)};
    });
    const scroll=page();
    const value={width:innerWidth,height:innerHeight,ratio:devicePixelRatio,screenX,screenY,captureTitle:'Cumulus',visible:document.visibilityState==='visible',focused:document.hasFocus(),focusValid:active!==document.body&&!!bounds(active),focusTag:active?.tagName,focusLabel:active?.getAttribute('aria-label')??active?.textContent.trim().slice(0,100),focusSort:active?.dataset?.commentSort??null,focusComment:active?.closest?.('.comment-card')?.dataset.commentId??null,
      theme:document.documentElement.dataset.theme,motion:document.documentElement.dataset.motion,
      overflow:document.documentElement.scrollWidth>innerWidth||!!scroll&&scroll.scrollWidth>scroll.clientWidth+1,cards:all('.comment-card').length,controls,
      ids:ids(),visibleIds:rows().filter(node=>bounds(node)).map(node=>node.dataset.commentId),floorIds:floorIds(),floorVisibleIds:all('[data-floor-group="replies"] .comment-card').filter(node=>bounds(node)).map(node=>node.dataset.commentId),parent:floor()?.dataset.parentId??null,floorCurrent:one('[data-floor-group="current"] .comment-card')?.dataset.commentId??null,
      sort:one('.comments-tabs [aria-selected="true"]')?.dataset.commentSort??null,scrollTop:scroll?.scrollTop??null,pagination:pagination()?.textContent.trim()??'',
      error:one('.comment-read-error')?.textContent.trim()??'',floorError:one('.comment-floor-panel .error-state')?.textContent.trim()??'',cacheWarnings:all('.form-warning').map(node=>node.textContent.trim()),draft:one('[aria-label="评论内容"]')?.value??null,
      paintSamples:[{x:Math.max(box.left+4,paintBox.left+2),y:(paintBox.top+paintBox.bottom)/2,color:getComputedStyle(paint).backgroundColor}],...extra};
    if(!extra.diagnostic){window.__commentReadBeforeCheck={...value,active:active?.outerHTML?.slice(0,700),busy:page()?.getAttribute('aria-busy')};assert(value.visible&&value.focused&&value.focusValid&&!value.overflow,'visible focused comment view');assert(controls.every(control=>control.hit),'comment controls actually hit');assert(value.cards<=64,'bounded comment pages');}
    const stage=`ux_${String(++count).padStart(2,'0')}_${label}`;window.__CUMULUS_ALBUM_CAPTURE_ACK__=null;
    await emit(stage,value);await wait(()=>window.__CUMULUS_ALBUM_CAPTURE_ACK__===stage,'capture ACK');
  }
  try {
    await wait(()=>one('.track-row'),'startup');await resize(1120,760);await theme('light');await comments();
    input('评论内容','合成草稿：只检查保留，不发送。');one('[aria-label="评论内容"]').focus();
    await wait(()=>one('[data-group="page"] [data-comment-id="101"] .comment-avatar:not(img)'),'avatar fallback');await capture('initial_light');
    await mode('fail-next');await read(button('下一页',pagination()),()=>ready()&&one('.comment-read-error'),'next failed');assert(ids()[0]==='101','failed next retains page');assert(fullControl(button('重试',pagination())),'pagination error retry fully visible');await capture('next_failed_light');
    await mode('none');await read(button('重试',pagination()),()=>ready()&&ids()[0]==='121','retry exact next');await capture('next_retry_light');
    await read(button('上一页',pagination()),()=>ready()&&ids()[0]==='101','back first');await capture('back_light');
    await mode('fail-next');await read(button('下一页',pagination()),()=>ready()&&ids()[0]==='121'&&one('.comment-read-error'),'cached navigation');const cachedRetry=button('重试',pagination());cachedRetry.scrollIntoView({block:'nearest'});cachedRetry.focus({preventScroll:true});await capture('cached_next_failure_light');
    await mode('none');await read(button('重试',pagination()),()=>ready()&&!one('.comment-read-error'),'refresh cached target');assert(page().scrollTop>0&&rows().some(node=>node.dataset.commentId==='140'&&bounds(node)),'cached refresh retains bottom reading');await capture('cached_next_retry_light');
    await read(button('上一页',pagination()),()=>ready()&&ids()[0]==='101','single back');assert(button('上一页',pagination()).disabled,'cache retry did not duplicate history');await capture('cache_return_light');
    await theme('dark');await mode('fail-hot');await read(one('[data-comment-sort="hot"]'),()=>ready()&&one('.comment-read-error'),'hot failed');assert(one('[data-comment-sort="latest"]').getAttribute('aria-selected')==='true','failed sort retains selection');await capture('hot_failed_dark');
    await mode('none');await read(button('重试',one('.comment-read-error')),()=>ready()&&ids()[0]==='301','hot retry');await capture('hot_retry_dark');
    const hot=one('[data-comment-sort="hot"]');hot.focus();hot.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowLeft',bubbles:true,cancelable:true}));await wait(()=>ready()&&ids()[0]==='101','keyboard latest');await settle();assert(document.activeElement?.dataset.commentSort==='latest','keyboard selected tab focus');await capture('keyboard_latest_dark');
    await locate('invalid');assert(one('.comment-read-error')&&!button('重试',one('.comment-read-error')),'invalid ID is editable not a queued request');await capture('invalid_target_dark');
    await mode('fail-target');await locate('900719925474099312345');assert(one('.comment-read-error'),'target read error');await capture('target_failed_dark');
    await mode('none');await read(button('重试',one('.comment-read-error')),()=>ready()&&one('[data-group="current"] [data-comment-id="900719925474099312345"]'),'target retry');await capture('target_retry_dark');
    await mode('missing-target');await locate('777');assert(one('.comment-target-missing')&&ids().length===20,'missing target explicit with ordinary page preserved');await capture('missing_target_dark');
    await mode('empty');await read(one('[data-comment-sort="latest"]'),()=>ready()&&ids().length===0,'empty comments');await capture('empty_dark');
    await mode('none');await theme('light');await read(button('刷新评论',page()),()=>ready()&&ids()[0]==='101','latest recovery');await capture('latest_recovered_light');
    await mode('fail-floor-first');await openFloor();assert(one('.comment-floor-panel .error-state'),'first floor failure');await capture('floor_failed_first_light');
    await mode('none');await read(button('重试',one('.comment-floor-panel .error-state')),()=>floorReady()&&floorIds()[0]==='501','first floor retry');assert(one('[data-floor-group="current"] [data-comment-id="602"]'),'current reply visible');await capture('floor_retry_light');
    await mode('fail-floor-next');await read(button('下一页回复',floor()),()=>floorReady()&&one('.comment-floor-panel .error-state'),'floor next failure');assert(floorIds()[0]==='501','failed floor retains page');assert(fullControl(button('重试',floor().querySelector('footer'))),'floor error retry fully visible');await capture('floor_next_failed_light');
    await mode('none');await read(button('重试',floor().querySelector('footer')),()=>floorReady()&&floorIds()[0]==='521','floor cursor retry');await capture('floor_next_retry_light');
    await read(button('上一页回复',floor()),()=>floorReady()&&floorIds()[0]==='501','floor back');await capture('floor_back_light');
    await mode('fail-floor-next');await read(button('下一页回复',floor()),()=>floorReady()&&floorIds()[0]==='521'&&one('.comment-floor-panel .error-state'),'floor cached navigation');const floorCachedRetry=button('重试',floor().querySelector('footer'));floorCachedRetry.scrollIntoView({block:'nearest'});floorCachedRetry.focus({preventScroll:true});await capture('floor_cached_failure_light');
    await mode('none');await read(button('重试',floor().querySelector('footer')),()=>floorReady()&&!one('.comment-floor-panel .error-state'),'floor cached retry');assert(floorIds().some(id=>id==='540')&&bounds(one('[data-floor-group="replies"] [data-comment-id="540"]')),'floor cache refresh retains bottom reading');await capture('floor_cached_retry_light');
    await read(button('上一页回复',floor()),()=>floorReady()&&floorIds()[0]==='501','one floor back');assert(button('上一页回复',floor()).disabled,'floor cache did not duplicate history');await capture('floor_cached_back_light');
    click(button('收起回复',floor()));await wait(()=>!floor(),'close floor');await settle();assert(document.activeElement?.closest('.comment-card')?.dataset.commentId==='101','floor source restored');await capture('floor_closed_light');
    await theme('dark');await mode('hold-next');click(button('下一页',pagination()));await wait(async()=> (await mode('read')).pending,'held next');await capture('late_read_inflight_dark');
    click(button('每日推荐'));await wait(()=>one('.track-row')&&!page(),'leave before response');await capture('left_before_late_dark');
    await mode('none');await wait(async()=> (await mode('read')).delayedCompleted===1,'old read released');assert(!page(),'late read cannot reopen comments');await capture('late_finished_dark');
    await comments();one('[aria-label="评论内容"]').focus();await capture('reentered_dark');
    await resize(760,540);await theme('light');one('[aria-label="评论内容"]').focus();await capture('small_latest_light');
    await read(button('下一页',pagination()),()=>ready()&&ids()[0]==='121','small next');await read(button('下一页',pagination()),()=>ready()&&ids()[0]==='141','last page');button('上一页',pagination()).focus();pagination().scrollIntoView({block:'nearest'});assert(button('下一页',pagination()).disabled,'last page bounded');await capture('small_page_end_light');
    await theme('dark');await read(one('[data-comment-sort="hot"]'),()=>ready()&&ids()[0]==='301','small hot');await capture('small_hot_dark');
    await openFloor(301);await capture('small_floor_dark');await mode('fail-floor-next');await read(button('下一页回复',floor()),()=>floorReady()&&one('.comment-floor-panel .error-state'),'small floor failure');assert(fullControl(button('重试',floor().querySelector('footer'))),'small floor error retry fully visible');await capture('small_floor_fail_dark');
    await mode('none');await read(button('重试',floor().querySelector('footer')),()=>floorReady()&&floorIds()[0]==='521','small floor retry');await capture('small_floor_retry_dark');click(button('收起回复',floor()));await wait(()=>!floor(),'small floor close');await settle();await capture('small_floor_close_dark');
    localStorage.setItem('cumulus.motion.v1','disabled');dispatchEvent(new StorageEvent('storage',{key:'cumulus.motion.v1',newValue:'disabled',storageArea:localStorage}));await wait(()=>document.documentElement.dataset.motion==='reduced','reduced preference');await theme('light');await openFloor(301);await capture('reduced_floor_open_light');click(button('收起回复',floor()));await wait(()=>!floor(),'reduced close');await settle();await capture('reduced_floor_close_light');
    await mode('expire');await read(button('刷新评论',page()),()=>one('.welcome')&&!page(),'expired account');await capture('expired_light');
    await emit('complete',{ok:true,commentReads:true,stageCount:count,realAccountWrites:0});
  } catch(error) {
    const failedAt=last;
    await capture('diagnostic',{diagnostic:true}).catch(()=>{});await mode('none').catch(()=>{});
    await emit('failed',{ok:false,error:String(error),stack:error?.stack,last:failedAt,failedView:window.__commentReadBeforeCheck,active:document.activeElement?.outerHTML?.slice(0,600)});
  }
})();
