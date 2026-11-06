// Native X11 drags and keyboard copy are supplied by vinyl_text_copy_smoke.py.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const button = (label) => one(`button[aria-label="${label}"]`);
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, fields = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...fields },
  });
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (ok, message) => { if (!ok) throw Error(message); };
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
  const panel = () => one('.lyrics-panel');
  const dialog = () => one('.vinyl-mode-sheet[open]');
  const title = () => one('.vinyl-info h1[data-vinyl-copy-text]', panel());
  const cover = () => one('.vinyl-cover-line[data-vinyl-copy-text]', panel());
  const text = (node) => node?.textContent.trim() ?? '';
  const busy = () => one('.lyrics-panel-enter-active,.lyrics-panel-leave-active,.modal-enter-active,.modal-leave-active');
  const tag = (node) => (node instanceof Element ? node : node?.parentElement)
    ?.closest('[data-vinyl-copy-text]')?.tagName ?? null;
  const sourceTarget = (node) => node instanceof Element && !!node.closest('.vinyl-source');
  const currentSelection = () => {
    const selected = getSelection();
    return { text: selected?.toString() ?? '', start: tag(selected?.anchorNode),
      end: tag(selected?.focusNode), collapsed: selected?.isCollapsed ?? true };
  };
  const input = [];
  const geometry = {};
  let exclusion;
  let copied;
  for (const type of ['pointerdown', 'pointerup', 'keydown']) {
    document.addEventListener(type, (event) => {
      if (event.isTrusted) input.push({ type, tag: tag(event.target), key: event.key ?? null,
        ctrl: event.ctrlKey, trusted: true, source: sourceTarget(event.target) });
    }, { capture: true });
  }
  document.addEventListener('copy', (event) => {
    const hasClipboardData = !!event.clipboardData;
    setTimeout(() => { copied = { trusted: event.isTrusted, handled: event.defaultPrevented, hasClipboardData }; }, 0);
  });
  function point(node, edge) {
    if (!node) return;
    const walker = document.createTreeWalker(node, NodeFilter.SHOW_TEXT);
    const nodes = [];
    for (let n = walker.nextNode(); n; n = walker.nextNode()) if (n.textContent.trim()) nodes.push(n);
    const source = edge === 'end' ? nodes.at(-1) : nodes[0];
    if (!source) return;
    const offset = edge === 'end' ? source.textContent.trimEnd().length - 1 :
      source.length - source.textContent.trimStart().length;
    const range = document.createRange();
    range.setStart(source, offset);
    range.setEnd(source, offset + 1);
    const r = range.getBoundingClientRect();
    const x = r.left + r.width * (edge === 'end' ? .93 : .15), y = r.top + r.height / 2;
    const hit = document.elementFromPoint(x, y);
    geometry[edge] = { x, y, width: r.width, height: r.height, hit: hit?.outerHTML.slice(0, 150) };
    return r.width > 2 && r.height > 3 && node.contains(hit) ?
      { x, y, ratio: devicePixelRatio, width: innerWidth, height: innerHeight } : undefined;
  }
  let epoch, baseline, count = 0, phase = 'startup';
  const actor = () => invoke('player_snapshot', { epoch });
  async function capture(label, values = {}) {
    phase = label;
    const snapshot = await actor();
    assert(snapshot.state === 'paused' && !snapshot.error && !snapshot.nativeSeeking &&
      ['playId', 'entryId', 'queueRevision', 'position'].every((key) => snapshot[key] === baseline[key]),
    'text copy and mode changes preserve the paused native decoder and clock');
    const stage = `ux_${String(++count).padStart(2, '0')}_${label}`;
    window.__queueUiStageRecorded = '';
    await emit(stage, { ...values, mode: panel()?.dataset.vinylMode, selection: currentSelection(), copy: copied ?? null });
    await wait(() => window.__queueUiStageRecorded === stage, 'native stage acknowledged');
  }
  async function mode(value) {
    click(one('.vinyl-mode-entry'));
    await wait(() => dialog() && !busy(), 'mode sheet settled');
    click(one(`input[value="${value}"]`, dialog()));
    await wait(() => panel()?.dataset.vinylMode === value, 'mode selected');
    click(button('关闭播放器模式'));
    await wait(() => !dialog() && !busy() && document.hasFocus(), 'mode sheet closed');
  }
  async function copyText(label, getNode, expected) {
    phase = `${label}_prepare`;
    copied = undefined;
    const targets = await wait(() => {
      const node = getNode();
      if (!document.hasFocus() || busy() || text(node) !== expected) return;
      const from = point(node, 'start'), to = point(node, 'end');
      return from && to && { from, to };
    }, `visible selectable ${label}`);
    const before = input.length;
    await capture(`${label}_drag_ready`, { targets });
    const targetTag = getNode().tagName;
    await wait(() => {
      const selection = currentSelection();
      return selection.text.trim() === expected && selection.start === targetTag && selection.end === targetTag &&
        input.slice(before).some(({ type, tag }) => type === 'pointerdown' && tag === targetTag) &&
        input.slice(before).some(({ type, tag }) => type === 'pointerup' && tag === targetTag);
    }, `trusted drag selected exact ${label}`);
    await capture(`${label}_copy_ready`);
    await wait(() => copied?.trusted && copied.handled && copied.hasClipboardData &&
      input.slice(before).some(({ type, key, ctrl }) => type === 'keydown' && key === 'c' && ctrl),
    `trusted Ctrl+C reached the vinyl plain filter (${label})`);
    await capture(`${label}_copied`);
  }

  try {
    await wait(async () => {
      const account = await invoke('account_snapshot');
      if (account.status !== 'authenticated' || button('打开设置')?.disabled !== false) return false;
      epoch = account.epoch;
      return !!one('.daily-track-actions');
    }, 'synthetic Daily navigation ready');
    click(await wait(() => one('.track-row[data-track-id="2"] .track-play:not(:disabled)'), 'song 2 playable'));
    await wait(async () => {
      const snapshot = await actor();
      return snapshot.state === 'playing' && snapshot.track?.id === '2' && snapshot.position > .2 && button('暂停');
    }, 'native audio played');
    click(button('暂停'));
    await wait(async () => (await actor()).state === 'paused', 'native audio paused');
    await invoke('player_action', { epoch, action: { kind: 'seek', seconds: 4.25 } });
    let previous;
    baseline = await wait(async () => {
      const snapshot = await actor();
      const value = snapshot.state === 'paused' && !snapshot.nativeSeeking && !snapshot.error &&
        Math.abs(snapshot.position - 4.25) < .12 ? snapshot : undefined;
      const stable = value && value.position === previous;
      previous = value?.position;
      return stable && value;
    }, 'native clock settled after initial seek');
    click(button('歌词'));
    await wait(() => title() && panel().dataset.vinylMode === 'classic' && !busy(), 'classic details displayed');
    const selectStyle = (node) => {
      if (!node) return null;
      const css = getComputedStyle(node);
      return css.getPropertyValue('user-select') || css.getPropertyValue('-webkit-user-select');
    };
    const selectors = ['.vinyl-credits', '.vinyl-album', '.vinyl-source'];
    const metadata = selectors.map((selector) => one(selector, panel()));
    exclusion = { panel: selectStyle(panel()), rows: metadata.map((node, index) => ({
      selector: selectors[index], present: !!node, excluded: !!node?.closest('[data-non-selectable]'),
      allowed: !!node?.closest('[data-vinyl-copy-text]'), userSelect: selectStyle(node),
    })) };
    assert(exclusion.panel === 'none' && exclusion.rows.every((row) =>
      row.present && row.excluded && !row.allowed && row.userSelect !== 'text'),
    'classic artist, album and source rows are excluded');
    const source = metadata[2];
    assert(!one('a,button', source), 'sealed source label is non-interactive');
    const sourcePoints = await wait(() => {
      const from = point(source, 'start'), to = point(source, 'end');
      return from && to && { from, to };
    }, 'excluded source label is visible');
    await capture('classic_source_drag_ready', { targets: sourcePoints });
    await wait(() => input.some((item) => item.type === 'pointerup' && item.source),
      'trusted drag ended on the excluded source');
    assert(input.some((item) => item.type === 'pointerdown' && item.source) && currentSelection().collapsed,
      'native drag cannot select the classic source');
    await capture('classic_source_excluded');

    const song = '合成歌曲 2 · A quiet afternoon';
    await copyText('classic_title', title, song);
    await mode('cover');
    await copyText('cover_preview', cover, '合成歌词 4');
    await mode('lyric');
    await copyText('lyric_title', title, song);
    await emit('complete', { ok: true, stageCount: count, excludedClassicMetadata: true,
      realMusicRequests: 0, accountWrites: 0 });
  } catch (error) {
    await emit('failed', { ok: false, phase, error: String(error), count, geometry, exclusion,
      selection: currentSelection(), copy: copied ?? null, mode: panel()?.dataset.vinylMode ?? null,
      focus: document.hasFocus(), input, active: document.activeElement?.outerHTML.slice(0, 250) });
  }
})();
