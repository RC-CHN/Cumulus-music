// Sealed native episode records, the real program detail and null-output libmpv.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector) ?? null;
  const panel = () => one('.lyrics-panel:not(.lyrics-panel-leave-active)');
  const card = () => one('.voice-description-card', panel());
  const body = () => one('[data-voice-description-text]', card());
  const toggle = () => one('[data-voice-description-toggle]', card());
  const button = (label, root = document) => one(`button[aria-label="${label}"]`, root);
  const visible = (node) => node?.getClientRects().length && !node.disabled && !node.closest('[inert]');
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const frame = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const assert = (ok, label) => { if (!ok) throw Error(label); };
  const config = window.__voiceDescriptionProof;
  const lines = ['第一行合成节目简介', '第二行说明不改变播放', '第三行保留换行',
    '第四行仍然可见', '第五行仅展开后可见', '第六行结尾标记'];
  const short = '这是合成短简介';
  const long = lines.join('\n');
  const four = lines.slice(0, 4).join('\n');
  const expected = { collapsed: four + '…', expanded: long, short };
  let epoch, last = 'startup', count = 0, baseline, copyEvent;
  const input = [];
  const state = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  async function wait(test, label) {
    last = label;
    const end = performance.now() + 18000;
    while (performance.now() < end) {
      const value = await test();
      if (value) return value;
      await sleep(55);
    }
    throw Error(label);
  }
  async function click(get, label) {
    const node = await wait(() => { const n = get(); return visible(n) && n; }, label);
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    node.click();
  }
  function layout() {
    const node = body();
    if (!node) return null;
    const lineHeight = parseFloat(getComputedStyle(node).lineHeight);
    const height = node.getBoundingClientRect().height;
    const link = toggle();
    return { text: node.textContent, lineHeight, height,
      heading: one('h2', card())?.textContent,
      selectable: node.hasAttribute('data-vinyl-copy-text'),
      toggle: link ? { name: link.textContent.trim(), expanded: link.getAttribute('aria-expanded'),
        ownsBody: link.getAttribute('aria-controls') === node.id,
        focused: document.activeElement === link } : null };
  }
  function matches(kind) {
    const value = layout();
    if (kind === 'missing') return !card() && !body() && !toggle();
    const rows = { collapsed: 4, expanded: 6, short: 1 }[kind];
    return visible(body()) && value?.selectable && value.heading === '简介' &&
      value.text === expected[kind] && value.lineHeight === 26 &&
      Math.abs(value.height - 26 * rows) < .5 &&
      (kind === 'short' ? !value.toggle : value.toggle?.ownsBody &&
        value.toggle.name === (kind === 'expanded' ? '收起' : '展开') &&
        value.toggle.expanded === String(kind === 'expanded'));
  }
  function selection() {
    const selected = getSelection();
    return { text: selected?.toString() ?? '', collapsed: selected?.isCollapsed ?? true,
      bodyOnly: !!body() && body().contains(selected?.anchorNode) && body().contains(selected?.focusNode) };
  }
  function target(index, trailing) {
    const node = body();
    const text = node?.firstChild;
    if (!text || text.nodeType !== Node.TEXT_NODE || text.length <= index) return;
    const range = document.createRange();
    range.setStart(text, index);
    range.setEnd(text, index + 1);
    const rect = range.getBoundingClientRect();
    const x = rect.left + rect.width * (trailing ? .93 : .15);
    const y = rect.top + rect.height / 2;
    if (rect.width > 2 && rect.height > 3 && node.contains(document.elementFromPoint(x, y))) {
      return { x, y, ratio: devicePixelRatio, width: innerWidth, height: innerHeight };
    }
  }
  async function capture(label, kind, extra = {}) {
    last = label;
    if (window.__CUMULUS_PROOF_INVOKE__) {
      await wait(() => {
        const node = panel();
        return visible(node) && getComputedStyle(node).opacity === '1' &&
          !node.getAnimations().some((animation) => animation.pending || animation.playState === 'running');
      }, 'program detail compositor settles');
      await frame();
      await sleep(120);
    }
    assert(matches(kind), `exact visible ${kind} program description`);
    const player = await state();
    const rows = await queue();
    const stage = `ux_${String(++count).padStart(2, '0')}_${label}`;
    window.__podcastProgressStage = '';
    await emit(stage, { ...extra, case: config.case, actual: document.visibilityState === 'visible',
      layout: layout(), title: one('h1', panel())?.textContent.trim() ?? null,
      state: { state: player.state, playId: player.playId, entryId: player.entryId,
        queueRevision: player.queueRevision, voice: player.track?.voiceId, position: player.position,
        seeking: player.nativeSeeking, error: player.error, voiceDetail: player.voiceDetail },
      queue: { revision: rows.revision, current: rows.current,
        entries: rows.tracks.map((entry) => [entry.entryId, entry.track.voiceId]) },
    });
    await wait(() => window.__podcastProgressStage === stage, `native records ${stage}`);
    return player;
  }
  async function copyCollapsed() {
    for (const type of ['pointerdown', 'pointerup', 'keydown']) {
      document.addEventListener(type, (event) => {
        if (event.isTrusted) input.push({ type, body: !!body()?.contains(event.target),
          key: event.key ?? null, ctrl: event.ctrlKey });
      }, { capture: true });
    }
    document.addEventListener('copy', (event) => {
      const hasClipboardData = !!event.clipboardData;
      setTimeout(() => { copyEvent = { trusted: event.isTrusted, handled: event.defaultPrevented, hasClipboardData }; }, 0);
    });
    body().scrollIntoView({ block: 'center', behavior: 'instant' });
    await frame();
    const targets = await wait(() => {
      const from = target(0, false);
      const to = target(four.length - 1, true);
      return from && to && to.y - from.y > 70 && { from, to };
    }, 'first and fourth program description lines unobstructed');
    await capture('copy_drag_ready', 'collapsed', { targets });
    await wait(() => {
      const selected = selection();
      return selected.bodyOnly && !selected.collapsed && selected.text === four &&
        ['pointerdown', 'pointerup'].every((type) => input.some((v) => v.type === type && v.body));
    }, 'trusted drag selects the four visible lines without the ellipsis');
    await capture('copy_ready', 'collapsed', { selection: selection(), input: [...input] });
    await wait(() => copyEvent?.trusted && copyEvent.handled && copyEvent.hasClipboardData &&
      input.some((v) => v.type === 'keydown' && v.key === 'c' && v.ctrl), 'trusted Ctrl+C was handled');
    await capture('copied', 'collapsed', { selection: selection(), copy: copyEvent, input: [...input] });
    getSelection()?.removeAllRanges();
  }
  try {
    assert(['short', 'missing'].includes(config.case), 'closed description fixture');
    await wait(async () => {
      const account = await invoke('account_snapshot');
      if (account.status !== 'authenticated' || !visible(button('打开设置'))) return false;
      epoch = account.epoch;
      return visible(one('input[aria-label="搜索音乐"]'));
    }, 'authenticated synthetic shell is ready');
    await click(() => one('input[aria-label="搜索音乐"]'), 'open search discovery');
    await click(() => button('播客榜：更多', one('#search-discovery-panel:not(.menu-leave-active)')),
      'open classic podcast page');
    await wait(() => one('.podcast-discovery .podcast-top[data-layout="classic"]') &&
      one('[data-podcast-scene-fm] [data-scene-fm-title]'), 'classic Scene FM fixture is rendered');
    await click(() => button('播放随心听FM', one('[data-podcast-scene-fm]')), 'play the native first program');
    const loaded = await wait(async () => {
      const value = await state();
      return value.state === 'playing' && value.track?.voiceId === '601' && value.position > .2 && value;
    }, 'native first program decoder advances');
    await click(() => one('[data-podcast-scene-fm] [data-scene-fm-title]'), 'open playing program details');
    await wait(() => matches('collapsed'), 'six-line program measures to exactly four visible lines');
    const playing = await capture('playing', 'collapsed');
    assert(playing.state === 'playing' && playing.playId === loaded.playId, 'opening preserves the decoder');
    await click(() => button('暂停'), 'pause without changing program');
    let previous;
    baseline = await wait(async () => {
      const value = await state();
      if (value.state !== 'paused' || value.playId !== loaded.playId || value.nativeSeeking ||
          value.error || !Number.isFinite(value.position)) return false;
      const settled = previous === value.position;
      previous = value.position;
      return settled && value;
    }, 'native paused program clock settles');
    await capture('collapsed', 'collapsed');
    await click(toggle, 'expand the description');
    await wait(() => matches('expanded') && document.activeElement === toggle(),
      'all six lines visible with focus on collapse');
    await capture('expanded', 'expanded');
    await click(toggle, 'collapse the description');
    await wait(() => matches('collapsed') && document.activeElement === toggle(),
      'back to four lines with focus on expand');
    await capture('recollapsed', 'collapsed');
    if (config.clipboard) await copyCollapsed();
    await click(() => button('播放'), 'resume the same decoder');
    await wait(async () => {
      const value = await state();
      return value.state === 'playing' && value.playId === baseline.playId && value.position > baseline.position;
    }, 'same decoder advances beyond the paused clock');
    await capture('resumed', 'collapsed');
    await click(() => button('下一首'), 'advance to the next real program');
    await wait(async () => {
      const value = await state();
      return value.state === 'playing' && value.track?.voiceId === '602' && value.playId !== baseline.playId &&
        value.position > .2 && visible(panel()) && one('h1', panel())?.textContent.includes('602') &&
        matches(config.case);
    }, 'next program replaces old description with its own text or no card');
    await capture('next_voice', config.case);
    await emit('complete', { ok: true, case: config.case, stageCount: count,
      realMusicRequests: 0, realAccountWrites: 0, physicalAudioVerified: false });
  } catch (error) {
    let native = null;
    if (epoch) {
      const value = await state().catch(() => null);
      if (value) native = { state: value.state, voice: value.track?.voiceId, error: value.error,
        playId: value.playId, position: value.position };
    }
    await emit('failed', { ok: false, last, error: String(error), stageCount: count,
      layout: layout(), native, input, selection: selection(), copy: copyEvent ?? null,
      page: one('.main-content')?.innerText.slice(0, 1500) });
  }
})();
