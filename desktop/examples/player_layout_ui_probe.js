// Actual production UI/IPC and synthetic libmpv/null audio. No account writes.
(async () => {
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const one = selector => document.querySelector(selector);
  const all = selector => [...document.querySelectorAll(selector)];
  const assert = (ok, message) => { if (!ok) throw Error(message); };
  const wait = async (test, label, attempts = 180) => { for (let i = 0; i < attempts; i++) { if (test()) return; await sleep(100); } throw Error(label); };
  const click = node => { assert(node && !node.disabled, 'unavailable control'); node.focus({preventScroll: true}); node.click(); };
  const emit = (stage, rest = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {event: 'ux-proof', payload: {stage, ...rest}});
  const rect = node => { const r = node.getBoundingClientRect(); return {x:r.x,y:r.y,right:r.right,bottom:r.bottom,width:r.width,height:r.height}; };
  const inside = (a, b) => a.x >= b.x - 1 && a.y >= b.y - 1 && a.right <= b.right + 1 && a.bottom <= b.bottom + 1;
  const hit = node => { const r=rect(node), target=document.elementFromPoint(r.x+r.width/2,r.y+r.height/2); return node===target || node.contains(target); };
  const checks = [];
  let stageCount = 0;
  const playerComment = () => one('.player-comment');
  const commentBadge = () => one('.player-comment-count');
  const commentDescription = () => document.getElementById(playerComment()?.getAttribute('aria-describedby'));
  const trackActionTrigger = () => one('.playerbar [aria-label="当前歌曲更多操作"]');
  const trackActionMenu = () => one('.action-menu[aria-label="当前歌曲更多操作"]:not([inert]):not(.menu-leave-active)');
  const trackAction = key => trackActionMenu()?.querySelector(`[data-menu-key="${key}"]`);
  async function openTrackActions(shareExpected = true) {
    assert(trackActionTrigger() && hit(trackActionTrigger()),'more menu trigger is hit-testable');
    click(trackActionTrigger());
    await wait(()=>trackAction('effects')&&hit(trackAction('effects'))&&
      (shareExpected ? trackAction('share')&&hit(trackAction('share')) : !trackAction('share')),
      'current song menu shows the expected controls above the bottom bar');
    return trackActionMenu();
  }
  const shareDialog = kind => one(
    `[data-player-song-share-dialog][data-history-share-dialog="${kind}"][open]:not([inert])`,
  );
  const shareStatus = () => one('[data-player-song-share-status]');
  let clipboardSequence = 0;
  window.__playerSongShareClipboard = {};
  async function nativeClipboard(action) {
    const sequence = ++clipboardSequence;
    await emit('player-song-share-clipboard', {sequence,action});
    await wait(() => typeof window.__playerSongShareClipboard[sequence] === 'boolean', 'native clipboard receipt');
    assert(window.__playerSongShareClipboard[sequence], 'native clipboard must match the exact sealed value: '+action);
  }
  async function chooser(alreadyOpen = false) {
    if (!alreadyOpen) await openTrackActions();
    click(trackAction('share'));
    await wait(() => shareDialog('chooser') && document.activeElement?.getAttribute('aria-label') === '关闭分享',
      'current song share chooser has close focus');
    const dialog = shareDialog('chooser');
    const choices = [...dialog.querySelectorAll('[data-history-share-channel]')].map(node => node.textContent.trim());
    assert(dialog.querySelector('h2')?.textContent.trim()==='分享到'&&choices.join(',')==='微信好友,复制链接',
      'ordinary Song exposes only the supported local channels');
    return dialog;
  }
  async function shareFocus() {
    await wait(() => !one('[data-player-song-share-dialog]')&&document.activeElement===trackActionTrigger()&&
      hit(trackActionTrigger()),'current share returns focus to the visible More trigger');
  }
  function shareQrModules(svg) {
    const [x,y,width,height] = svg.getAttribute('viewBox').split(/\s+/).map(Number);
    const size = width-8, d = svg.querySelector('path')?.getAttribute('d') ?? '';
    assert(x===0&&y===0&&width===height&&[25,29].includes(size)&&d,'rendered QR has a bounded grid and quiet zone');
    const rows = Array.from({length:size},()=>Array(size).fill('0'));
    const shapes = [...d.matchAll(/M(\d+) (\d+)h1v1h-1z/g)];
    assert(shapes.map(m=>m[0]).join(' ')===d,'rendered QR path consists of unit squares');
    for(const [,column,row] of shapes) {
      const c=Number(column)-4,r=Number(row)-4;
      assert(rows[r]?.[c]==='0','QR quiet zone remains clear and modules are unique');
      rows[r][c]='1';
    }
    return rows.map(row=>row.join(''));
  }
  async function captureShare(label, fields) {
    const bar=one('.playerbar');
    await emit(`ux_${String(++stageCount).padStart(2,'0')}_current_share_${label}`,{
      playId:bar.dataset.playId,trackId:bar.dataset.trackId,position:Number(bar.dataset.position),...fields,
    });
    await sleep(800);
  }
  async function verifyCurrentShare() {
    const invoke = (command,args) => window.__TAURI_INTERNALS__.invoke(command,args);
    const {epoch} = await invoke('account_snapshot');
    const {playId} = await invoke('player_snapshot',{epoch});
    const forged = await invoke('player_song_share_prepare',{epoch,target:{playId,entryId:'forged'}}).then(()=>null,error=>error);
    assert(forged?.code==='stale','registered share IPC rejects a forged entry before reading rights');
    const dialog = await chooser(true);
    assert(!dialog.dispatchEvent(new Event('cancel',{cancelable:true})),'dialog Cancel is handled');
    await shareFocus();
    await captureShare('cancelled',{focusRestored:true,forgedRejected:true});

    await nativeClipboard('seed');
    click((await chooser()).querySelector('[data-history-share-channel="copy"]'));
    await wait(()=>shareStatus()?.textContent.trim()==='当前资源暂不支持分享。','second rights read denies copying');
    await shareFocus();
    await nativeClipboard('marker');
    await captureShare('denied',{focusRestored:true,clipboardUntouched:true,notice:shareStatus().textContent.trim()});

    click((await chooser()).querySelector('[data-history-share-channel="copy"]'));
    await wait(()=>shareStatus()?.textContent.trim()==='复制成功','typed native clipboard success');
    await shareFocus();
    await nativeClipboard('song');
    await captureShare('copied',{focusRestored:true,nativeClipboard:true,notice:shareStatus().textContent.trim()});

    click((await chooser()).querySelector('[data-history-share-channel="wechat_qr"]'));
    await wait(()=>shareDialog('wechat_qr')?.querySelector('[data-history-share-qr]')&&
      document.activeElement?.getAttribute('aria-label')==='关闭微信分享二维码','local QR replaces chooser and focuses Close');
    const qr = shareDialog('wechat_qr'), svg = qr.querySelector('[data-history-share-qr]');
    const title=qr.querySelector('h2').textContent.trim(), hint=qr.querySelector('p').textContent.trim();
    assert(title==='分享到微信'&&hint==='打开微信, 扫一扫'&&svg.getAttribute('width')==='180','current share QR content');
    await wait(()=>getComputedStyle(qr).opacity==='1'&&!qr.classList.contains('modal-enter-active')&&
      !one('[data-player-song-share-dialog][data-history-share-dialog="chooser"]'),
      'QR dialog and chooser transition reach the painted state');
    const modules=shareQrModules(svg), box=rect(svg), unit=box.width/(modules.length+8);
    assert(inside(box,{x:0,y:0,right:innerWidth,bottom:innerHeight})&&box.width===180&&box.height===180,
      'QR stays inside the narrow viewport after animation');
    await captureShare('wechat',{title,hint,qrWidth:180,qrModules:modules,
      screenX,screenY,ratio:devicePixelRatio,paintSamples:[
        {x:box.x+2*unit,y:box.y+2*unit,color:'rgb(255, 255, 255)'},
        {x:box.x+7.5*unit,y:box.y+7.5*unit,color:'rgb(0, 0, 0)'},
      ],
    });
    click(qr.querySelector('header button'));
    await shareFocus();
  }
  async function verifyCurrentShareRetirement() {
    const invoke = (command,args) => window.__TAURI_INTERNALS__.invoke(command,args);
    const {epoch} = await invoke('account_snapshot');
    const before = await invoke('player_snapshot',{epoch});
    await nativeClipboard('seed');
    await chooser();
    const target={playId:before.playId,entryId:before.entryId};
    // A second registered native offer retires the UI's older one. Hold its
    // actual typed copy invocation to observe the return after native selection.
    const prepared = await invoke('player_song_share_prepare',{epoch,target});
    assert(prepared.value.kind==='ready','same source permits the explicit newer proof offer');
    const pending = invoke('player_song_share_choose',{epoch,target,offer:prepared.value.receipt,choice:'copy'}).then(()=>null,error=>error);
    for(let i=0;i<30&&window.__playerSongShareReads!==10;i++) {
      await emit('player-song-share-read-count');
      await sleep(45);
    }
    assert(window.__playerSongShareReads===10,'native transport received the held channel read');
    await invoke('play_selection',{epoch,selection:{kind:'songs',ids:['3'],index:0,quality:'standard'}});
    await wait(()=>one('.playerbar').dataset.trackId==='3'&&!one('[data-player-song-share-dialog]'),
      'native track replacement retires the old UI chooser');
    await emit('player-song-share-release');
    const stale=await pending;
    assert(stale?.code==='stale','held native channel returns stale after replacing the actual player');
    await nativeClipboard('marker');
    const menu=await openTrackActions(false);
    assert(!shareStatus()&&!trackAction('share')&&menu,'blocked replacement has no unguarded share action');
    await captureShare('stale',{clipboardUntouched:true,oldChooserRetired:true,oldPlayId:before.playId,shareHidden:true,stale:stale.code});
    click(trackActionTrigger());
  }
  async function verifyCurrentShareSubscription() {
    const native=window.__TAURI_INTERNALS__;
    const invoke=(command,args)=>native.invoke(command,args);
    const {epoch}=await invoke('account_snapshot');
    const before=await invoke('player_snapshot',{epoch});
    assert(before.track.id==='1'&&before.state==='paused','same-account subscription test has a settled native song');
    const target={playId:before.playId,entryId:before.entryId};
    await nativeClipboard('seed');
    const original=await invoke('player_song_share_prepare',{epoch,target});
    assert(original.value.kind==='ready','original subscription produced an offer');

    let observed;
    const callback=native.transformCallback(message=>{
      if(message.message?.kind==='state') observed=message.message.value;
    },false);
    const streamId='phase316-'+crypto.randomUUID();
    try {
      // Register a new real IPC channel at the same epoch; the published
      // player and selection deliberately remain exactly the same.
      await invoke('watch_player',{epoch,streamId,onUpdate:`__CHANNEL__:${callback}`});
      await wait(()=>observed?.playId===before.playId&&observed?.entryId===before.entryId,
        'replacement native subscription sees the same paused selection');
      const stale=await invoke('player_song_share_choose',{
        epoch,target,offer:original.value.receipt,choice:'copy',
      }).then(()=>null,error=>error);
      assert(stale?.code==='stale','the same song and new channel cannot consume an old share offer');
      await nativeClipboard('marker');
      const fresh=await invoke('player_song_share_prepare',{epoch,target});
      assert(fresh.value.kind==='ready'&&fresh.value.receipt!==original.value.receipt,
        'replacement channel can prepare a new offer');
      await invoke('player_song_share_cancel',{epoch,target,offer:fresh.value.receipt});
      await captureShare('subscription',{originalLeaseRejected:true,clipboardUntouched:true,replacementReady:true,
        entryId:before.entryId,queueRevision:before.queueRevision,epoch});
    } finally {
      await invoke('unwatch_player',{streamId});
      native.unregisterCallback(callback);
    }
  }
  async function capture(label, extra = {}) {
    await sleep(500);
    const panel=one('.lyrics-panel'), body=one('.lyrics-body'), record=one('.vinyl-record'), scroller=one('.lyric-scroll');
    const metadata=all('.vinyl-info h1, .vinyl-info p, .vinyl-info small');
    const credits=all('.vinyl-credits .vinyl-credit, .vinyl-album .vinyl-credit');
    const hintTargets=[one('.vinyl-info h1'), ...credits];
    const controls=all('.playerbar button:not(:disabled), .playerbar input:not(:disabled), .playerbar select:not(:disabled), .lyric-tools button:not(:disabled), .lyric-browse-time:not(:disabled), [aria-label="收起播放详情"]').filter(n=>rect(n).width>0 && rect(n).height>0);
    const player=rect(one('.playerbar'));
    const row={...extra,label,width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,
      panel:rect(panel),body:rect(body),info:rect(one('.vinyl-info')),record:rect(record),player,
      metadata:metadata.map(node=>({tag:node.tagName,...rect(node),title:node.title})),
      controls:controls.map(node=>({label:node.getAttribute('aria-label')||node.textContent.trim(),player:!!node.closest('.playerbar'),...rect(node),hit:hit(node)})),
      scroll:{width:scroller.clientWidth,contentWidth:scroller.scrollWidth,top:scroller.scrollTop},
      queue:one('.queue-panel') ? {rendered:all('.queue-row').length,total:40,...rect(one('.queue-panel'))} : null,
      commentBadge:commentBadge()?.textContent ?? null,
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
      commentBadgeContained:window.__playerLayoutBaseline || (!!commentBadge() &&
        inside(rect(commentBadge()),rect(playerComment())) && hit(playerComment())),
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
  async function verifyLyricsAfterStop() {
    all('.track-row')[2].dispatchEvent(new MouseEvent('dblclick',{bubbles:true}));
    await wait(()=>one('.playerbar')?.dataset.state==='playing'&&one('.playerbar').dataset.trackId==='3','play song with delayed lyrics');
    click(one('[aria-label="歌词"]'));
    await wait(()=>one('.lyrics-column [role="status"]')?.textContent.includes('正在加载歌词'),'delayed lyric read is visible');
    click(one('[aria-label="停止播放"]'));
    await wait(()=>one('.playerbar').dataset.state==='idle','stop during lyric read');
    const cleared=()=>one('.lyrics-column .empty-state')?.textContent.trim()==='暂无播放中的歌曲';
    await wait(cleared,'stopping clears the old lyric spinner');
    await sleep(2300);
    const stopped={cleared:cleared(),title:one('.lyrics-panel > header > span')?.textContent,
      lines:all('.lyric-line').length,offset:!!one('.lyric-tools [aria-label="歌词提前半秒"]')};
    assert(stopped.cleared&&stopped.title?.trim()==='播放详情'&&!stopped.lines&&!stopped.offset,
      'late lyrics never replace the stopped view: '+JSON.stringify(stopped));
    await emit(`ux_${String(++stageCount).padStart(2,'0')}_lyrics_stopped_read`,{cleared:cleared(),playId:one('.playerbar').dataset.playId??null});
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
  async function verifySeekTimePreview() {
    const rail=one('.seek-rail'), input=rail.querySelector('input'), preview=rail.querySelector('.seek-time-preview');
    const visibility=()=>getComputedStyle(preview).visibility;
    const original=Number(one('.playerbar').dataset.position);
    const timeText=seconds=>`${String(Math.floor(seconds/60)).padStart(2,'0')}:${String(Math.floor(seconds%60)).padStart(2,'0')}`;
    assert(visibility()==='hidden','time preview idle is hidden');
    rail.dispatchEvent(new PointerEvent('pointerenter',{pointerType:'mouse'}));
    await wait(()=>visibility()==='visible'&&preview.textContent.trim()===`${timeText(original)} / 00:30`,'hover shows current and full time');
    rail.dispatchEvent(new PointerEvent('pointerleave',{pointerType:'mouse'}));
    await wait(()=>visibility()==='hidden','leaving the seek rail hides the hover preview');
    assert(Math.abs(Number(input.step)-.03)<.0000001,'30-second track uses duration divided by 1000');
    input.value='15';
    input.stepUp();
    assert(Math.abs(Number(input.value)-15.03)<.00001,'native range steps by the scaled interval');
    input.value='29';
    input.dispatchEvent(new Event('input',{bubbles:true}));
    await wait(()=>visibility()==='visible'&&preview.textContent.trim()==='00:29 / 00:30','drag previews target before commit');
    assert(input.getAttribute('aria-valuetext')==='00:29 / 00:30','accessible text follows the draft');
    assert(Number(one('.playerbar').dataset.position)===original,'draft never seeks native playback');
    assert(rect(preview).x>=rect(rail).x-.5&&rect(preview).right<=rect(rail).right+.5,'time preview stays inside rail near its end');
    await capture('seek_time_preview');
    input.dispatchEvent(new PointerEvent('pointercancel',{pointerType:'touch'}));
    await wait(()=>visibility()==='hidden'&&Math.abs(Number(input.value)-original)<.11,'cancel clears the pending preview');
    assert(Number(one('.playerbar').dataset.position)===original,'cancel leaves native playback untouched');
    input.value='2.40';
    input.stepUp();
    input.dispatchEvent(new Event('input',{bubbles:true}));
    input.dispatchEvent(new Event('change',{bubbles:true}));
    await wait(()=>Math.abs(Number(one('.playerbar').dataset.position)-2.43)<.015,'native seek uses the scaled target');
  }
  async function verifySlowPlayback() {
    window.__cardSourceAck = {};
    let sequence=0;
    const configure=async mode=>{
      const id=++sequence;
      await emit('card-source-fixture',{id,mode});
      const field=mode.startsWith('seek-')?'heldSeeking':'forcedBuffering';
      await wait(()=>window.__cardSourceAck[id]?.[field]===mode.endsWith('-on'),'native status fixture command');
    };
    const indicator=()=>one('.seek-load-indicator');
    const playingState=()=>one('.playerbar').dataset.state;
    const identity=one('.playerbar').dataset.playId;
    await configure('buffer-on');
    click(one('.playerbar [aria-label="播放"]'));
    await wait(()=>playingState()==='buffering','short native buffering');
    assert(!indicator(),'no immediate slow loading cue');
    await sleep(700);
    await configure('buffer-off');
    await wait(()=>playingState()==='playing','short buffer recovers');
    assert(!indicator(),'short native buffering does not flash');
    await configure('buffer-on');
    await wait(()=>playingState()==='buffering','second native buffering episode');
    const entered=performance.now();
    await sleep(2250);
    assert(!indicator(),'the second wait did not inherit the first timer');
    await wait(indicator,'sustained buffering shows seek thumb cue');
    assert(performance.now()-entered>=2700,'cue waits for continuous buffering');
    const root=document.documentElement, previous=root.dataset.motion;
    if(!matchMedia('(prefers-reduced-motion: reduce)').matches) {
      root.dataset.motion='full';
      assert(getComputedStyle(indicator()).animationName==='seek-load-pulse','full motion pulses the thumb');
    }
    root.dataset.motion='reduced';
    assert(getComputedStyle(indicator()).animationName==='none','reduced motion keeps a static cue');
    if(previous===undefined) delete root.dataset.motion; else root.dataset.motion=previous;
    await capture('slow_playback_wait');
    await configure('buffer-off');
    await wait(()=>playingState()==='playing'&&!indicator(),'cue clears when native playback resumes');
    assert(one('.playerbar').dataset.playId===identity,'buffering does not replace the song');
    const input=one('.playerbar [aria-label="播放进度"]');
    await configure('seek-on');
    await wait(()=>!input.disabled,'ordinary playback allows seek');
    input.value='14.01';
    input.dispatchEvent(new Event('input',{bubbles:true}));
    input.dispatchEvent(new Event('change',{bubbles:true}));
    await wait(()=>Math.abs(Number(one('.playerbar').dataset.position)-14.01)<.015&&!input.disabled,
      'ordinary native seek accepted and IPC is no longer pending');
    assert(playingState()==='playing'&&!indicator(),'seek wait is not buffering and does not flash');
    const accepted=performance.now();
    // The real decoder is not blocked by this status-only test fixture. Fail
    // within the held entry, before a long wait could run into its natural EOF.
    for(let i=0;i<55&&!indicator();i++) await sleep(100);
    assert(indicator(),'sustained native seek still shows the thumb cue after IPC');
    assert(performance.now()-accepted>=2400,'native seek cue retains the delayed threshold');
    assert(playingState()==='playing'&&one('.playerbar').dataset.playId===identity,
      'seek cue does not change playback intent or identity');
    await capture('slow_native_seek_wait',{playId:identity});
    await configure('seek-off');
    await wait(()=>!indicator()&&Number(one('.playerbar').dataset.position)>14.1,'seek cue clears and same clock advances');
    click(one('.playerbar [aria-label="暂停"]'));
    await wait(()=>playingState()==='paused','pause after native buffering proof');
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
    const centerError = () => {
      const row = rect(lyric), viewport = rect(scroller);
      return (row.y + row.bottom - viewport.y - viewport.bottom) / 2;
    };
    // Move only the lyrics viewport, so an ancestor scroll or an earlier
    // smooth follow cannot determine where this manual gesture lands.
    scroller.scrollTo({top:scroller.scrollTop + centerError(),behavior:'instant'});
    try {
      await wait(()=>Math.abs(centerError())<=2 && marker()?.textContent.trim()==='00:07' &&
        lyric.classList.contains('browsed'),'centered lyric timeline',30);
    } catch (error) {
      // The paused browse marker expires after five seconds. Capture geometry
      // while it is still meaningful instead of extending the general wait.
      throw Error(`${error.message}: ${JSON.stringify({ scrollTop:scroller.scrollTop,
        centerError:centerError(), marker:marker()?.textContent.trim(),
        browsed:scroller.querySelector('.lyric-line.browsed')?.dataset.line,
        browsing:scroller.parentElement.classList.contains('browsing') })}`);
    }
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
  async function verifyAudioOutputRecovery() {
    const bar=()=>one('.playerbar');
    const settings=()=>all('.playback-message button').find(node=>node.textContent.trim()==='音频输出设置');
    const retry=()=>all('.playback-message button').find(node=>node.textContent.trim()==='重试');
    assert(!settings(),'normal playback does not expose output recovery action');
    window.__cardSourceAck={};
    await emit('card-source-fixture',{id:1,mode:'fail-next-output'});
    await wait(()=>window.__cardSourceAck[1]?.pendingOutputFailure,'arm native audio output failure');
    all('.track-row')[0].dispatchEvent(new MouseEvent('dblclick',{bubbles:true}));
    await wait(()=>bar().dataset.state==='error'&&settings()?.disabled===false&&retry()?.disabled===false,'output failure exposes settings and explicit retry');
    const failedId=bar().dataset.playId;
    assert(failedId,'failed selection remains addressable');
    click(settings());
    const heading=()=>one('#settings-audio-output');
    await wait(()=>heading()&&document.activeElement===heading()&&hit(heading()),'audio output settings focuses and scrolls to the correct section');
    assert(bar().dataset.state==='error'&&bar().dataset.playId===failedId,'settings navigation does not retry or replace audio');
    await emit(`ux_${String(++stageCount).padStart(2,'0')}_audio_output_recovery`,{target:heading().textContent,failedId});
    await sleep(800);
    click(retry());
    await wait(()=>bar().dataset.state==='playing'&&Number(bar().dataset.position)>.15&&!settings(),'explicit retry recovers native playback');
    click(one('.playerbar [aria-label="暂停"]'));
    await wait(()=>bar().dataset.state==='paused','pause recovered song');
  }
  async function verifyRecommendationToggle() {
    const bar=()=>one('.playerbar');
    const button=()=>one('.daily-track-actions [data-recommendation-play]');
    const text=()=>button()?.textContent.trim();
    const {playId,trackId,position}=bar().dataset;
    await wait(()=>text()==='继续播放'&&!button().disabled,'same recommendation resumes from the header');
    click(button());
    await wait(()=>text()==='暂停'&&bar().dataset.state==='playing'&&!button().disabled,'header resumes existing recommendation playback');
    assert(bar().dataset.playId===playId&&bar().dataset.trackId===trackId&&Number(bar().dataset.position)>=Number(position)-.1,'resume keeps the current track and position');
    click(button());
    await wait(()=>text()==='继续播放'&&bar().dataset.state==='paused'&&!button().disabled,'header pauses existing recommendation playback');
    await wait(()=>document.activeElement===button(),'header toggle preserves keyboard focus');
    assert(bar().dataset.playId===playId,'pause keeps the same play ID');
    await emit(`ux_${String(++stageCount).padStart(2,'0')}_daily_play_toggle`,{playId,trackId,button:text()});
    await sleep(800);
    click(one('[aria-label="刷新当前页面"]'));
    await wait(()=>text()==='播放全部'&&!button().disabled,'new recommendation snapshot exposes start action');
    assert(bar().dataset.playId===playId&&bar().dataset.state==='paused','refresh leaves the paused queue intact');
  }
  async function verifyCurrentRowButton() {
    const bar=()=>one('.playerbar');
    const {playId,trackId,position}=bar().dataset;
    const row=()=>one(`.track-row[data-track-id="${trackId}"] .track-play`);
    await wait(()=>row()?.getAttribute('aria-label').startsWith('继续播放 ')&&!row().disabled,'current song row offers continue');
    click(row());
    await wait(()=>row()?.getAttribute('aria-label').startsWith('暂停 ')&&bar().dataset.state==='playing'&&!row().disabled,'current song row resumes');
    assert(bar().dataset.playId===playId&&Number(bar().dataset.position)>=Number(position)-.1,'row resume preserves session and position');
    click(row());
    await wait(()=>row()?.getAttribute('aria-label').startsWith('继续播放 ')&&bar().dataset.state==='paused'&&!row().disabled,'current song row pauses');
    await wait(()=>document.activeElement===row(),'current row toggle preserves focus');
    assert(bar().dataset.playId===playId,'row pause preserves the active session');
    await emit(`ux_${String(++stageCount).padStart(2,'0')}_current_row_toggle`,{playId,trackId});
    await sleep(800);
  }
  async function verifyTrackActions() {
    await emit('resize',{width:760,height:540});
    await wait(()=>innerWidth===760&&innerHeight===540,'more menu narrow window');
    const bar = one('.playerbar');
    const {playId, trackId, position} = bar.dataset;
    const menu = await openTrackActions();
    assert(Array.from(menu.querySelectorAll('[data-menu-key]'),item=>item.dataset.menuKey).join() === 'share,artist,album,effects','uses playback-specific actions rather than the row menu');
    assert(trackAction('artist').textContent.trim()==='歌手：测试歌手'&&trackAction('album').textContent.trim()==='专辑：测试专辑','labels use first credited artist and current album');
    assert(inside(rect(menu),{x:0,y:0,right:innerWidth,bottom:innerHeight})&&rect(menu).bottom<=rect(trackActionTrigger()).y,'narrow menu fits above the trigger');
    await emit(`ux_${String(++stageCount).padStart(2,'0')}_current_track_menu`,{playId,trackId,menu:true});
    await sleep(800);
    await verifyCurrentShare();
    await openTrackActions();
    click(trackAction('effects'));
    const activeCatalogue = '.audio-effects-panel[open] [data-audio-effect-pane="catalogue"]:not([hidden])';
    const legacyDirectory = activeCatalogue + ' .effect-directory[data-effect-layout="legacy"]';
    const groupIds = group => all(legacyDirectory + ` [data-effect-group="${group}"] [data-effect-id]`)
      .map(row => row.dataset.effectId).join();
    await wait(()=>one('.audio-effects-panel[open] [data-audio-effect-tab="catalogue"][aria-selected="true"]')
      && groupIds('legacy_recommended')==='201' && groupIds('legacy_all')==='202',
      'current-song effects opens the default tab and renders both legacy groups from native reads');
    click(one('.audio-effects-panel [aria-label="关闭音效"]'));
    await wait(()=>!one('.audio-effects-panel')&&document.activeElement===trackActionTrigger(),
      'effects closes back to the playback trigger');
    await openTrackActions();
    click(trackAction('artist'));
    await wait(()=>one('.artist-header h1')?.textContent==='测试歌手','the first artist route loads');
    await openTrackActions();
    click(trackAction('album'));
    await wait(()=>one('.album-header h1')?.textContent.trim()==='测试专辑','the current album route loads');
    assert(bar.dataset.playId===playId&&bar.dataset.trackId===trackId&&bar.dataset.state==='paused'&&Math.abs(Number(bar.dataset.position)-Number(position))<.1,'actions preserve the native song and paused position');
    click(all('.sidebar button').find(node=>node.textContent.trim()==='每日推荐'));
    await wait(()=>one('.daily-track-actions')&&!one('.catalog-page')&&all('.track-row').length>2,'restore the daily fixture for the output recovery case');
    await verifyCurrentShareRetirement();
    await emit('resize',{width:1120,height:760});
    await wait(()=>innerWidth===1120&&innerHeight===760,'restore the output recovery viewport');
  }
  try {
    await wait(()=>all('.track-row').length>2,'daily');
    await wait(()=>one('[aria-label="列表循环"]') && !one('[aria-label="列表循环"]').disabled,'native player ready');
    assert(one('.track-row').textContent.includes('沿着海岸'),'long fixture missing');
    if(!window.__playerLayoutBaseline) await verifyLyricsAfterStop();
    await play(0);
    const firstCommentPlay = one('.playerbar').dataset.playId;
    if(!window.__playerLayoutBaseline) {
      const mark=()=>one('.seek-chorus');
      await wait(()=>mark(),'chorus metadata appears on the playing track');
      assert(Math.abs(parseFloat(mark().style.left)-400/30)<.01&&!mark().classList.contains('reached'),'chorus marker uses full track clock');
      await wait(()=>commentBadge()?.textContent==='999+'&&commentDescription()?.textContent.trim()==='1234 条评论','current song exposes compact and accessible comment counts');
      await verifySeekTimePreview();
      await verifyLyricModes();
      await verifyLyricFollow();
      await wait(()=>mark()?.classList.contains('reached'),'chorus marker reflects passed progress');
      await verifyLyricFont();
      await verifySlowPlayback();
    }
    for(const [width,height,theme] of [[1120,760,'light'],[760,540,'light'],[760,540,'dark'],[1120,760,'dark']]) {
      await emit('resize',{width,height});await wait(()=>innerWidth===width&&innerHeight===height,'resize');await appearance(theme);
      const suffix=`${width}_${theme}`;
      one('.lyric-line.current').scrollIntoView({block:'center',behavior:'instant'});
      const hoverNarrowDark=!window.__playerLayoutBaseline&&width===760&&theme==='dark';
      if(hoverNarrowDark) {
        one('.seek-rail').dispatchEvent(new PointerEvent('pointerenter',{pointerType:'mouse'}));
        await wait(()=>getComputedStyle(one('.seek-time-preview')).visibility==='visible','narrow dark seek preview');
        assert(inside(rect(one('.seek-time-preview')),{x:0,y:0,right:innerWidth,bottom:innerHeight}),'narrow seek preview stays inside window');
      }
      await capture(`long_${suffix}`);
      if(hoverNarrowDark)
        one('.seek-rail').dispatchEvent(new PointerEvent('pointerleave',{pointerType:'mouse'}));
      const scroll=one('.lyric-scroll');scroll.dispatchEvent(new WheelEvent('wheel',{bubbles:true,deltaY:500}));scroll.scrollTop=scroll.scrollHeight;
      await capture(`lyrics_bottom_${suffix}`);
      click(one('[aria-label="播放队列"]'));await wait(()=>all('.queue-row').length,'queue');await capture(`queue_${suffix}`);
      one('.queue-scroll').scrollTop=one('.queue-scroll').scrollHeight;await capture(`queue_bottom_${suffix}`);
      click(one('[aria-label="关闭播放队列"]'));await wait(()=>!one('.queue-panel'),'queue close');
    }
    if(!window.__playerLayoutBaseline) {
      const menu = await openTrackActions();
      assert(trackAction('artist').disabled&&trackAction('album').disabled&&menu.scrollWidth<=menu.clientWidth+1,'missing link IDs cannot navigate, and long names do not overflow');
    }
    await play(1);
    if(!window.__playerLayoutBaseline) {
      assert(!trackActionMenu(),'track replacement dismisses the old current-song menu');
      await wait(()=>commentBadge()?.textContent==='25'&&commentDescription()?.textContent.trim()==='25 条评论','replacement song shows its own count');
      const invoke = (command,args) => window.__TAURI_INTERNALS__.invoke(command,args);
      const {epoch} = await invoke('account_snapshot');
      const stale = await invoke('player_comment_count',{epoch,playId:firstCommentPlay}).then(()=>null,cause=>cause);
      assert(stale?.code==='stale','previous play ID cannot read current-song comment count');
    }
    if(!window.__playerLayoutBaseline)
      assert(!one('[aria-label="翻译歌词"]')&&!one('[aria-label="音译歌词"]')&&one('[aria-label="歌词提前半秒"]'),'original-only song does not show auxiliary toggles');
    if(!window.__playerLayoutBaseline) assert(!one('.seek-chorus'),'replacement song does not reuse previous marker');
    for(const [width,height,theme] of [[1120,760,'light'],[760,540,'light'],[760,540,'dark'],[1120,760,'dark']]) {
      await emit('resize',{width,height});await wait(()=>innerWidth===width&&innerHeight===height,'resize');await appearance(theme);
      await capture(`short_${width}_${theme}`);
    }
    click(one('[aria-label="收起播放详情"]'));await wait(()=>!one('.lyrics-panel'),'details close');await sleep(300);
    assert(document.activeElement===one('[aria-label="歌词"]'),'detail close focus lost');
    if(!window.__playerLayoutBaseline) { await verifyCurrentRowButton(); await verifyRecommendationToggle(); await verifyTrackActions(); await verifyAudioOutputRecovery(); await verifyCurrentShareSubscription(); }
    const passed=checks.every(row=>Object.entries(row).every(([key,value])=>key==='label'||value===true));
    assert(window.__playerLayoutBaseline||passed,JSON.stringify(checks.filter(row=>Object.values(row).includes(false))));
    await emit('complete',{ok:true,layoutPassed:passed,checks,stageCount,realMusicRequests:0,accountWrites:0,focusRestored:true,lyricFollowVerified:!window.__playerLayoutBaseline,lyricTimelineVerified:!window.__playerLayoutBaseline,lyricFontVerified:!window.__playerLayoutBaseline,lyricModesVerified:!window.__playerLayoutBaseline,lyricStopVerified:!window.__playerLayoutBaseline,chorusMarkerVerified:!window.__playerLayoutBaseline,seekTimePreviewVerified:!window.__playerLayoutBaseline,slowPlaybackWaitVerified:!window.__playerLayoutBaseline,seekStepVerified:!window.__playerLayoutBaseline,audioOutputRecoveryVerified:!window.__playerLayoutBaseline,recommendationToggleVerified:!window.__playerLayoutBaseline,currentRowToggleVerified:!window.__playerLayoutBaseline,playerCommentCountVerified:!window.__playerLayoutBaseline,currentTrackMenuVerified:!window.__playerLayoutBaseline});
  } catch(error) {await emit('failed',{ok:false,error:String(error),stageCount,checks});}
})();
