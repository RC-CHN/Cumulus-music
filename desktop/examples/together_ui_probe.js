(async () => {
  if (window.__togetherStarted) return;
  window.__togetherStarted = true;
  const input = window.__togetherProof;
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const el = selector => document.querySelector(selector);
  const all = selector => [...document.querySelectorAll(selector)];
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const report = (stage, data = {}) => invoke('plugin:event|emit', {event: 'together-proof', payload: {stage, ...data}});
  const wait = async (test, label, ms = 90000) => {
    const deadline = Date.now() + ms;
    while (Date.now() < deadline) {const result = await test(); if (result) return result; await sleep(180);}
    throw Error(label);
  };
  const button = (text, scope = document) => [...scope.querySelectorAll('button')].find(node => node.textContent.trim() === text);
  const click = async node => {
    if (!node || node.disabled) throw Error('button_not_ready:' + (node?.getAttribute('aria-label') || node?.textContent.trim() || 'missing'));
    node.scrollIntoView({block: 'nearest'}); node.click(); await sleep(160);
  };
  const stage = async (name, data = {}) => {
    await sleep(350);
    await report(name, {...data, width: innerWidth, height: innerHeight, horizontalOverflow: document.documentElement.scrollWidth > innerWidth, verticalOverflow: document.documentElement.scrollHeight > innerHeight});
    await sleep(1100);
  };
  let epoch, step = 'bootstrap';
  const native = async () => (await invoke('together_view', {epoch, refresh: false})).value;
  const settled = async predicate => {
    const value = await native();
    return !value.pending.length && !value.syncError && predicate(value) ? value : false;
  };
  const paused = () => el('.playerbar')?.dataset.state === 'paused';
  const playing = () => el('.playerbar')?.dataset.state === 'playing';
  async function pause() {
    if (!paused()) {await click(el('.playerbar [aria-label="暂停"]')); await wait(paused, 'native_pause');}
  }
  async function seek(seconds) {
    const range = el('[aria-label="播放进度"]');
    range.value = String(seconds); range.dispatchEvent(new Event('change', {bubbles: true}));
    await wait(() => Math.abs(Number(el('.playerbar').dataset.position) - seconds) < .5, 'native_seek');
  }
  try {
    await wait(() => all('.track-row .track-play').some(n => !n.disabled), 'daily_loaded');
    epoch = (await invoke('account_snapshot')).epoch;
    await click(all('.track-row .track-play').find(n => !n.disabled));
    await wait(() => playing() && Number(el('.playerbar').dataset.position) > .3, 'mpv_decoded');
    await click(el('.playerbar [aria-label="播放速度"]'));
    await wait(()=>el('.speed-action-menu'),'speed_menu');
    await click(all('.speed-action-menu button').find(n=>n.textContent.trim()==='1.25×'));
    await wait(() => Number(el('.playerbar').dataset.speed) === 1.25, 'speed_preference');
    for (let i = 0; i < 3; i++) {await click(el('.transport-controls button')); await sleep(200);}
    await wait(() => el('.transport-controls button').getAttribute('aria-label') === '顺序播放', 'mode_preference');
    await click(button('一起听'));
    await wait(() => button('创建房间') && !button('创建房间').disabled, 'empty_room_page');
    await stage('together_empty');
    step = 'create'; await click(button('创建房间'));
    const created = await wait(() => settled(v => v.attached && v.tracks.length > 1 && v.playing), 'queue_published');
    await wait(() => all('.together-track').length > 1 && !el('.together-pending') && Number(el('.playerbar').dataset.speed) === 1, 'room_rendered');
    await stage('together_created', {count: created.tracks.length, attached: true, realTimeReady: created.realtimeReady});
    if(input.im){
      step='im_connected';
      const connected=await wait(async()=>{const v=await native();return v.connection.state==='connected'&&v.connection.received>0?v:false;},'native_im_enter_and_receive');
      await wait(()=>el('.together-connection')?.dataset.state==='connected'&&button('重新连接'),'im_connected_ui');
      await stage('together_im_connected',{connection:connected.connection,realtimeReady:connected.realtimeReady});
      await pause();
      await wait(()=>settled(v=>!v.playing),'pause_before_im_reconnect');
      await stage('together_im_before_reconnect');
      await click(button('重新连接'));
      const reconnected=await wait(async()=>{const v=await native();return v.connection.state==='connected'&&v.connection.received>0&&!button('重新连接')?.disabled?v:false;},'native_im_reentered');
      await stage('together_im_reconnected',{connection:reconnected.connection,stillPaused:paused()});
      if(!input.close){await click(el('.playerbar [aria-label="播放"]'));await wait(()=>settled(v=>v.playing),'resume_after_im_reconnect');}
    }
    if (input.close) {
      step = 'close_uncertain';
      await report('close-request', {dropAck: true});
      await wait(() => all('.together-page [role="alert"]').some(n => n.textContent.includes('窗口已保留')) && el('.together-pending button'), 'close_error_and_pending');
      await stage('together_close_failed', {windowRetained: true, pending: (await native()).pending.length});
      await click(el('.together-pending button'));
      await wait(() => settled(v => !v.room && v.localRecovery), 'closed_room_readback');
      await stage('together_close_reconciled', {pending: 0, localRecovery: true});
      await report('close-final-ready', {ui_ok: true});
      await report('close-request', {dropAck: false});
      return;
    }
    step = 'lost_pause'; await report('inject-play-loss'); await pause();
    await wait(() => el('.together-pending button') && !el('.together-pending button').disabled, 'uncertain_visible');
    const uncertain = await native();
    if (uncertain.pending.length !== 1 || uncertain.pending[0].action !== 'play') throw Error('wrong_pending');
    await stage('together_uncertain', {pending: 1, nativePaused: paused(), publishDisabled: button('重新同步当前播放').disabled});
    // Local playback continues to work while room writes are fenced.
    await seek(12);
    await click(el('.together-pending button'));
    await wait(() => !el('.together-pending') && !button('重新同步当前播放').disabled, 'readback_complete');
    await click(button('重新同步当前播放'));
    const republished = await wait(() => settled(v => !v.playing && Math.abs(v.progressMs - 12000) < 500 && Math.abs(v.progressMs - Number(el('.playerbar').dataset.position) * 1000) < 500), 'republish_actual_native_position');
    await stage('together_reconciled', {pending: 0, requestedMs: 12000, reportedMs: republished.progressMs, nativePosition: Number(el('.playerbar').dataset.position)});
    step = 'zero_seek';
    await click(el('.playerbar [aria-label="播放"]'));
    await wait(() => settled(v => v.playing), 'resume_reported');
    await seek(0);
    const zero = await wait(() => settled(v => v.playing && v.progressMs < 500), 'explicit_zero_reported');
    await stage('together_zero_seek', {playing: true, requestedMs: 0, reportedMs: zero.progressMs});
    step = 'controls';
    const old = (await native()).targetId;
    await click(el('.playerbar [aria-label="下一首"]'));
    await wait(() => settled(v => v.targetId && v.targetId !== old), 'next_reported');
    await wait(() => playing(), 'next_decoded'); await pause();
    await wait(() => settled(v => !v.playing), 'pause_reported');
    await seek(18); const sought = await wait(() => settled(v => Math.abs(v.progressMs - 18000) < 500 && Math.abs(v.progressMs - Number(el('.playerbar').dataset.position) * 1000) < 500), 'seek_reported');
    await stage('together_controls', {next: true, pause: true, requestedMs: 18000, reportedMs: sought.progressMs, nativePosition: Number(el('.playerbar').dataset.position)});
    step = 'queue';
    const before = (await native()).tracks.length;
    const row = all('.together-track').find(n => !n.classList.contains('current'));
    await click(row.querySelector('[aria-label="从共享队列移除"]'));
    await wait(() => settled(v => v.tracks.length === before - 1), 'queue_remove_reported');
    await click(el('.transport-controls button'));
    await wait(() => settled(v => v.mode === 'SINGLE_LOOP'), 'shared_mode_changed');
    await wait(() => !el('.together-pending') && !button('重新同步当前播放').disabled, 'shared_mode_rendered');
    el('.together-queue').scrollIntoView({block: 'start'});
    await stage('together_queue', {count: before - 1, mode: 'SINGLE_LOOP'});
    step = 'exit'; await click(button('退出房间')); await click(button('确认退出'));
    await wait(() => settled(v => !v.room && !v.localRecovery), 'exit_confirmed');
    await wait(() => Number(el('.playerbar').dataset.speed) === 1.25, 'speed_restored');
    if (el('.transport-controls button').getAttribute('aria-label') !== '单曲循环') throw Error('user_mode_change_lost');
    await stage('together_exit', {roomAbsent: true, speedRestored: true, userModePreserved: true});
    await report('complete', {ok: true, uiCreate: true, queue: true, controls: true, zeroSeek: true, lostAckReconciled: true, uiExit: true, pending: 0, audioOutput: 'null'});
  } catch (error) {
    await report('failed', {ok: false, step, error: error instanceof Error ? error.message : 'native_failure', pageErrors: all('.together-page [role="alert"]').map(n => n.textContent), playerError: el('.player-error')?.textContent});
  }
})();
