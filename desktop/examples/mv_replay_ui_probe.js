// Real local six-second fixture, decoder and canvas; no remote media or account.
(async () => {
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, data = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...data } });
  const one = (selector) => document.querySelector(selector);
  const button = (label) => [...document.querySelectorAll('button')].find(
    (node) => node.textContent.trim() === label,
  );
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  async function wait(test, label, timeout = 16000) {
    const deadline = Date.now() + timeout;
    while (Date.now() < deadline) {
      const result = await test();
      if (result) return result;
      await sleep(80);
    }
    throw Error(label);
  }
  try {
    await wait(() => one('.track-row'), 'initial page');
    window.__downloadPageAck = null;
    await emit('download-page-fixture', { mode: 'seed' });
    await wait(() => window.__downloadPageAck?.ok, 'synthetic downloaded file', 60000);
    button('下载管理').click();
    (await wait(() => one('#download-tab-mv'), 'MV downloads tab')).click();
    const card = await wait(
      () => one('button[aria-label^="播放下载MV 合成MV 133"]'), 'downloaded MV',
    );
    card.click();
    const { epoch } = await invoke('account_snapshot');
    const snapshot = () => invoke('video_snapshot', { epoch });
    const started = await wait(async () => {
      const v = await snapshot();
      return v.id === '133' && v.local && v.state === 'playing' && v;
    }, 'actual native local video');
    await wait(() => Number(one('.video-canvas')?.dataset.renderedFrames) > 1, 'rendered video');
    if (one('.mv-replay')) throw Error('replay appeared before EOF');
    const ended = await wait(async () => {
      const v = await snapshot();
      return v.playId === started.playId && v.state === 'ended' && v;
    }, 'real decoder EOF');
    const replay = await wait(() => one('.mv-replay:not(:disabled)'), 'centered replay');
    if (Math.abs(ended.position - ended.duration) > .1 ||
        !one('.video-controls [aria-label="播放MV"]')) throw Error('EOF transport presentation');
    replay.focus({ preventScroll: true });
    if (document.activeElement !== replay) throw Error('replay is not keyboard-focusable');
    await sleep(400); // Let the native WebView paint the EOF controls before capture.
    await emit('ux_01_mv_ended', { position: ended.position, duration: ended.duration });
    await sleep(400);
    replay.click();
    const restarted = await wait(async () => {
      const v = await snapshot();
      return v.state === 'playing' && v.id === started.id && v.local &&
        v.playId !== ended.playId && v.position > 0 && v;
    }, 'same MV starts a new native playback');
    if (restarted.position >= 3 || one('.mv-replay')) throw Error('replay did not restart at beginning');
    await wait(() => document.activeElement === one('.video-controls [aria-label="暂停MV"]'),
      'replay hands focus back to transport');
    await wait(() => Number(one('.video-canvas')?.dataset.renderedFrames) > 1, 'replayed frames');
    await sleep(300);
    await emit('ux_02_mv_restarted', { position: restarted.position, duration: restarted.duration });
    await sleep(400);
    await emit('complete', { ok: true, nativeMvReplay: true, local: true, frames: true });
  } catch (error) {
    await emit('failed', { error: String(error?.message ?? error) });
  }
})();
