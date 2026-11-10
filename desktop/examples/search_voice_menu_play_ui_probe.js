// A menu Play on the active program must never issue the row's Pause.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, extra = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...extra } });
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const settled = () => !one('.page-enter-active,.page-leave-active,.search-results-enter-active,.search-results-leave-active');
  const assert = (value, label) => { if (!value) throw Error(label); };
  async function wait(read, label) {
    for (let n = 0; n < 200; n++) {
      const value = await read();
      if (value) return value;
      await sleep(45);
    }
    throw Error(label);
  }
  const row = () => one('.search-voice-table [data-result-key="voice:401"]');
  const more = () => row()?.querySelector('.menu-trigger');
  const index = () => row()?.querySelector('[data-result-action="play"]');
  const panel = () => {
    const id = more()?.getAttribute('aria-controls');
    const node = id && document.getElementById(id);
    return node && !node.closest('[inert]') && more()?.getAttribute('aria-expanded') === 'true' && node;
  };
  async function menuPlay() {
    const trigger = await wait(more, 'observed row More trigger');
    trigger.focus({ preventScroll: true });
    trigger.click();
    const play = await wait(() => panel()?.querySelector('[data-menu-key="play"]:not(:disabled)'),
      'prepared native menu Play');
    assert(play.textContent.trim() === '播放', 'More Play remains explicit while the voice is current');
    return play;
  }
  const pointer = (node) => {
    const rect = node.getBoundingClientRect();
    const x = rect.left + rect.width / 2, y = rect.top + rect.height / 2;
    assert(node.contains(document.elementFromPoint(x, y)), 'trusted pointer hits intended control');
    return { x: Math.round((screenX + x) * devicePixelRatio),
      y: Math.round((screenY + y) * devicePixelRatio) };
  };
  let count = 0, step = 'startup', epoch;
  const view = () => invoke('player_snapshot', { epoch });
  const at = (state) => async () => {
    const current = await view();
    return current.state === state && current.track?.voiceId === '401' &&
      one('.playerbar')?.dataset.state === state && current;
  };
  async function capture(label, extra = {}, menu, pointerTarget) {
    const samples = [];
    if (menu) {
      await wait(() => !menu.classList.contains('menu-enter-active') &&
        getComputedStyle(menu).opacity === '1', 'menu is ready to paint');
      const r = menu.getBoundingClientRect();
      samples.push({ x:r.left + 3, y:r.top + r.height / 2, color:getComputedStyle(menu).backgroundColor });
    }
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await emit(`ux_${String(++count).padStart(2, '0')}_${label}`,
      { ...extra, pointer:pointerTarget && pointer(pointerTarget), playback:await view(),
        screenX, screenY, ratio:devicePixelRatio, paintSamples:samples });
  }
  let trustedMenu = 0, trustedIndex = 0;
  document.addEventListener('click', (event) => {
    if (!event.isTrusted || !(event.target instanceof Element)) return;
    if (event.target.closest('[data-menu-key="play"]')) trustedMenu++;
    if (event.target.closest('[data-result-action="play"]') === index()) trustedIndex++;
  }, true);
  try {
    await wait(() => settled() && one('.track-row') && one('[aria-label="搜索音乐"]'), 'Daily and search header');
    ({ epoch } = await invoke('account_snapshot'));
    await invoke('player_action', { epoch, action:{ kind:'mode', mode:'sequential' } });
    window.__searchFixtureAck = false;
    await emit('search-fixture', { mode:'voice_next' });
    await wait(() => window.__searchFixtureAck, 'sealed search fixture');
    const input = one('[aria-label="搜索音乐"]');
    input.value = '合成';
    input.dispatchEvent(new Event('input', { bubbles:true }));
    input.form.dispatchEvent(new Event('submit', { bubbles:true, cancelable:true }));
    (await wait(() => settled() && one('#search-tab-voices'), 'Voices tab')).click();
    await wait(() => settled() && index() && more(), 'classified voice row');
    step = 'cold-menu';
    (await menuPlay()).click();
    const started = await wait(at('playing'), 'menu starts exact program');
    assert(started.queueLen === 1, 'menu loads only one program');
    const same = (state) => state.playId === started.playId &&
      state.entryId === started.entryId && state.queueRevision === started.queueRevision;
    step = 'playing-menu';
    const repeat = await menuPlay();
    await capture('playing_menu', { label:repeat.textContent.trim() }, panel(), repeat);
    await wait(async () => {
      const value = await at('playing')();
      return value && same(value) && !panel() && value.position > started.position + .15;
    }, 'same decoder stays playing after explicit Menu Play');
    await capture('playing_kept', { trustedMenu });
    step = 'row-toggle';
    const button = await wait(() => index()?.getAttribute('aria-label')?.startsWith('暂停') && index(),
      'row still exposes Pause');
    await capture('row_pause', {}, null, button);
    const paused = await wait(at('paused'), 'trusted row index still pauses');
    assert(same(paused), 'row does not change decoder');
    step = 'paused-menu';
    const resume = await menuPlay();
    await capture('paused_menu', { label:resume.textContent.trim() }, panel(), resume);
    await wait(async () => {
      const value = await at('playing')();
      return value && same(value) && value.position > paused.position + .15;
    }, 'Menu Play resumes the same clock');
    assert(trustedMenu === 2 && trustedIndex === 1, 'two real Menu Play clicks and one real row Pause');
    await capture('resumed', { paused, trustedMenu, trustedIndex });
    await emit('complete', { ok:true, stageCount:count });
  } catch (error) {
    await emit('failed', { step, error:String(error), stageCount:count });
  }
})();
