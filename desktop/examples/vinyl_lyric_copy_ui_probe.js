// Input comes from the independent X11 driver, never synthetic DOM events.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const line = (index) => one(`.lyric-scroll .lyric-line[data-line="${index}"]`);
  const text = (index) => one('[data-lyric-text]', line(index));
  const button = (label) => one(`.playerbar button[aria-label="${label}"]`);
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (condition, message) => { if (!condition) throw Error(message); };
  async function wait(read, message, ms = 18000) {
    const end = performance.now() + ms;
    while (performance.now() < end) {
      const value = await read();
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
  const lineIndex = (node) => {
    const element = node instanceof Element ? node : node?.parentElement;
    return element?.closest('.lyric-line')?.dataset.line ?? null;
  };
  function selection() {
    const value = getSelection();
    return { text: value?.toString() ?? '', start: lineIndex(value?.anchorNode),
      end: lineIndex(value?.focusNode), collapsed: value?.isCollapsed ?? true };
  }
  const events = [];
  let copying;
  for (const type of ['pointerdown', 'pointerup', 'click', 'keydown']) {
    document.addEventListener(type, (event) => {
      if (!event.isTrusted) return;
      events.push({ type, line: lineIndex(event.target), key: event.key ?? null,
        ctrl: event.ctrlKey, trusted: true });
    }, { capture: true });
  }
  document.addEventListener('copy', (event) => {
    const target = lineIndex(event.target), hasClipboardData = !!event.clipboardData;
    // Trusted browser dispatch can run microtasks between listeners on the
    // same document. Read defaultPrevented after the complete dispatch.
    setTimeout(() => { copying = { trusted: event.isTrusted, handled: event.defaultPrevented,
      target, hasClipboardData }; }, 0);
  });

  const geometry = {};
  function point(index, edge) {
    const owner = text(index);
    const walker = document.createTreeWalker(owner, NodeFilter.SHOW_TEXT);
    const nodes = [];
    for (let node = walker.nextNode(); node; node = walker.nextNode()) if (node.length) nodes.push(node);
    const target = edge === 'end' ? nodes.at(-1) : nodes[0];
    const indexInText = edge === 'end' ? target.textContent.trimEnd().length - 1 :
      target.length - target.textContent.trimStart().length;
    const range = document.createRange();
    range.setStart(target, indexInText);
    range.setEnd(target, indexInText + 1);
    const r = range.getBoundingClientRect();
    const row = line(index);
    const x = edge === 'row' ? row.getBoundingClientRect().left + 4 :
      r.left + r.width * (edge === 'end' ? .93 : .15);
    const y = r.top + r.height / 2;
    const hit = document.elementFromPoint(x, y);
    geometry[`${index}-${edge}`] = { x, y, width: r.width, height: r.height,
      text: target.textContent, hit: hit?.outerHTML.slice(0, 120) ?? null,
      scroll: one('.lyric-scroll')?.scrollTop };
    return r.width > 2 && r.height > 3 && (edge === 'row' ? row : owner).contains(hit) ?
      { x, y, ratio: devicePixelRatio, width: innerWidth, height: innerHeight } : undefined;
  }

  let epoch, baseline, count = 0, phase = 'startup';
  const actor = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  const controls = (player) => [player.playId, player.entryId, player.queueRevision];
  async function settled(seconds) {
    let previous;
    return wait(async () => {
      const snapshot = await actor();
      const value = snapshot.state === 'paused' && snapshot.track?.id === '1' && !snapshot.error &&
        !snapshot.nativeSeeking && Math.abs(snapshot.position - seconds) < .12 ? snapshot : undefined;
      const stable = value && previous === value.position;
      previous = value?.position;
      return stable && value;
    }, `native paused seek settled at ${seconds}s`);
  }
  async function capture(label, fields = {}) {
    phase = label;
    const stage = `ux_${String(++count).padStart(2, '0')}_${label}`;
    window.__queueUiStageRecorded = '';
    await emit(stage, { ...fields, events: [...events], selection: selection(), copy: copying ?? null });
    await wait(() => window.__queueUiStageRecorded === stage, 'native stage acknowledged');
  }
  async function unchanged(expected = baseline) {
    const state = await actor();
    assert(state.state === 'paused' && state.position === expected.position && !state.nativeSeeking &&
      controls(state).every((value, index) => value === controls(expected)[index]),
    'text input did not seek, resume, or replace native playback');
  }
  async function renderStable() {
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  }
  async function focusLine(index) {
    await wait(() => line(index)?.getAttribute('aria-disabled') === 'false', `line ${index} can seek`);
    line(index).focus({ preventScroll: true });
    assert(document.hasFocus() && document.activeElement === line(index), 'actual WebView line focus');
  }
  try {
    await wait(async () => {
      const account = await invoke('account_snapshot');
      if (account.status !== 'authenticated' || one('[aria-label="打开设置"]')?.disabled !== false) return false;
      epoch = account.epoch;
      return !!one('.daily-track-actions') && !!one('.track-row[data-track-id="1"]');
    }, 'authenticated synthetic Daily and navigation ready');
    phase = 'load_and_pause';
    click(await wait(() => one('.track-row[data-track-id="1"] .track-play:not(:disabled)'), 'song 1 play enabled'));
    await wait(async () => {
      const state = await actor();
      return state.state === 'playing' && state.track?.id === '1' && state.position > .2 && button('暂停');
    }, 'real native playback started');
    click(button('暂停'));
    await wait(async () => (await actor()).state === 'paused', 'native pause accepted');
    await invoke('player_action', { epoch, action: { kind: 'seek', seconds: 4.25 } });
    baseline = await settled(4.25);
    const initialQueue = await queue();
    assert(initialQueue.current === baseline.entryId && initialQueue.revision === baseline.queueRevision,
      'native baseline queue matches decoder');
    click(button('歌词'));
    await wait(() => one('.lyric-line.current[data-line="4"]') && text(5) &&
      one('.lyric-line[data-line="4"] small[data-lyric-text]')?.textContent.trim() === '合成翻译 4' &&
      document.hasFocus(), 'focused WebView displays the fixture lyric and translation');
    await renderStable();
    assert(text(4).textContent.trim() === '合成歌词 4' && text(5).textContent.trim() === '合成歌词 5',
      'the tested main fragments are exact');
    const targets = await wait(() => {
      const from = point(4, 'start'), to = point(5, 'end');
      return from && to && !one('.lyrics-panel-enter-active') && { from, to };
    }, 'settled lyric pane has both unobstructed glyph targets');
    await capture('drag_ready', { targets });

    await wait(() => events.some(({ type, line }) => type === 'pointerup' && line === '5') &&
      selection().start === '4' && selection().end === '5' && selection().text.includes('合成翻译 4'),
    'trusted drag selects the two fixture lines and intervening translation');
    assert(events.some(({ type, line }) => type === 'pointerdown' && line === '4'), 'actual starting pointer press');
    await unchanged();
    await capture('copy_ready');

    await wait(() => copying?.trusted && copying.handled && events.some(({ type, key, ctrl }) =>
      type === 'keydown' && key.toLowerCase() === 'c' && ctrl), 'trusted Ctrl+C used the scoped clipboard filter');
    await unchanged();
    const target = await wait(() => point(5, 'row'), 'ordinary seek row can be clicked');
    await capture('copied', { target });

    await settled(5);
    assert(events.some(({ type, line }) => type === 'click' && line === '5'), 'trusted plain lyric click');
    await focusLine(4);
    await capture('pointer_seek', { focused: '4' });

    await settled(4);
    assert(events.some(({ type, line, key }) => type === 'keydown' && line === '4' && key === 'Enter'),
      'trusted Return on a focused lyric');
    await focusLine(5);
    await capture('keyboard_seek', { focused: '5' });

    const afterSpace = await settled(5);
    assert(events.some(({ type, line, key }) => type === 'keydown' && line === '5' && key === ' '),
      'trusted Space on a focused lyric');
    await capture('space_seek');

    const sameRowTargets = await wait(() => {
      const from = point(4, 'start'), to = point(4, 'end');
      return from && to && { from, to };
    }, 'same-row drag glyphs are unobstructed');
    const beforeDragEvents = events.length;
    await capture('same_row_drag_ready', { targets: sameRowTargets });
    await wait(() => events.slice(beforeDragEvents).some(({ type, line }) => type === 'click' && line === '4') &&
      selection().start === '4' && selection().end === '4' && selection().text === '合成歌词 4',
    'same-row trusted drag generates a real click without collapsing selection');
    await unchanged(afterSpace);
    await capture('same_row_dragged');
    await emit('complete', { ok: true, stageCount: count, realMusicRequests: 0, accountWrites: 0 });
  } catch (error) {
    const native = epoch ? await actor().catch(() => null) : null;
    await emit('failed', { ok: false, phase, error: String(error), events, selection: selection(), geometry,
      copy: copying ?? null, active: document.activeElement?.outerHTML.slice(0, 350),
      position: native?.position, state: native?.state, track: native?.track?.id,
      page: one('.main-content')?.innerText.slice(0, 350) });
  }
})();
