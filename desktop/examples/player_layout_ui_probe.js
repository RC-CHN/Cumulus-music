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
    const credits=all('.vinyl-credits .vinyl-credit, .vinyl-album .vinyl-credit');
    const hintTargets=[one('.vinyl-info h1'), ...credits];
    const controls=all('.playerbar button:not(:disabled), .playerbar input:not(:disabled), .playerbar select:not(:disabled), .lyric-tools button:not(:disabled), .lyric-browse-time:not(:disabled), [aria-label="收起播放详情"]').filter(n=>rect(n).width>0 && rect(n).height>0);
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
      metadataHints:credits.length>=2 && hintTargets.every(node=>node && node.title===node.textContent.trim()),
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
  async function verifyLyricModes() {
    const roman=()=>one('[aria-label="音译歌词"]'), translation=()=>one('[aria-label="翻译歌词"]');
    const pressed=node=>node?.getAttribute('aria-pressed')==='true';
    const displayed=text=>one('.lyric-line.current small')?.textContent.includes(text);
    const scroller=one('.lyric-scroll');
    await wait(()=>roman()&&translation()&&displayed('合成翻译'),'initial auxiliary mode');
    assert(pressed(translation())&&!pressed(roman())&&all('.lyric-line.current small').length===1,'legacy settings do not display two auxiliaries');
    scroller.dispatchEvent(new WheelEvent('wheel',{bubbles:true,deltaY:500}));
    scroller.scrollTop=scroller.scrollHeight;
    await wait(()=>one('.lyric-scroll-shell.browsing'),'manual scroll before mode change');
    click(roman());
    await wait(()=>pressed(roman())&&!pressed(translation())&&displayed('synthetic roman')&&!one('.lyric-scroll-shell.browsing'),'romanization replaces translation and resumes follow');
    click(roman());
    await wait(()=>!pressed(roman())&&!one('.lyric-line.current small'),'original lyrics only');
    click(translation());
    await wait(()=>pressed(translation())&&displayed('合成翻译'),'restore translation');
    const advance=one('[aria-label="歌词提前半秒"]'), delay=one('[aria-label="歌词延后半秒"]');
    const offset=()=>advance.previousElementSibling.textContent.trim();
    assert(offset()==='0.0s','initial lyric offset');
    click(advance);
    await wait(()=>offset()==='0.5s'&&advance.getAttribute('aria-disabled')==='false','advance half a second');
    click(delay);
    await wait(()=>offset()==='0.0s'&&delay.getAttribute('aria-disabled')==='false','restore lyric offset');
  }
  async function verifyLyricFollow() {
    const scroller=one('.lyric-scroll');
    const resume=()=>all('.lyric-tools button').find(node=>node.textContent.trim()==='回到当前');
    const atCurrent=()=>{
      const current=one('.lyric-line.current');
      if(!current) return false;
      const a=rect(current), b=rect(scroller);
      return Math.abs((a.y+a.bottom)-(b.y+b.bottom))<16;
    };
    const browse=()=>{
      scroller.dispatchEvent(new WheelEvent('wheel',{bubbles:true,deltaY:500}));
      scroller.scrollTop=scroller.scrollHeight;
      assert(resume() || !atCurrent(),'manual browsing moved away');
    };
    browse();
    await wait(()=>resume()&&!atCurrent(),'paused manual browse');
    await sleep(5200);
    assert(resume()&&!atCurrent(),'paused browse did not snap back after idle');
    click(one('.playerbar [aria-label="播放"]'));
    await wait(()=>one('.playerbar').dataset.state==='playing'&&!resume()&&atCurrent(),'resume follows current line');
    browse();
    await wait(()=>resume()&&!atCurrent(),'playing manual browse');
    await sleep(2800);
    browse();
    await sleep(2900);
    assert(resume()&&!atCurrent(),'second gesture restarted idle deadline');
    await wait(()=>!resume()&&atCurrent(),'playing browse resumes after inactivity');
    click(one('.playerbar [aria-label="暂停"]'));
    await wait(()=>one('.playerbar').dataset.state==='paused','pause after lyric follow test');
    const identity=one('.playerbar').dataset.playId;
    const lyric=one('.lyric-line[data-line="7"]');
    const marker=()=>one('.lyric-browse-time');
    scroller.dispatchEvent(new WheelEvent('wheel',{bubbles:true,deltaY:500}));
    await wait(()=>scroller.parentElement.classList.contains('browsing'),'timeline browsing');
    lyric.scrollIntoView({block:'center',behavior:'instant'});
    await wait(()=>marker()?.textContent.trim()==='00:07' && lyric.classList.contains('browsed'),'centered lyric timeline');
    assert(hit(marker()) && one('.playerbar').dataset.state==='paused','timeline is reachable without changing playback');
    await capture('lyric_timeline');
    click(marker());
    await wait(()=>{
      const bar=one('.playerbar');
      return bar.dataset.state==='playing'&&bar.dataset.playId===identity&&
        Number(bar.dataset.position)>=7&&Number(bar.dataset.position)<9&&!marker();
    },'timeline seeks and resumes same entry');
    click(one('.playerbar [aria-label="暂停"]'));
    await wait(()=>one('.playerbar').dataset.state==='paused','pause after timeline test');
  }
  async function verifyLyricFont() {
    const control=()=>one('[aria-label="调整歌词字号"]');
    const popup=()=>one('.lyric-font-popover');
    const gain=()=>parseFloat(getComputedStyle(one('.lyric-line')).fontSize);
    const initial=gain();
    const playId=one('.playerbar').dataset.playId;
    click(control());
    await wait(()=>popup()&&document.activeElement===popup().querySelector('input'),'font slider focus');
    const slider=popup().querySelector('input');
    const plus=popup().querySelector('[aria-label="增大歌词字号"]');
    const minus=popup().querySelector('[aria-label="减小歌词字号"]');
    click(plus);
    await wait(()=>gain()===initial+1,'font increment affects lyric size');
    slider.value='4';slider.dispatchEvent(new Event('input',{bubbles:true}));
    await wait(()=>gain()===initial+4&&plus.getAttribute('aria-disabled')==='true','font gain is bounded');
    assert(hit(slider)&&hit(minus),'font controls are reachable');
    await capture('lyric_font');
    click(minus);
    await wait(()=>gain()===initial+3,'font decrement');
    minus.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true}));
    await wait(()=>!popup()&&document.activeElement===control(),'font Escape closes only popover');
    click(one('[aria-label="收起播放详情"]'));
    await wait(()=>!one('.lyrics-panel'),'font preference unmount');
    click(one('[aria-label="歌词"]'));
    await wait(()=>one('.lyric-line.current')&&gain()===initial+3,'saved font gain restored');
    assert(one('.playerbar').dataset.playId===playId&&one('.playerbar').dataset.state==='paused','font preference does not alter native playback');
  }
  try {
    await wait(()=>all('.track-row').length>2,'daily');
    await wait(()=>one('[aria-label="列表循环"]') && !one('[aria-label="列表循环"]').disabled,'native player ready');
    assert(one('.track-row').textContent.includes('沿着海岸'),'long fixture missing');
    await play(0);
    if(!window.__playerLayoutBaseline) {
      const mark=()=>one('.seek-chorus');
      await wait(()=>mark(),'chorus metadata appears on the playing track');
      assert(Math.abs(parseFloat(mark().style.left)-400/30)<.01&&!mark().classList.contains('reached'),'chorus marker uses full track clock');
      await verifyLyricModes();
      await verifyLyricFollow();
      await wait(()=>mark()?.classList.contains('reached'),'chorus marker reflects passed progress');
      await verifyLyricFont();
    }
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
    if(!window.__playerLayoutBaseline)
      assert(!one('[aria-label="翻译歌词"]')&&!one('[aria-label="音译歌词"]')&&one('[aria-label="歌词提前半秒"]'),'original-only song does not show auxiliary toggles');
    if(!window.__playerLayoutBaseline) assert(!one('.seek-chorus'),'replacement song does not reuse previous marker');
    for(const [width,height,theme] of [[1120,760,'light'],[760,540,'light'],[760,540,'dark'],[1120,760,'dark']]) {
      await emit('resize',{width,height});await wait(()=>innerWidth===width&&innerHeight===height,'resize');await appearance(theme);
      await capture(`short_${width}_${theme}`);
    }
    click(one('[aria-label="收起播放详情"]'));await wait(()=>!one('.lyrics-panel'),'details close');await sleep(300);
    assert(document.activeElement===one('[aria-label="歌词"]'),'detail close focus lost');
    const passed=checks.every(row=>Object.entries(row).every(([key,value])=>key==='label'||value===true));
    assert(window.__playerLayoutBaseline||passed,JSON.stringify(checks.filter(row=>Object.values(row).includes(false))));
    await emit('complete',{ok:true,layoutPassed:passed,checks,stageCount,realMusicRequests:0,accountWrites:0,focusRestored:true,lyricFollowVerified:!window.__playerLayoutBaseline,lyricTimelineVerified:!window.__playerLayoutBaseline,lyricFontVerified:!window.__playerLayoutBaseline,lyricModesVerified:!window.__playerLayoutBaseline,chorusMarkerVerified:!window.__playerLayoutBaseline});
  } catch(error) {await emit('failed',{ok:false,error:String(error),stageCount,checks});}
})();
