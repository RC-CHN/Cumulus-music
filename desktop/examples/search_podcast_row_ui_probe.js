// The Linux runner provides real X11 input for the row body, not synthetic dblclick.
(async () => {
  const query = '合成播单续播';
  const invoke = (command, args) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, extra = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...extra } });
  const one = (selector) => document.querySelector(selector);
  const row = (id = '7') => one(`.search-podcast-row[data-result-key="podcast:${id}"]`);
  const tab = () => one('#search-tab-podcasts');
  const index = (id) => row(id)?.querySelector('[data-result-action="play"]');
  const bar = () => one('.playerbar');
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (ok, message) => { if (!ok) throw Error(message); };
  const settled = () => !one('.page-enter-active,.page-leave-active,.search-results-enter-active,.search-results-leave-active');
  const ready = (node) => node?.isConnected && !node.disabled && node;
  const visible = () => settled() && one('.search-results-page h1')?.textContent === query &&
    tab()?.getAttribute('aria-selected') === 'true' && ready(index());
  async function wait(read, message) {
    for (let attempt = 0; attempt < 250; attempt++) {
      const value = await read();
      if (value) return value;
      await sleep(60);
    }
    throw Error(message);
  }
  let epoch, step = 'startup', count = 0, last;
  const gestures = [];
  let trustedIndexClicks = 0;
  for (const type of ['click','dblclick']) document.addEventListener(type, (event) => {
    if (!event.isTrusted || !(event.target instanceof Element) || !event.target.closest('.search-podcast-row')) return;
    if (type === 'click' && event.target.closest('[data-result-action="play"]')) trustedIndexClicks++;
    else if (!event.target.closest('button, a')) gestures.push([type, event.detail]);
  }, true);
  const snapshot = async () => (last = await invoke('player_snapshot', { epoch }));
  const queue = () => invoke('player_queue', { epoch });
  const action = (action) => invoke('player_action', { epoch, action });
  const current = (id, state) => async () => {
    const value = await snapshot();
    return value.state === state && value.track?.voiceId === id && value.playId &&
      bar()?.dataset.playId === value.playId && bar()?.dataset.state === state && value;
  };
  const targetPoint = (target) => {
    assert(target, 'pointer target exists');
    const rect = target.getBoundingClientRect();
    const x = rect.left + rect.width / 2, y = rect.top + rect.height / 2;
    assert(target.contains(document.elementFromPoint(x,y)), 'actual pointer target is unobstructed');
    return { x:Math.round((screenX+x)*devicePixelRatio), y:Math.round((screenY+y)*devicePixelRatio) };
  };
  const rowPoint = () => targetPoint(row()?.querySelector('.search-podcast-score'));
  async function capture(label, extra = {}) {
    const stage = `ux_${String(++count).padStart(2,'0')}_${label}`;
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await sleep(120);
    await emit(stage,{...extra, gestures:[...gestures], queue:await queue()});
    await wait(() => window.__searchPodcastPlaybackRecorded === stage, `native sample ${stage}`);
  }
  try {
    ({epoch}=await wait(async()=>{
      const a=await invoke('account_snapshot');
      return a.status==='authenticated' && (await invoke('player_snapshot',{epoch:a.epoch})).state==='idle' &&
        settled() && one('.track-row') && ready(one('[aria-label="搜索音乐"]')) && a;
    },'signed fixture, idle player, Daily and header ready'));
    step='classified-search';
    const input=one('[aria-label="搜索音乐"]');
    input.value=query;
    input.dispatchEvent(new Event('input',{bubbles:true}));
    input.form.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));
    (await wait(()=>settled() && one('.search-results-page h1')?.textContent===query && ready(tab()),'Podcasts tab')).click();
    await wait(visible,'classified search podcast row ready');
    step='body-single';
    await capture('single_armed',{pointer:rowPoint()});
    await wait(()=>settled() && one('.podcast-page h1')?.textContent==='播客详情' &&
      one('.podcast-page')?.textContent.includes('合成续播播单'),'trusted single opens exact detail');
    await capture('single_detail');
    one('[aria-label="返回上一页"]').click();
    await wait(visible,'Back restored the same classified search');
    step='paused-baseline';
    await action({kind:'mode',mode:'single_loop'});
    await Promise.race([
      invoke('play_selection',{epoch,selection:{kind:'search_result_voice',id:'400'}}),
      sleep(8000).then(()=>{throw Error('baseline native selection did not return');}),
    ]);
    await wait(current('400','playing'),'real older voice loads');
    await action({kind:'pause',paused:true});
    const old=await wait(current('400','paused'),'real older voice paused');
    const oldQueue=await queue();
    const pref=await invoke('queue_click_preferences',{epoch});
    await invoke('queue_click_preferences_command',{epoch,command:{kind:'change',
      expected_revision:pref.value.revision,replace_queue:false}});
    step='body-double';
    await capture('double_armed',{pointer:rowPoint(),old});
    const first=await wait(current('1','playing'),'trusted double-click starts stored episode');
    assert(first.queueLen===252 && first.mode==='single_loop' && first.position>=18.8 && visible(),'kept old queue, mode and resumed checkpoint without navigation');
    const list=await queue();
    assert(list.tracks[0].entryId===oldQueue.current && list.tracks[1].track.voiceId==='251' &&
      list.tracks.at(-1).track.voiceId==='1' && list.current===first.entryId,'native order is old then whole descending list');
    const pauseIndex=await wait(()=>ready(index())?.getAttribute('aria-label')?.startsWith('暂停') && index(),'same-source index pause');
    pauseIndex.focus({preventScroll:true});
    await capture('double_played',{first,old,pointer:targetPoint(pauseIndex)});
    step='index-pause';
    const paused=await wait(current('1','paused'),'same decoder pauses');
    assert(paused.playId===first.playId&&paused.entryId===first.entryId&&paused.queueRevision===first.queueRevision,'index pause did not select again');
    assert(trustedIndexClicks===1,'physical index click is trusted');
    await capture('index_paused',{paused,trustedIndexClicks});
    step='failed-index';
    (await wait(()=>ready(index('8')),'other index playable')).click();
    await wait(()=>one('.playback-message [data-retry-key]'),'other radio failure visible');
    const unchanged=await wait(current('1','paused'),'failed radio leaves old decoder paused');
    assert(unchanged.playId===paused.playId&&unchanged.position===paused.position&&unchanged.queueRevision===paused.queueRevision,'failed radio preserved paused clock');
    await capture('failed_kept',{unchanged,pointer:rowPoint()});
    step='same-episode-body-resume';
    const resumed=await wait(current('1','playing'),'same retained episode resumes from row body');
    assert(resumed.playId===paused.playId && resumed.entryId===paused.entryId &&
      resumed.position>=paused.position && visible(),'body replay keeps the same decoder and does not navigate');
    const advanced=await wait(async()=>{
      const state=await current('1','playing')();
      return state && state.position>paused.position+.12 && state;
    },'retained real decoder advances its existing clock');
    await capture('same_episode_resumed',{resumed,advanced});
    await action({kind:'stop'});
    await wait(async()=>(await snapshot()).state==='idle','explicit native stop');
    await emit('complete',{ok:true,stageCount:count,gestures,trustedIndexClicks});
  } catch (error) {
    await emit('failed',{step,count,message:String(error),last,gestures,
      page:one('.main-content')?.textContent.trim().slice(0,500)});
  }
})();
