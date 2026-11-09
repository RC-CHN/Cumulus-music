// Main interface preference through the real Tauri WebView and native device store.
(async () => {
  if (window.__interfaceFontProofStarted) return;
  window.__interfaceFontProofStarted = true;
  const phase = window.__interfaceFontProofPhase;
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = payload => invoke('plugin:event|emit', { event: 'ux-proof', payload });
  const one = selector => document.querySelector(selector);
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const check = (ok, label) => { if (!ok) throw Error(label); };
  const equal = (left, right) => JSON.stringify(left) === JSON.stringify(right);
  async function wait(test, label) {
    for (let i = 0; i < 300; i++) {
      const result = await test();
      if (result) return result;
      await sleep(50);
    }
    throw Error(label);
  }
  const settingsButton = '[aria-label="打开设置"]';
  const control = () => one('[aria-labelledby="settings-general"] select[data-interface-font]');
  const row = () => one('.track-row:not([inert] *)');
  const usable = node => node && !node.disabled && !node.closest('[inert]') &&
    node.getClientRects().length > 0;
  const options = select => [...select.options].map(option => ({
    value: option.value, label: option.textContent.trim(), disabled: option.disabled,
  }));
  const choices = ['', '仿宋', '华文隶书', '合成黑体', '楷体'];
  const expectedOptions = (missing = false) =>
    (missing ? ['', '楷体', ...choices.slice(1, -1)] :
      phase === 'reopen' ? choices.slice(0, -1) : choices)
      .map(value => ({ value, label: value || '默认', disabled: missing && value === '楷体' }));
  const css = () => {
    const style = getComputedStyle(document.body);
    const firstFamily = style.fontFamily.split(',')[0].trim().replace(/^(['"])(.*)\1$/, '$2');
    return { family: style.fontFamily, firstFamily, size: style.fontSize };
  };
  const matches = (family, size) => {
    const style = css();
    return style.firstFamily === (family || 'Microsoft YaHei') &&
      style.size === size + 'px' && style.family.includes('sans-serif');
  };
  let epoch, paused, lyricBaseline, stageCount = 0, last = 'startup';
  const preference = () => invoke('interface_font');
  const player = () => invoke('player_snapshot', { epoch });
  const action = action => invoke('player_action', { epoch, action });
  const lyric = async () => {
    const state = await invoke('desktop_lyric_settings', { epoch });
    return { visible: state.visible, preferences: state.preferences.current };
  };
  const playback = state => ({
    state: state.state, playId: state.playId, entryId: state.entryId,
    trackId: state.track?.id, position: state.position, queueRevision: state.queueRevision,
    queueLen: state.queueLen, nativeSeeking: state.nativeSeeking,
  });
  async function loadSettings(family, missing = false) {
    (await wait(() => {
      const button = one(settingsButton);
      return usable(button) && button;
    }, 'Settings button ready')).click();
    await wait(() => {
      const select = control();
      return usable(select) && select.getAttribute('aria-busy') === 'false' &&
        select.value === (family || '') && equal(options(select), expectedOptions(missing));
    }, 'filtered interface font options in General');
  }
  async function choose(family, size) {
    last = 'choose ' + (family || '默认');
    const select = control();
    check(usable(select) && select.getAttribute('aria-busy') === 'false' &&
      [...select.options].some(option => option.value === (family || '') && !option.disabled),
    'ready enabled choice: ' + last);
    select.focus({ preventScroll: true });
    select.value = family || '';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    await wait(async () => {
      const saved = await preference();
      return select.value === (family || '') && select.getAttribute('aria-busy') === 'false' &&
        saved.family === family && saved.sizePx === size && matches(family, size);
    }, 'native preference saved and body repainted: ' + last);
  }
  async function capture(name, family, size, inSettings, missing = false) {
    last = name;
    await wait(() => {
      const page = one('.page-view:not([inert])');
      return page && getComputedStyle(page).opacity === '1' &&
        !page.getAnimations().some(animation => animation.pending || animation.playState === 'running');
    }, 'settled visible page: ' + name);
    const select = control();
    check(document.documentElement.dataset.surface === 'main' &&
      document.visibilityState === 'visible', 'main WebView visible');
    if (inSettings) {
      check(usable(select) && select.value === (family || '') &&
        equal(options(select), expectedOptions(missing)), 'General selection: ' + name);
      select.scrollIntoView({ block: 'center', behavior: 'instant' });
    } else {
      check(!select && row(), 'font reaches the main catalog beyond Settings');
    }
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    if (inSettings) {
      const rect = select.getBoundingClientRect();
      check(rect.width > 0 && rect.height > 0 &&
        select.contains(document.elementFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2)),
      'interface font selector is visible and hit-testable');
    }
    const saved = await preference();
    check(saved.family === family && saved.sizePx === size && matches(family, size),
      'native preference and computed body font: ' + name);
    const state = playback(await player());
    check(equal(paused, state), 'font change disturbed paused native playback: ' + name);
    check(equal(lyricBaseline, await lyric()), 'main font changed desktop lyric preferences');
    check(!one('.interface-font-setting [role="alert"]'), 'unexpected font error: ' + name);
    await emit({
      kind: 'snapshot', stage: `ux_${String(++stageCount).padStart(2, '0')}_${name}`,
      surface: 'main', phase, inSettings, css: css(), preference: saved,
      selection: select?.value ?? null, options: select ? options(select) : null,
      playback: state, lyricPreferencesUnchanged: true,
    });
  }
  try {
    check(['first', 'reopen', 'reset'].includes(phase), 'known interface font phase');
    epoch = await wait(async () => {
      const account = await invoke('account_snapshot');
      return account.status === 'authenticated' && row() && account.epoch;
    }, 'synthetic account and real main WebView');
    await invoke('play_selection', {
      epoch, selection: { kind: 'songs', ids: ['1', '2'], index: 0, quality: 'standard' },
    });
    await wait(async () => {
      const state = await player();
      return state.state === 'playing' && state.track?.id === '1' && state.position > .08;
    }, 'synthetic PCM clock advanced in real GUI');
    await action({ kind: 'pause', paused: true });
    await action({ kind: 'seek', seconds: 4 });
    paused = await wait(async () => {
      const state = playback(await player());
      return state.state === 'paused' && state.playId && state.entryId && state.trackId === '1' &&
        state.queueLen === 2 && !state.nativeSeeking && Math.abs(state.position - 4) < .1 && state;
    }, 'paused PCM seek and two retained queue entries');
    lyricBaseline = await lyric();
    const selected = phase === 'reopen' ? '楷体' : null;
    const size = selected ? 14 : 12;
    await wait(() => matches(selected, size), 'saved CSS applied outside Settings on startup');
    await capture('main', selected, size, false);
    await loadSettings(selected, phase === 'reopen');
    if (phase === 'first') {
      await capture('default', null, 12, true);
      for (const [name, family, size] of [
        ['fangsong', '仿宋', 13], ['lishu', '华文隶书', 15],
        ['localized', '合成黑体', 12], ['kaishu', '楷体', 14],
      ]) {
        await choose(family, size);
        await capture(name, family, size, true);
      }
    } else if (phase === 'reopen') {
      await capture('missing_kaishu', '楷体', 14, true, true);
      await choose(null, 12);
      await capture('reset_default', null, 12, true);
    } else {
      await capture('reopened_default', null, 12, true);
    }
    await emit({ kind: 'complete', stage: 'complete', phase, ok: true, stageCount });
  } catch (error) {
    await emit({ kind: 'failed', stage: 'failed', phase, last, error: String(error?.message || error) });
  }
})();
