// Production lyric rows, queue marker, native drop and sealed null playback.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
  const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (condition, label) => { if (!condition) throw Error(label); };
  const wait = async (test, label) => {
    for (let i = 0; i < 180; i++) {
      const value = await test();
      if (value) return value;
      await pause(60);
    }
    throw Error(label);
  };
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, info = {}) => invoke('plugin:event|emit', { event:'ux-proof', payload:{ stage, ...info } });
  const panel = () => one('#search-result-panel');
  const row = (ordinal) => panel()?.querySelector(`[data-lyric-index="${ordinal}"] .track-row`);
  const title = (ordinal) => row(ordinal)?.querySelector('.track-row-title-text');
  const selection = () => all('#search-result-panel [data-lyric-index] .track-row[aria-selected="true"]')
    .map((node) => Number(node.closest('[data-lyric-index]').dataset.lyricIndex));
  const bar = () => one('.playerbar');
  const scroller = () => one('.queue-scroll');
  const marker = () => one('.queue-drop-marker');
  const hint = () => one('.queue-drag-hint');
  const button = (label, root = document) => [...root.querySelectorAll('button')].find((node) =>
    node.getAttribute('aria-label') === label || node.textContent.trim() === label);
  const click = (node) => {
    assert(node?.isConnected && !node.disabled, 'exact enabled control');
    node.focus({ preventScroll:true });
    node.click();
  };
  const trusted = window.__cumulusSearchDropTrusted === true;
  const tickets = new Map(), dropCalls = [];
  let epoch, baseline, count = 0, step = 'startup';
  const player = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  const ids = (queue) => queue.tracks.map((entry) => entry.track.id).join(',');
  const identity = (state) => [state.epoch,state.state,state.playId,state.entryId,state.track?.id,state.position];
  const same = (state) => JSON.stringify(identity(state)) === JSON.stringify(identity(baseline));
  const point = (node) => {
    const box = node.getBoundingClientRect();
    const x = box.left + Math.min(8, box.width / 2), y = box.top + box.height / 2;
    return { x, y };
  };
  const absolute = ({x,y}) => ({ x:Math.floor((screenX+x)*devicePixelRatio), y:Math.floor((screenY+y)*devicePixelRatio) });

  function dispatch(target, type, coords, { ctrlKey = false, buttons = 1 } = {}) {
    const EventType = type.startsWith('pointer') ? PointerEvent : MouseEvent;
    target.dispatchEvent(new EventType(type, { bubbles:true, cancelable:true, isPrimary:true,
      pointerType:'mouse', pointerId:57, button:0, buttons, ctrlKey, clientX:coords.x, clientY:coords.y }));
  }
  async function source(ordinal) {
    const node = await wait(() => title(ordinal), 'exact virtual lyric row '+ordinal);
    node.scrollIntoView({ block:'nearest', inline:'nearest', behavior:'instant' });
    let previous, stableSince;
    return wait(() => {
      if (!node.isConnected || node !== title(ordinal)) return false;
      const {x,y} = point(node);
      const geometry = [x,y,panel()?.scrollTop].join(',');
      if (geometry !== previous) { previous = geometry; stableSince = performance.now(); return false; }
      return performance.now() - stableSince >= 100 && node.contains(document.elementFromPoint(x,y)) && node;
    }, 'physical lyric row after scroll '+ordinal);
  }
  async function select(ordinal, ctrlKey = false) {
    const node = await source(ordinal), at = point(node);
    dispatch(node,'pointerdown',at,{ctrlKey});
    dispatch(node,'mousedown',at,{ctrlKey});
    dispatch(node,'pointerup',at,{ctrlKey,buttons:0});
    dispatch(node,'mouseup',at,{ctrlKey,buttons:0});
    dispatch(node,'click',at,{ctrlKey,buttons:0});
  }
  async function capture(label, extra = {}) {
    step = label;
    const [checkedPlayer, checkedQueue] = await Promise.all([player(),queue()]);
    assert(same(checkedPlayer) && checkedPlayer.queueRevision === checkedQueue.revision &&
      checkedQueue.current === baseline.entryId, 'same paused native identity and clock');
    await emit(`ux_${String(++count).padStart(2,'0')}_${label}`, { label, checkedPlayer, checkedQueue,
      selectedOrdinals:selection(), dropCalls:[...dropCalls], screenX, screenY, ratio:devicePixelRatio, ...extra });
  }
  async function drag(ordinal, gap, label) {
    const node = await source(ordinal);
    const target = scroller();
    const queueRows = [...target.querySelectorAll('.queue-row')];
    const viewport = target.getBoundingClientRect();
    const first = queueRows[0].getBoundingClientRect(), last = queueRows.at(-1).getBoundingClientRect();
    const from = point(node), to = { x:viewport.left + viewport.width/2,
      y:gap === 'front' ? first.top+8 : last.bottom-6 };
    assert(target.contains(document.elementFromPoint(to.x,to.y)), 'real queue destination is hittable');
    let nativeDown = false, nativeUp = false;
    const down = (event) => { if (event.isTrusted && node.contains(event.target)) nativeDown = true; };
    const up = (event) => { if (event.isTrusted && scroller()?.contains(event.target)) nativeUp = true; };
    if (trusted) {
      document.addEventListener('pointerdown',down,{ capture:true,once:true });
      document.addEventListener('pointerup',up,{ capture:true,once:true });
    }
    await capture(label + '_drag_target', { pointer:{ from:absolute(from), to:absolute(to) } });
    if (!trusted) {
      dispatch(node,'pointerdown',from);
      dispatch(node,'mousedown',from);
      dispatch(target,'pointermove',to);
    }
    const markerTop = gap === 'front' ? '0px' : `${queueRows.length*64}px`;
    await wait(() => marker()?.style.top === markerTop && hint()?.textContent.includes('正在添加 3 首歌曲'),
      'three selected tracks reach the exact visual queue gap '+gap);
    assert(!trusted || nativeDown, 'real X11 pointer down reached the grabbed hit');
    const line = marker(), rect = line.getBoundingClientRect();
    const sample = { x:rect.left+8, y:rect.top+1, color:getComputedStyle(line).backgroundColor };
    await capture(label + '_marker', { markerTop, paintSamples:[sample], nativeDown });
    return async (stale = false) => {
      if (!trusted) {
        const releaseTarget = target.isConnected ? target : document.body;
        dispatch(releaseTarget,'pointerup',to,{buttons:0});
        dispatch(releaseTarget,'mouseup',to,{buttons:0});
      } else {
        await wait(() => nativeUp, stale ? 'released over the queue after leaving' : 'real X11 pointer up reached the queue');
      }
      document.removeEventListener('pointerdown',down,true);
      document.removeEventListener('pointerup',up,true);
      return nativeUp;
    };
  }

  try {
    const originalFetch = window.fetch;
    window.fetch = async function (url, options) {
      const isSearch = String(url).endsWith('/search_results') && typeof options?.body === 'string';
      const query = isSearch ? JSON.parse(options.body).query : null;
      const response = await originalFetch.call(window,url,options);
      if (query?.category === 'lyrics' && response.ok) {
        const data = await response.clone().json();
        for (const section of data.value?.sections ?? []) section.items.forEach((item,index) => {
          if (item.lyricQueueDrop?.receipt) tickets.set(item.lyricQueueDrop.receipt,
            { id:item.track.id, ordinal:query.offset+index, right:item.lyricQueueDrop.right });
        });
      }
      return response;
    };
    await wait(() => one('.track-row') && button('列表循环',bar() ?? document)?.disabled === false,
      'authenticated daily page and player ready');
    await emit('resize',{ width:1120, height:760 });
    await wait(() => innerWidth === 1120 && innerHeight === 760,'wide viewport');
    ({epoch} = await invoke('account_snapshot'));
    await invoke('player_action',{epoch,action:{kind:'mode',mode:'single_loop'}});
    await invoke('play_selection',{epoch,selection:{kind:'songs',ids:['92','91','93'],index:1,quality:'standard'}});
    await wait(async () => { const value=await player(); return value.state==='playing' &&
      value.track?.id==='91' && value.position>.2; },'real native null playback of the middle row');
    await invoke('player_action',{epoch,action:{kind:'pause',paused:true}});
    await invoke('player_action',{epoch,action:{kind:'seek',seconds:10}});
    baseline = await wait(async () => { const value=await player(); return value.state==='paused' &&
      !value.nativeSeeking && Math.abs(value.position-10)<.2 && value; },'exact paused native seek');
    assert(ids(await queue()) === '92,91,93' && baseline.track?.id === '91', 'baseline current is not first');
    window.__searchFixtureAck = false;
    await emit('search-fixture',{mode:'good'});
    await wait(() => window.__searchFixtureAck,'sealed search fixture');
    const search = one('[aria-label="搜索音乐"]');
    search.value = '合成歌词拖入队列';
    search.dispatchEvent(new Event('input',{bubbles:true}));
    search.form.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));
    await wait(() => one('.search-results-page h1')?.textContent === '合成歌词拖入队列' &&
      panel()?.getAttribute('aria-busy') === 'false' && !one('.page-enter-active,.page-leave-active'),'initial search');
    click(one('[data-category="lyrics"]'));
    await wait(() => one('[data-category="lyrics"]')?.getAttribute('aria-selected') === 'true' &&
      panel()?.getAttribute('aria-busy') === 'false' && [0,1,2,3,4].every(index=>row(index)) && tickets.size===5,
      'five exact native lyric hits');
    assert([0,1,2,3,4].map(index=>row(index).dataset.trackId).join(',') === '101,102,103,101,104',
      'result order and duplicate identity');
    click(button('播放队列',bar()));
    await wait(() => scroller()?.querySelectorAll('.queue-row').length===3 &&
      !one('.queue-panel-enter-active,.search-results-enter-active'),'real queue next to lyric results');
    const provides=one('#app').__vue_app__._context.provides;
    const core=provides[Reflect.ownKeys(provides).find(key=>typeof key==='symbol' && key.description==='core-client')];
    const action=core.playerAction;
    core.playerAction=(owner,input)=> {
      if (input.kind==='drop_search_lyrics_into_queue') {
        assert(owner===epoch && Object.keys(input).sort().join(',')===
          'anchor,before_entry_id,expected_play_id,expected_queue_revision,kind,quality,rows', 'only typed drop fields');
        const anchor=tickets.get(input.anchor), rows=input.rows.map(ticket=>tickets.get(ticket));
        assert(anchor && rows.every(Boolean), 'receipts are exact native lyric hits');
        dropCalls.push({anchor,rows,beforeEntry:input.before_entry_id,queueRevision:input.expected_queue_revision,
          playId:input.expected_play_id,quality:input.quality});
      }
      return action(owner,input);
    };
    await capture('paused_baseline');
    await select(2);
    await select(1,true);
    await select(0,true);
    await wait(() => selection().join(',')==='0,1,2,3','group selection includes both hits of 101');
    await capture('group_selection');

    const blockedRelease=await drag(1,'front','blocked');
    const blockedUp=await blockedRelease();
    const front=await wait(async () => { const value=await queue(); return ids(value)==='92,91,103,101,93' &&
      value.revision!==baseline.queueRevision && value; },'visual front gap inserts after paused current with blocked grabbed hit skipped');
    await wait(() => scroller()?.querySelectorAll('.queue-row').length===5 && !marker(),'committed front drop rendered');
    assert(dropCalls.length===1,'one native drop after release');
    await capture('blocked_committed',{ nativeUp:blockedUp });

    const duplicateRelease=await drag(3,'tail','duplicate');
    const duplicateUp=await duplicateRelease();
    const tail=await wait(async () => { const value=await queue(); return ids(value)==='92,91,93,103,101' &&
      value.revision!==front.revision && value; },'tail moves both admitted entries without duplicating them');
    await wait(() => !marker(),'tail marker gone');
    assert(dropCalls.length===2,'one native drop for duplicate gesture');
    await capture('duplicate_committed',{ nativeUp:duplicateUp });

    const staleRelease=await drag(3,'front','stale');
    click(one('[data-category="songs"]'));
    await wait(() => one('[data-category="songs"]')?.getAttribute('aria-selected')==='true' &&
      panel()?.getAttribute('aria-busy')==='false' && !row(3) && !marker() && !hint(), 'category change cancels the old gesture');
    await capture('origin_left');
    const staleUp=await staleRelease(true);
    await wait(async () => (await queue()).revision===tail.revision && !marker(),'releasing stale gesture leaves native queue unchanged');
    assert(dropCalls.length===2,'the retired source sent no drop');
    await capture('stale_released',{ nativeUp:staleUp });
    core.playerAction=action;
    window.fetch=originalFetch;
    await emit('complete',{ ok:true, actualNativePlayback:true, stageCount:count, trustedPointer:trusted,
      dropCalls, realMusicRequests:0, realAccountWrites:0 });
  } catch(error) {
    await emit('failed',{ ok:false, step, stageCount:count,error:String(error),stack:error.stack,
      selectedOrdinals:selection(),tickets:[...tickets.values()],dropCalls,marker:marker()?.style.top,
      hint:hint()?.textContent,body:document.body.innerText.slice(0,2600) });
  }
})();
