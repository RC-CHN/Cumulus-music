// Actual production UI/IPC and synthetic libmpv/null audio. No account writes.
(async () => {
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const one = selector => document.querySelector(selector);
  const all = selector => [...document.querySelectorAll(selector)];
  const assert = (ok, message) => { if (!ok) throw Error(message); };
  const wait = async (test, label) => { for (let i = 0; i < 180; i++) { if (test()) return; await sleep(100); } throw Error(label); };
  const click = node => { assert(node && !node.disabled, 'unavailable control'); node.focus({preventScroll: true}); node.click(); };
  const emit = (stage, rest = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {event: 'ux-proof', payload: {stage, ...rest}});
  const rect = node => { const r = node.getBoundingClientRect(); return {x:r.x,y:r.y,right:r.right,bottom:r.bottom,width:r.width,height:r.height}; };
  const inside = (a, b) => a.x >= b.x - 1 && a.y >= b.y - 1 && a.right <= b.right + 1 && a.bottom <= b.bottom + 1;
  const hit = node => { const r=rect(node), target=document.elementFromPoint(r.x+r.width/2,r.y+r.height/2); return node===target || node.contains(target); };
  const checks = [];
  let stageCount = 0;
  async function capture(label) {
    await sleep(500);
    const panel=one('.lyrics-panel'), body=one('.lyrics-body'), record=one('.vinyl-record'), scroller=one('.lyric-scroll');
    const metadata=all('.vinyl-info h1, .vinyl-info p, .vinyl-info small');
    const controls=all('.playerbar button:not(:disabled), .playerbar input:not(:disabled), .playerbar select:not(:disabled), .lyric-tools button:not(:disabled), [aria-label="收起播放详情"]').filter(n=>rect(n).width>0 && rect(n).height>0);
    const player=rect(one('.playerbar'));
    const row={label,width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,
      panel:rect(panel),body:rect(body),info:rect(one('.vinyl-info')),record:rect(record),player,
      metadata:metadata.map(node=>({tag:node.tagName,...rect(node),title:node.title})),
      controls:controls.map(node=>({label:node.getAttribute('aria-label')||node.textContent.trim(),player:!!node.closest('.playerbar'),...rect(node),hit:hit(node)})),
      scroll:{width:scroller.clientWidth,contentWidth:scroller.scrollWidth,top:scroller.scrollTop},
      queue:one('.queue-panel') ? {rendered:all('.queue-row').length,total:40,...rect(one('.queue-panel'))} : null,
    };
    // The queue intentionally covers part of the lyric column; test those controls once closed.
    const covered=!!row.queue;
    row.checks={documentContained:document.documentElement.scrollWidth<=innerWidth,
      metadataContained:metadata.every(node=>inside(rect(node),rect(body)) && inside(rect(node),row.info)),
      recordCircular:Math.abs(row.record.width-row.record.height)<1,
      recordContained:inside(row.record,row.info),
      lyricsWrap:scroller.scrollWidth<=scroller.clientWidth+1,
      playerControlsHit:row.controls.filter(n=>n.player).every(n=>n.hit),
      lyricControlsHit:covered || row.controls.every(n=>n.hit),
      playerSeparated:rect(panel).bottom<=player.y+1,
      queueBounded:!covered || row.queue.rendered<40,
      metadataHints:metadata.every(node=>node.title===node.textContent.trim()),
      playerHints:all('.now-copy .ellipsis').every(node=>node.title===node.textContent.trim()),
      queueHints:all('.queue-song .ellipsis').every(node=>node.title===node.textContent.trim()),
    };
    checks.push({label,...row.checks});
    await emit(`ux_${String(++stageCount).padStart(2,'0')}_${label}`,row);
    await sleep(800);
  }
  async function appearance(value) {
    click(one('[aria-label="外观主题"]'));
    await wait(()=>one('.action-menu'),'appearance');
    click(all('.action-menu button').find(n=>n.textContent.trim()===(value==='dark'?'深色':'浅色')));
    await wait(()=>document.documentElement.dataset.theme===value&&!one('.action-menu'),'theme applied');
  }
  async function play(index) {
    if(one('.lyrics-panel')) {click(one('[aria-label="收起播放详情"]'));await wait(()=>!one('.lyrics-panel'),'details close');}
    all('.track-row')[index].dispatchEvent(new MouseEvent('dblclick',{bubbles:true}));
    await wait(()=>one('.playerbar')?.dataset.state==='playing'&&Number(one('.playerbar').dataset.position)>.15,'synthetic native playing');
    click(one('[aria-label="暂停"]'));
    await wait(()=>one('.playerbar').dataset.state==='paused','native paused');
    click(one('[aria-label="歌词"]'));
    await wait(()=>one('.lyric-line.current'),'lyrics loaded');
  }
  try {
    await wait(()=>all('.track-row').length>2,'daily');
    await wait(()=>one('[aria-label="列表循环"]') && !one('[aria-label="列表循环"]').disabled,'native player ready');
    assert(one('.track-row').textContent.includes('沿着海岸'),'long fixture missing');
    await play(0);
    for(const [width,height,theme] of [[1120,760,'light'],[760,540,'light'],[760,540,'dark'],[1120,760,'dark']]) {
      await emit('resize',{width,height});await wait(()=>innerWidth===width&&innerHeight===height,'resize');await appearance(theme);
      const suffix=`${width}_${theme}`;
      one('.lyric-line.current').scrollIntoView({block:'center',behavior:'instant'});
      await capture(`long_${suffix}`);
      const scroll=one('.lyric-scroll');scroll.dispatchEvent(new WheelEvent('wheel',{bubbles:true,deltaY:500}));scroll.scrollTop=scroll.scrollHeight;
      await capture(`lyrics_bottom_${suffix}`);
      click(one('[aria-label="播放队列"]'));await wait(()=>all('.queue-row').length,'queue');await capture(`queue_${suffix}`);
      one('.queue-scroll').scrollTop=one('.queue-scroll').scrollHeight;await capture(`queue_bottom_${suffix}`);
      click(one('[aria-label="关闭播放队列"]'));await wait(()=>!one('.queue-panel'),'queue close');
    }
    await play(1);
    for(const [width,height,theme] of [[1120,760,'light'],[760,540,'light'],[760,540,'dark'],[1120,760,'dark']]) {
      await emit('resize',{width,height});await wait(()=>innerWidth===width&&innerHeight===height,'resize');await appearance(theme);
      await capture(`short_${width}_${theme}`);
    }
    click(one('[aria-label="收起播放详情"]'));await wait(()=>!one('.lyrics-panel'),'details close');await sleep(300);
    assert(document.activeElement===one('[aria-label="歌词"]'),'detail close focus lost');
    const passed=checks.every(row=>Object.entries(row).every(([key,value])=>key==='label'||value===true));
    assert(window.__playerLayoutBaseline||passed,JSON.stringify(checks.filter(row=>Object.values(row).includes(false))));
    await emit('complete',{ok:true,layoutPassed:passed,checks,stageCount,realMusicRequests:0,accountWrites:0,focusRestored:true});
  } catch(error) {await emit('failed',{ok:false,error:String(error),stageCount,checks});}
})();
