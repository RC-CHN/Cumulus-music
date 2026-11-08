// True sealed PC assignment, native quality reads, and a real paused decoder.
(async () => {
  const invoke = (command, args) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, extra = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...extra } });
  const one = (selector) => document.querySelector(selector);
  const menu = () => one('.quality-action-menu:not([inert])');
  const trigger = () => one('.quality-control button[aria-label="音质"]');
  const row = (key) => menu()?.querySelector(`[data-menu-key="${key}"]`);
  const keys = () => [...(menu()?.querySelectorAll('[data-menu-key]') ?? [])].map((node) => node.dataset.menuKey);
  const ready = (node) => node?.isConnected && !node.disabled && node;
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (value, message) => { if (!value) throw Error(message); };
  let epoch, baseline, last, step = 'startup', stageCount = 0;
  const mode = window.__cumulusQualityCardsCase;
  const snapshot = async () => (last = await invoke('player_snapshot', { epoch }));
  const focusKey = () => document.activeElement?.dataset.menuKey;
  function key(name) {
    document.activeElement.dispatchEvent(new KeyboardEvent('keydown', { key: name, bubbles: true, cancelable: true }));
  }
  async function wait(read, message) {
    for (let attempt = 0; attempt < 240; attempt++) {
      const value = await read();
      if (value) return value;
      await sleep(60);
    }
    throw Error(message);
  }
  function click(node) {
    assert(ready(node), 'connected enabled control required');
    node.focus({ preventScroll: true });
    node.click();
  }
  function rect(node) {
    const { left, top, right, bottom, width } = node.getBoundingClientRect();
    return { left, top, right, bottom, width };
  }
  function metadataReady() {
    return menu()?.querySelector('.quality-menu-note')?.getAttribute('aria-busy') === 'false' &&
      row('standard')?.getAttribute('aria-checked') === 'true';
  }
  async function openMenu() {
    click(await wait(() => ready(trigger()), 'quality trigger'));
    await wait(metadataReady, 'actual player quality metadata settled');
  }
  async function closeMenu() {
    key('Escape');
    await wait(() => !menu() && document.activeElement === trigger(), 'Escape restores quality trigger');
  }
  async function capture(label) {
    step = label;
    const state = await snapshot();
    assert(state.state === 'paused' && state.playId === baseline.playId &&
      state.entryId === baseline.entryId && state.queueRevision === baseline.queueRevision &&
      state.position === baseline.position && state.actualQuality === 'standard' &&
      state.qualityPreferences.revision === baseline.qualityPreferences.revision,
      'quality presentation leaves native play identity, clock and preferences unchanged');
    const stage = `ux_${String(++stageCount).padStart(2, '0')}_${label}`;
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await sleep(120);
    await emit(stage, {
      keys: keys(), focus: focusKey() ?? null, observedPosition: state.position,
      paired: !!menu()?.querySelector('.quality-card-grid'),
      geometry: Object.fromEntries(['sky','jymaster','vivid'].filter((id) => row(id)).map((id) => [id, rect(row(id))])),
      trialDialog: !!one('.quality-trial-dialog[open]'),
    });
    await wait(() => window.__qualityCardStageRecorded === stage, 'native sample ' + stage);
  }
  async function arrange() {
    step = 'verified-native-assignment';
    await emit('quality-cards-assignment');
    await wait(() => window.__qualityCardsAssigned === true, 'one signed PC native assignment applied');
  }
  try {
    assert(['layout_vivid','layout_only','control'].includes(mode), 'sealed mode');
    ({ epoch } = await wait(async () => {
      const account = await invoke('account_snapshot');
      return account.status === 'authenticated' && ready(one('.track-row .track-play')) &&
        !one('.page-enter-active,.page-leave-active') && account;
    }, 'authenticated fixture and ordinary daily row'));
    step = 'start-audio';
    click(one('.track-row .track-play'));
    await wait(async () => {
      const state = await snapshot();
      return state.state === 'playing' && state.playId && state.position > .25;
    }, 'native clock advances');
    click(await wait(() => ready(one('.playerbar [aria-label="暂停"]')), 'pause control'));
    let previous;
    baseline = await wait(async () => {
      const state = await snapshot();
      if (state.state !== 'paused' || state.nativeSeeking || !Number.isFinite(state.position)) return false;
      if (previous?.playId === state.playId && previous.position === state.position) return state;
      previous = state;
      return false;
    }, 'paused native identity stable');
    await emit('resize', { width: 760, height: 540 });
    await wait(() => innerWidth === 760 && innerHeight === 540, 'compact viewport');

    if (mode === 'layout_only') {
      await openMenu();
      assert(keys()[0] === 'vivid' && !menu().querySelector('[data-quality-card]'),
        'unobserved assignment remains legacy');
      await capture('unobserved_legacy');
      await closeMenu();
    }
    await arrange();
    await openMenu();
    if (mode === 'control') {
      assert(keys().slice(0,3).join() === 'vivid,sky,jymaster' &&
        !menu().querySelector('[data-quality-card]'), 'control remains vertical in original order');
      key('Home');
      key('ArrowRight');
      assert(focusKey() === 'vivid', 'legacy ArrowRight does not change quality focus');
      await capture('signed_control');
      await closeMenu();
    } else {
      assert(keys().slice(0,2).join() === 'sky,jymaster' &&
        menu().querySelectorAll('[data-quality-card]').length === 2 &&
        !!row('vivid') === (mode === 'layout_vivid'),
        'pair uses offered Sky/Master and Vivid requires both groups');
      key('Home');
      const left = rect(row('sky')), right = rect(row('jymaster'));
      assert(focusKey() === 'sky' && Math.abs(left.top-right.top) <= 1 && left.right < right.left,
        'Sky then Master render side by side');
      if (row('vivid')) assert(rect(row('vivid')).top >= right.bottom, 'Vivid is a normal row after the pair');
      await capture('paired');
      key('ArrowRight');
      assert(focusKey() === 'jymaster', 'Right moves to the second card');
      await capture('right_card_focused');
      key('ArrowLeft');
      assert(focusKey() === 'sky', 'Left returns to the first card');
      window.__qualityFixtureAck = null;
      await emit('quality-fixture', { kind: 'cards-master-without-sky' });
      await wait(() => window.__qualityFixtureAck, 'changed actual offer fixture acknowledged');
      menu().querySelector('.quality-menu-note button').click();
      await wait(() => metadataReady() && !row('sky') && row('jymaster') &&
        !menu().querySelector('[data-quality-card]'), 'single remaining Master renders as a vertical row');
      await wait(() => focusKey() === 'standard', 'removed focused card returns focus to actual quality');
      key('Home');
      assert(focusKey() === 'jymaster' && keys()[0] === 'jymaster' &&
        rect(row('jymaster')).width >= menu().clientWidth - 24,
        'the remaining Master occupies the full row');
      await capture('singleton');
      if (mode === 'layout_only') {
        click(row('jymaster'));
        const trial = await wait(() => one('.quality-trial-dialog[open]'), 'existing trial consent dialog');
        assert(trial.textContent.includes('试用超清母带') && trial.textContent.includes('确认启用试用'),
          'layout still hands off to the real trial confirmation path');
        await capture('trial_consent');
        click([...trial.querySelectorAll('button')].find((button) => button.textContent.trim() === '取消'));
        await wait(() => !one('.quality-trial-dialog') && document.activeElement === trigger(),
          'cancel does not start the trial and restores focus');
      } else await closeMenu();
    }
    await emit('complete', { ok: true, stageCount, closedFocus: document.activeElement === trigger() });
  } catch (error) {
    await emit('failed', { step, stageCount, message: String(error), last, keys: keys(),
      focus: focusKey(), currentMenu: menu()?.textContent.trim().slice(0, 600) });
  }
})();
