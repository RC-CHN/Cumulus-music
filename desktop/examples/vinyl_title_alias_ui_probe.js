// Sealed Daily rows and real native playback. The Python driver provides the
// trusted X11 pointer/keyboard events and checks the GTK clipboard.
(async () => {
  const one = (query, root = document) => root?.querySelector(query);
  const button = (label) => one(`button[aria-label="${label}"]`);
  const panel = () => one('.lyrics-panel');
  const info = () => one('.vinyl-info', panel());
  const title = () => one('h1', info());
  const alias = () => one('.vinyl-track-alias[data-vinyl-copy-text]', info());
  const dialog = () => one('.vinyl-mode-sheet[open]');
  const currentMode = () => panel()?.dataset.vinylMode;
  const text = (node) => node?.textContent.trim() ?? null;
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, fields = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...fields } });
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (condition, label) => { if (!condition) throw Error(label); };
  async function wait(check, label) {
    const deadline = performance.now() + 16000;
    while (performance.now() < deadline) {
      const value = await check();
      if (value) return value;
      await sleep(50);
    }
    throw Error(label);
  }
  function click(node) {
    assert(node?.isConnected && !node.disabled && !node.closest('[inert]'), 'enabled fixture control');
    node.focus({ preventScroll: true });
    node.click();
  }
  const transitioning = () => one(
    '.lyrics-panel-enter-active,.lyrics-panel-leave-active,.modal-enter-active,.modal-leave-active'
  );

  let epoch, baseline, count = 0, phase = 'startup', copied;
  const actor = () => invoke('player_snapshot', { epoch });
  const trusted = [];
  const aliasEndpoint = (node) => (node?.nodeType === Node.TEXT_NODE ? node.parentElement : node)
    ?.closest?.('.vinyl-track-alias');
  const selection = () => {
    const current = getSelection();
    return {
      text: current?.toString() ?? '',
      onAlias: Boolean(alias() && aliasEndpoint(current?.anchorNode) === alias() &&
        aliasEndpoint(current?.focusNode) === alias()),
    };
  };
  for (const type of ['pointerdown', 'pointerup', 'keydown']) {
    document.addEventListener(type, (event) => {
      if (!event.isTrusted) return;
      const relevant = type === 'keydown' ? event.ctrlKey && event.key === 'c' : aliasEndpoint(event.target);
      if (relevant) trusted.push(type);
    }, { capture: true });
  }
  document.addEventListener('copy', (event) => {
    const hasClipboardData = Boolean(event.clipboardData);
    setTimeout(() => {
      copied = { trusted: event.isTrusted, handled: event.defaultPrevented, hasClipboardData };
    }, 0);
  });

  function pointerTarget(node, end) {
    const textNode = [...node.childNodes].find((child) =>
      child.nodeType === Node.TEXT_NODE && child.textContent.trim());
    if (!textNode) return undefined;
    const content = textNode.textContent;
    const offset = end ? content.trimEnd().length - 1 : content.length - content.trimStart().length;
    const range = document.createRange();
    range.setStart(textNode, offset);
    range.setEnd(textNode, offset + 1);
    const r = range.getBoundingClientRect();
    const x = r.left + r.width * (end ? .93 : .15);
    const y = r.top + r.height / 2;
    return r.width > 2 && r.height > 3 && node.contains(document.elementFromPoint(x, y))
      ? { x, y, ratio: devicePixelRatio, width: innerWidth, height: innerHeight } : undefined;
  }
  async function chooseSong(id, expectedAlias) {
    phase = `load_${id}`;
    if (panel()) {
      click(button('收起播放详情'));
      await wait(() => !panel() && !transitioning(), 'vinyl panel closed');
    }
    const track = await wait(() => {
      const row = one(`.track-row[data-track-id="${id}"] .track-play:not(:disabled)`);
      return row?.getClientRects().length && !row.closest('[inert]') && row;
    }, `Daily song ${id} ready`);
    click(track);
    await wait(async () => {
      const now = await actor();
      return now.state === 'playing' && now.track?.id === id && now.position > .2 &&
        button('暂停')?.disabled === false;
    }, `actual native audio for ${id}`);
    click(button('暂停'));
    await wait(async () => (await actor()).state === 'paused', 'native pause acknowledged');
    await invoke('player_action', { epoch, action: { kind: 'seek', seconds: 4.25 } });
    let previous;
    baseline = await wait(async () => {
      const now = await actor();
      const value = now.state === 'paused' && !now.nativeSeeking && !now.error &&
        now.track?.id === id && Math.abs(now.position - 4.25) < .12 ? now : undefined;
      const stable = value && value.position === previous;
      previous = value?.position;
      return stable && value;
    }, 'native clock settled after paused seek');
    assert((baseline.titleAlias ?? null) === expectedAlias, `native title alias for ${id}`);
    click(button('歌词'));
    await wait(() => title() && currentMode() === 'classic' && !transitioning(), 'classic vinyl mounted');
  }
  async function mode(value) {
    click(one('.vinyl-mode-entry'));
    await wait(() => dialog() && !transitioning(), 'mode sheet settled');
    click(one(`input[value="${value}"]`, dialog()));
    await wait(() => currentMode() === value, `selected ${value} mode`);
    click(button('关闭播放器模式'));
    await wait(() => !dialog() && !transitioning() && document.hasFocus(), 'mode sheet closed');
  }
  function inspect(expectedAlias) {
    const heading = title();
    assert(heading && text(heading) === baseline.track.name, 'visible title is the current native song');
    const css = getComputedStyle(heading);
    const clamp = css.getPropertyValue('-webkit-line-clamp');
    const marked = info().classList.contains('has-track-alias');
    const visibleAlias = alias();
    assert(marked === (expectedAlias !== null) && text(visibleAlias) === expectedAlias,
      'alias is present only for the expected classic song');
    if (visibleAlias) {
      const style = getComputedStyle(visibleAlias);
      assert(visibleAlias.getClientRects().length && !visibleAlias.closest('[data-non-selectable]') &&
        (style.getPropertyValue('user-select') || style.getPropertyValue('-webkit-user-select')) === 'text',
      'classic alias is visible and selectable');
    } else {
      assert(!one('.vinyl-track-alias', panel()), 'no hidden alias survives outside the classic lane');
    }
    assert(clamp === (expectedAlias === null ? '2' : '1'), 'only a visible alias clamps title to one line');
    return { title: text(heading), alias: text(visibleAlias), marked, clamp };
  }
  async function capture(label, expectedAlias, fields = {}) {
    phase = label;
    const now = await actor();
    assert(now.state === 'paused' && !now.error && !now.nativeSeeking &&
      ['playId', 'entryId', 'queueRevision', 'position'].every((key) => now[key] === baseline[key]),
    'mode and copy actions preserve actual paused play, queue and clock');
    const stage = `ux_${String(++count).padStart(2, '0')}_${label}`;
    window.__queueUiStageRecorded = '';
    await emit(stage, { ...fields, mode: currentMode(), layout: inspect(expectedAlias),
      selection: selection(), copy: copied ?? null });
    await wait(() => window.__queueUiStageRecorded === stage, 'native stage acknowledged');
  }

  try {
    await wait(async () => {
      const account = await invoke('account_snapshot');
      if (account.status !== 'authenticated' || button('打开设置')?.disabled !== false) return false;
      epoch = account.epoch;
      return Boolean(one('.daily-track-actions'));
    }, 'sealed Daily route ready');

    const precedence = '合成翻译；另一译名';
    await chooseSong('2', precedence);
    const targets = await wait(() => {
      if (text(alias()) !== precedence || !document.hasFocus()) return;
      const from = pointerTarget(alias(), false), to = pointerTarget(alias(), true);
      return from && to && { from, to };
    }, 'alias glyphs hittable for trusted drag');
    await capture('precedence_drag_ready', precedence, { targets });
    await wait(() => selection().text.trim() === precedence && selection().onAlias &&
      trusted.includes('pointerdown') && trusted.includes('pointerup'), 'trusted drag selects only the alias');
    await capture('precedence_copy_ready', precedence);
    await wait(() => copied?.trusted && copied.handled && copied.hasClipboardData &&
      trusted.includes('keydown'), 'trusted Ctrl+C uses vinyl plain text filter');
    await capture('precedence_copied', precedence);

    await mode('cover');
    await capture('precedence_cover', null);
    await mode('lyric');
    await capture('precedence_lyric', null);
    await mode('classic');
    await capture('precedence_classic_return', precedence);

    await chooseSong('3', '备用曲名；录音室版');
    await capture('fallback_classic', '备用曲名；录音室版');
    await chooseSong('4', null);
    await capture('empty_tns_classic', null);
    await emit('complete', { ok: true, stageCount: count, realMusicRequests: 0, accountWrites: 0 });
  } catch (error) {
    await emit('failed', { ok: false, phase, error: String(error), stageCount: count,
      mode: currentMode(), title: text(title()), alias: text(alias()), trusted, copy: copied ?? null,
      selection: selection(), focused: document.hasFocus() });
  }
})();
