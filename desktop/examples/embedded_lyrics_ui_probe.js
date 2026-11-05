// Production local playback and both lyric WebViews; only frozen synthetic audio.
(async () => {
  if (window.__embeddedLyricProofStarted) return;
  window.__embeddedLyricProofStarted = true;
  window.__lyricAck = {};
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = payload => invoke('plugin:event|emit', { event: 'desktop-lyric-proof', payload });
  const one = selector => document.querySelector(selector);
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const assert = (ok, label) => { if (!ok) throw Error(label); };
  async function wait(test, label, delay = 50) {
    for (let i = 0; i < 300; i++) {
      const result = await test();
      if (result) return result;
      await sleep(delay);
    }
    throw Error(label);
  }
  let serial = 0;
  const nativeSidecar = window.__nativeSidecarLyricProof === true;
  const expectedText = nativeSidecar ? '未扫描也能显示歌词' : '保留歌词';
  const sourceFlag = nativeSidecar ? 'fromSidecar' : 'fromEmbedded';
  const caseName = nativeSidecar ? 'native_sidecar' : 'embedded';
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
    await new Promise((resolve, reject) => {
      const timeout = setTimeout(() => reject(Error('main paint did not advance')), 3000);
      requestAnimationFrame(() => requestAnimationFrame(() => {
        clearTimeout(timeout);
        resolve();
      }));
    });
    await emit({ kind: 'snapshot', stage, surface, ...facts });
    // Existing screenshot driver samples the isolated display while this
    // checkpoint remains stable.
    await sleep(1100);
  }

  try {
    const entry = await wait(() => [...document.querySelectorAll('button')].find(node =>
      (node.textContent.trim() === '本地音乐' || node.getAttribute('aria-label') === '本地音乐') &&
      !node.disabled), 'local library navigation');
    click(entry);
    await wait(() => one('.local-page'), 'local page rendered');
    let opened;
    if (nativeSidecar) {
      opened = await fixture('open_native_sidecar');
      assert(opened.unindexed && !!opened.playId && !one('.local-tracks tbody tr'),
        'explicit native open leaves the real local library empty');
    } else {
      await fixture('index_embedded');
      const track = await wait(() => {
        const rows = [...document.querySelectorAll('.local-tracks tbody tr')];
        const play = rows[0]?.querySelector('button[aria-label^="播放本地歌曲"]');
        return rows.length === 1 && play && !play.disabled && play;
      }, 'only frozen tagged FLAC indexed');
      click(track);
    }
    const { epoch } = await invoke('account_snapshot');
    const playback = await wait(async () => {
      const state = await invoke('player_snapshot', { epoch });
      return state.state === 'playing' && state.track?.local && state.position > .08 && state;
    }, 'native local playback clock advanced', 20);
    if (nativeSidecar) assert(playback.playId === opened.playId, 'exact native play receipt');
    const paused = await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    assert(paused.state === 'paused' && paused.playId === playback.playId, 'native playback paused on the same entry');

    click(one('[aria-label="打开播放详情"]'));
    await wait(() => one('.lyrics-panel .lyric-scroll')?.textContent.includes(expectedText) &&
      one('.lyrics-panel .lyric-cache-note')?.textContent.includes('正在显示本地歌词'),
    'main lyrics text and local notice');
    const main = await invoke('player_lyrics', { epoch, playId: playback.playId });
    assert(main.fromEmbedded === !nativeSidecar && main.fromSidecar === nativeSidecar &&
      !main.networkFailed && main.scrollable && main.lines.some(line => line.time === 0 && line.lyric === expectedText),
    'main IPC local provenance and timestamp');
    if (nativeSidecar) assert(!JSON.stringify(main).includes('.wav'), 'no native file path in lyric DTO');
    await capture(`ux_01_${caseName}_main`, 'main', {
      fromEmbedded: main.fromEmbedded, fromSidecar: main.fromSidecar, timedLine: true,
      nativeClockAdvanced: true, unindexed: opened?.unindexed ?? false,
      playId: playback.playId, overflow: document.documentElement.scrollWidth > innerWidth,
    });

    click(one('[aria-label="收起播放详情"]'));
    await wait(() => !one('.lyrics-panel'), 'main details closed');
    click(one('[aria-label="打开设置"]'));
    const show = await wait(() => {
      const input = one('[aria-label="显示桌面歌词"]');
      return input && !input.disabled && !input.checked && input;
    }, 'desktop lyrics setting ready');
    show.scrollIntoView({ block: 'center', behavior: 'instant' });
    click(show);
    await wait(async () => (await invoke('desktop_lyric_settings', { epoch })).visible, 'desktop lyric window visible');
    const desktop = await wait(async () => fixture('eval', { source: `
      const state = await window.__TAURI_INTERNALS__.invoke('desktop_lyric_read', {
        generation: location.hash.split('=')[1],
      });
      const row = [...document.querySelectorAll('.desktop-lyric-row')].find(node =>
        node.textContent.includes(${JSON.stringify(expectedText)}));
      if (state.state !== 'ready' || !state.lyrics?.info[${JSON.stringify(sourceFlag)}] || !row) return false;
      const bounds = row.getBoundingClientRect();
      return {playId: state.lyrics.playId, fromEmbedded: state.lyrics.info.fromEmbedded,
        fromSidecar: state.lyrics.info.fromSidecar, networkFailed: state.lyrics.info.networkFailed,
        locked: state.locked, overflow: document.documentElement.scrollWidth > innerWidth,
        paintedRow: document.visibilityState === 'visible' && bounds.width > 0 && bounds.height > 0 &&
          bounds.top >= 0 && bounds.bottom <= innerHeight};
    ` }), 'desktop local text and provenance');
    assert(desktop.playId === playback.playId && desktop.paintedRow &&
      desktop.fromSidecar === nativeSidecar && desktop.fromEmbedded === !nativeSidecar &&
      !desktop.networkFailed, 'desktop shares the same native local lyric and displays its row');
    await capture(`ux_02_${caseName}_desktop`, 'lyric', desktop);
    const stats = await fixture('stats');
    assert(stats.requests === 0 && stats.pending === 0, 'no online lyric reads');
    await emit({ kind: 'complete', stage: 'complete', ok: true, stageCount: 2, realMusicRequests: 0,
      unindexed: opened?.unindexed ?? false });
  } catch (error) {
    await emit({ kind: 'failed', stage: 'failed', error: String(error?.message ?? error) });
  }
})();
