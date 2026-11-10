// Use the real library and player UI over a closed CUE and song-lyric fixture.
(async () => {
  if (window.__matchedLocalLyricProofStarted) return;
  window.__matchedLocalLyricProofStarted = true;
  window.__lyricAck = {};
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = payload => invoke('plugin:event|emit', { event: 'desktop-lyric-proof', payload });
  const one = selector => document.querySelector(selector);
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const assert = (ok, label) => { if (!ok) throw Error(label); };
  async function wait(test, label) {
    const end = performance.now() + 20000;
    while (performance.now() < end) {
      const result = await test();
      if (result) return result;
      await sleep(60);
    }
    throw Error(label);
  }
  let serial = 0;
  async function fixture(kind, payload = {}) {
    const id = ++serial;
    await emit({ kind, id, ...payload });
    const ack = await wait(() => window.__lyricAck[id], 'fixture acknowledgement: ' + kind);
    delete window.__lyricAck[id];
    if (ack.error) throw Error(ack.error);
    return ack.value ?? ack;
  }
  function click(node) {
    assert(node && !node.disabled, 'available UI control');
    node.focus({ preventScroll: true });
    node.click();
  }
  async function capture(stage, surface, facts) {
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await emit({ kind: 'snapshot', stage, surface, ...facts });
    // Preserve the visible checkpoint while the existing X11 driver captures it.
    await sleep(1100);
  }
  const localTitle = '合成本地未匹配';
  const mappedTitle = '合成本地已匹配';
  const localLine = '原文件旁挂第一句';
  const onlineLine = '线上匹配歌词第一句';
  let epoch, step = 'bootstrap';
  const snapshot = () => invoke('player_snapshot', { epoch });
  function localRow(title) {
    return [...document.querySelectorAll('.local-tracks button')].find(button =>
      !button.disabled && button.getAttribute('aria-label') === '播放本地歌曲 ' + title);
  }
  async function playFromLibrary(title, mediaStart) {
    const row = await wait(() => localRow(title), 'playable indexed CUE row: ' + title);
    const localId = row.closest('[data-local-id]')?.dataset.localId;
    assert(localId && localId !== '123', 'play comes from the indexed CUE row, not the mapped song ID');
    click(row);
    const playing = await wait(async () => {
      const state = await snapshot();
      return state.state === 'playing' && state.track?.id === localId && state.position > .08 && state;
    }, 'actual local decoder clock advances: ' + title);
    assert(playing.track.local && playing.timeline?.mode === 'cue' &&
      playing.timeline.media_start === mediaStart, 'native CUE source and physical start');
    const queue = await invoke('player_queue', { epoch });
    assert(queue.tracks.length === 2 && queue.tracks.every(item => item.track.local) &&
      queue.tracks.some(item => item.track.id === localId), 'two real local queue entries');
    const paused = await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    assert(paused.state === 'paused' && paused.playId === playing.playId, 'pause the same local playback');
    click(one('[aria-label="打开播放详情"]'));
    return paused;
  }
  async function mainLyrics(expected, fromSidecar, play) {
    await wait(() => {
      const panel = one('.lyrics-panel');
      const text = panel?.querySelector('.lyric-scroll')?.textContent;
      return text?.includes(expected) && getComputedStyle(panel).opacity === '1' &&
        !panel.classList.contains('lyrics-panel-enter-active') && text;
    }, 'visible main lyrics: ' + expected);
    const document = await invoke('player_lyrics', { epoch, playId: play.playId, cachedOnly: true });
    assert(document.playId === play.playId && document.fromSidecar === fromSidecar &&
      !document.fromEmbedded && !document.networkFailed, 'same native lyric document and provenance');
    assert(document.lines[0]?.time === 0 && document.lines[0].lyric === expected &&
      document.lines[1]?.time === 2, 'lyrics retain the expected song-relative timestamps');
    assert(!document.contribution && !document.toolbarContributions?.length &&
      !one('[data-lyric-contribution],[data-lyric-bounty]'), 'local match cannot expose contribution actions');
    if (!fromSidecar) {
      assert(!one('.lyrics-panel .lyric-cache-note')?.textContent.includes('本地歌词') &&
        !document.lines.some(line => line.lyric.includes('旁挂') || line.lyric.includes('不应出现在')),
      'mapped lyrics cannot read the original-file sidecar');
    }
    return { playId: play.playId, fromSidecar, lyricTimes: document.lines.map(line => line.time),
      overflow: window.document.documentElement.scrollWidth > innerWidth };
  }

  try {
    const account = await wait(async () => {
      const state = await invoke('account_snapshot');
      return state.status === 'authenticated' && one('.playerbar') && state;
    }, 'synthetic account initialized');
    epoch = account.epoch;
    const entry = await wait(() => [...document.querySelectorAll('button')].find(node =>
      (node.textContent.trim() === '本地音乐' || node.getAttribute('aria-label') === '本地音乐') &&
      !node.disabled), 'local library navigation');
    click(entry);
    await wait(() => one('.local-page'), 'local page rendered');
    await fixture('watch_matched');
    await wait(() => localRow(localTitle) && localRow(mappedTitle), 'both generated CUE rows are indexed');
    await fixture('link_matched');

    step = 'unmatched';
    const unmatched = await playFromLibrary(localTitle, 0);
    const unmatchedMain = await mainLyrics(localLine, true, unmatched);
    assert((await fixture('stats')).requests === 0, 'unmatched CUE does not ask for online lyrics');
    await capture('ux_01_unmatched_main', 'main', unmatchedMain);
    click(one('[aria-label="收起播放详情"]'));
    await wait(() => !one('.lyrics-panel'), 'leave unmatched track details');

    step = 'matched';
    const matched = await playFromLibrary(mappedTitle, 4);
    assert(matched.playId !== unmatched.playId, 'second CUE selection has a distinct playback identity');
    const matchedMain = await mainLyrics(onlineLine, false, matched);
    assert((await fixture('stats')).requests === 1, 'one matching-song lyric read');
    await capture('ux_02_matched_main', 'main', matchedMain);

    step = 'desktop';
    click(one('[aria-label="收起播放详情"]'));
    await wait(() => !one('.lyrics-panel'), 'leave matched track details');
    click(one('[aria-label="打开设置"]'));
    const show = await wait(() => {
      const input = one('[aria-label="显示桌面歌词"]');
      return input && !input.disabled && !input.checked && input;
    }, 'desktop lyrics setting ready');
    show.scrollIntoView({ block: 'center', behavior: 'instant' });
    click(show);
    await wait(async () => (await invoke('desktop_lyric_settings', { epoch })).visible,
      'desktop lyric window visible');
    const desktop = await wait(() => fixture('eval', { source: [
      "const state=await window.__TAURI_INTERNALS__.invoke('desktop_lyric_read',{generation:location.hash.split('=')[1]});",
      "const row=[...document.querySelectorAll('.desktop-lyric-row')].find(node=>node.textContent.includes(" +
        JSON.stringify(onlineLine) + "));",
      "if(state.state!=='ready'||!row) return false;",
      "const bounds=row.getBoundingClientRect();",
      "return {playId:state.lyrics.playId,fromSidecar:state.lyrics.info.fromSidecar,",
      "fromEmbedded:state.lyrics.info.fromEmbedded,networkFailed:state.lyrics.info.networkFailed,locked:state.locked,",
      "overflow:document.documentElement.scrollWidth>innerWidth,",
      "paintedRow:document.visibilityState==='visible'&&bounds.width>0&&bounds.height>0&&bounds.top>=0&&bounds.bottom<=innerHeight};",
    ].join('') }), 'desktop lyric displays the same exact online line');
    assert(desktop.playId === matched.playId && !desktop.fromSidecar && !desktop.fromEmbedded &&
      !desktop.networkFailed && desktop.paintedRow, 'both surfaces share the exact native online document');
    await capture('ux_03_matched_desktop', 'lyric', desktop);
    const current = await snapshot();
    assert(current.playId === matched.playId && current.queueRevision === matched.queueRevision &&
      current.state === 'paused' && current.position === matched.position &&
      current.timeline.media_start === 4, 'lyrics never replace, resume or seek the selected local audio');
    assert((await fixture('stats')).requests === 1, 'both WebViews share the same single song-lyric read');
    await emit({ kind: 'complete', stage: 'complete', ok: true, stageCount: 3, realMusicRequests: 0,
      sameNativePlayback: true });
  } catch (error) {
    await emit({ kind: 'failed', stage: 'failed', step, error: String(error?.message ?? error) });
  }
})();
