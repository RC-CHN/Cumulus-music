// The independent X11 driver supplies the only selection and copy gestures.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const button = (label) => one(`button[aria-label="${label}"]`);
  const panel = () => one('.lyrics-panel');
  const body = () => one('[data-vinyl-comment-group="hot"] [data-vinyl-comment-id="101"] p[data-vinyl-copy-text]');
  const expected = '合成评论 101';
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (condition, message) => { if (!condition) throw Error(message); };
  async function wait(check, message) {
    const deadline = performance.now() + 18000;
    while (performance.now() < deadline) {
      const value = await check();
      if (value) return value;
      await sleep(50);
    }
    throw Error(message);
  }
  function click(node) {
    assert(node?.isConnected && !node.disabled && !node.closest('[inert]'), 'enabled setup control');
    node.focus({ preventScroll: true });
    node.click();
  }
  function selection() {
    const selected = getSelection(), container = body();
    return { text: selected?.toString() ?? '', collapsed: selected?.isCollapsed ?? true,
      bodyOnly: !!container && container.contains(selected?.anchorNode) && container.contains(selected?.focusNode) };
  }
  const input = [], geometry = {};
  let copied, phase = 'startup', epoch, baseline, count = 0;
  for (const type of ['pointerdown', 'pointerup', 'keydown']) {
    document.addEventListener(type, (event) => {
      if (event.isTrusted) input.push({ type, body: !!body()?.contains(event.target),
        key: event.key ?? null, ctrl: event.ctrlKey, trusted: true });
    }, { capture: true });
  }
  document.addEventListener('copy', (event) => {
    const hasClipboardData = !!event.clipboardData;
    // Read the final prevention state after every listener on the event path.
    setTimeout(() => { copied = { trusted: event.isTrusted, handled: event.defaultPrevented, hasClipboardData }; }, 0);
  });
  function point(edge) {
    const target = body();
    if (!target) return;
    const walker = document.createTreeWalker(target, NodeFilter.SHOW_TEXT);
    const nodes = [];
    for (let node = walker.nextNode(); node; node = walker.nextNode()) if (node.textContent.trim()) nodes.push(node);
    const glyph = edge === 'end' ? nodes.at(-1) : nodes[0];
    if (!glyph) return;
    const offset = edge === 'end' ? glyph.textContent.trimEnd().length - 1 :
      glyph.length - glyph.textContent.trimStart().length;
    const range = document.createRange();
    range.setStart(glyph, offset);
    range.setEnd(glyph, offset + 1);
    const r = range.getBoundingClientRect();
    const x = r.left + r.width * (edge === 'end' ? .93 : .15), y = r.top + r.height / 2;
    const hit = document.elementFromPoint(x, y);
    geometry[edge] = { x, y, width: r.width, height: r.height, hit: hit?.outerHTML.slice(0, 130) };
    return r.width > 2 && r.height > 3 && target.contains(hit) ?
      { x, y, ratio: devicePixelRatio, width: innerWidth, height: innerHeight } : undefined;
  }
  const actor = () => invoke('player_snapshot', { epoch });
  async function capture(label, fields = {}) {
    phase = label;
    const value = await actor();
    assert(value.state === 'paused' && !value.error && !value.nativeSeeking && value.track?.id === '1' &&
      ['playId', 'entryId', 'queueRevision', 'position'].every((key) => value[key] === baseline[key]),
    'comment view and copy do not affect the native decoder or clock');
    const stage = `ux_${String(++count).padStart(2, '0')}_${label}`;
    window.__queueUiStageRecorded = '';
    await emit(stage, { ...fields, selection: selection(), copy: copied ?? null, input: [...input],
      pane: panel()?.dataset.vinylTab, visible: document.visibilityState === 'visible' });
    await wait(() => window.__queueUiStageRecorded === stage, 'native stage acknowledged');
  }

  try {
    await wait(async () => {
      const account = await invoke('account_snapshot');
      if (account.status !== 'authenticated' || button('打开设置')?.disabled !== false) return false;
      epoch = account.epoch;
      return !!one('.daily-track-actions');
    }, 'synthetic daily and navigation ready');
    phase = 'pause_song_1';
    click(await wait(() => one('.track-row[data-track-id="1"] .track-play:not(:disabled)'), 'song 1 playable'));
    await wait(async () => {
      const value = await actor();
      return value.state === 'playing' && value.track?.id === '1' && value.position > .2 && button('暂停');
    }, 'native decoder advances');
    click(button('暂停'));
    await wait(async () => (await actor()).state === 'paused', 'native pause accepted');
    await invoke('player_action', { epoch, action: { kind: 'seek', seconds: 4.25 } });
    let previous;
    baseline = await wait(async () => {
      const value = await actor();
      const matches = value.state === 'paused' && !value.error && !value.nativeSeeking &&
        Math.abs(value.position - 4.25) < .12;
      const stable = matches && previous === value.position;
      previous = matches ? value.position : undefined;
      return stable && value;
    }, 'settled paused native clock');

    phase = 'open_t1_comments';
    await wait(() => one('.player-comment-count')?.textContent.trim() === '63', 'current fixture count');
    click(button('歌词'));
    await wait(() => panel()?.dataset.vinylTab === 'lyrics' && one('.vinyl-scroll-comment') &&
      !one('.lyrics-panel-enter-active') && button('当前歌曲评论')?.getAttribute('aria-pressed') === 'false',
    'authoritative t1 embedded comments available');
    click(button('当前歌曲评论'));
    await wait(() => panel()?.dataset.vinylTab === 'comments' && body()?.textContent.trim() === expected &&
      document.hasFocus(), 'fixture comment rendered in the native WebView');
    const targets = await wait(() => {
      const from = point('start'), to = point('end');
      return from && to && !one('.vinyl-tab-stack')?.getAnimations().some((a) => a.pending || a.playState === 'running') &&
        { from, to };
    }, 'settled comment body is an unobstructed pointer target');
    await capture('comment_drag_ready', { targets });
    await wait(() => input.some((e) => e.type === 'pointerup' && e.body) && selection().bodyOnly &&
      selection().text === expected, 'trusted pointer selected only the comment body');
    assert(input.some((e) => e.type === 'pointerdown' && e.body), 'native pointer started on comment');
    await capture('comment_copy_ready');
    await wait(() => copied?.trusted && copied.handled && copied.hasClipboardData &&
      input.some((e) => e.type === 'keydown' && e.key === 'c' && e.ctrl), 'trusted Ctrl+C was filtered');
    await capture('comment_copied');
    await emit('complete', { ok: true, stageCount: count, realMusicRequests: 0, accountWrites: 0 });
  } catch (error) {
    await emit('failed', { ok: false, phase, error: String(error), input, geometry, selection: selection(),
      copy: copied ?? null, pane: panel()?.dataset.vinylTab, active: document.activeElement?.outerHTML.slice(0, 260),
      comments: one('.vinyl-comments-scroll')?.innerText.slice(0, 400) });
  }
})();
