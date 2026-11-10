(async () => {
  const one = selector => document.querySelector(selector);
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, payload = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...payload },
  });
  const assert = (value, message) => { if (!value) throw Error(message); };
  async function wait(read, message) {
    const end = performance.now() + 20000;
    while (performance.now() < end) {
      const value = await read();
      if (value) return value;
      await new Promise(resolve => setTimeout(resolve, 60));
    }
    throw Error(message);
  }
  async function reject(command, args) {
    try { await invoke(command, args); } catch { return; }
    throw Error(command + ' should reject');
  }
  const buttons = () => [...document.querySelectorAll('.lyrics-panel [data-lyric-bounty]')].filter(button => button.offsetWidth);
  const action = kind => buttons().find(button => button.dataset.lyricBounty === kind);
  const dialog = () => one('.lyric-bounty-dialog[open]:not([inert])');
  const confirm = () => [...(dialog()?.querySelectorAll('button') ?? [])].find(button => button.textContent.trim() === '确定');
  const dismiss = () => dialog()?.querySelector('footer button');
  let epoch, count = 0;
  let browserOnline = true;
  // This desktop is sealed; only the browser hint is simulated here. The
  // independently injected native route and typed client remain separate.
  Object.defineProperty(navigator, 'onLine', { configurable: true, get: () => browserOnline });
  function setBrowserOnline(online) {
    browserOnline = online;
    window.dispatchEvent(new Event(online ? 'online' : 'offline'));
  }
  async function route(value) {
    window.__lyricBountyRoute = null;
    await emit('lyric-bounty-route', { route: value });
    await wait(() => window.__lyricBountyRoute === value, 'native route acknowledgement');
  }
  const playback = () => invoke('player_snapshot', { epoch });
  async function select(song) {
    await invoke('play_selection', { epoch, selection: { kind: 'songs', ids: [song], index: 0, quality: 'standard' } });
    const state = await wait(async () => {
      const value = await playback();
      return value.state === 'playing' && value.track?.id === song && value;
    }, 'native playback ' + song);
    await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    await wait(async () => (await playback()).state === 'paused', 'paused song ' + song);
    if (!one('.lyrics-panel')) one('.playerbar [aria-label="歌词"]').click();
    return state.playId;
  }
  async function show(kind, status) {
    const trigger = await wait(() => action(kind) && !action(kind).disabled && action(kind), 'available ' + kind);
    trigger.click();
    await state(status, kind);
    return trigger;
  }
  async function state(status, kind) {
    return wait(() => dialog()?.dataset.bountyAction === kind && dialog().dataset.bountyStatus === status &&
      !dialog().classList.contains('modal-enter-active') && dialog(), 'dialog ' + status + ' / ' + kind);
  }
  async function close() {
    dismiss().click();
    await wait(() => !one('.lyric-bounty-dialog'), 'closed dialog');
  }
  async function capture(label, song) {
    await wait(() => !one('.lyrics-panel-enter-active,.lyrics-panel-leave-active'), 'settled lyrics');
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const current = await playback();
    assert(current.state === 'paused' && current.track?.id === song, 'preserved native selection');
    await emit('ux_' + String(++count).padStart(2, '0') + '_' + label,
      { captureHandshake: true, song, playId: current.playId });
  }
  try {
    const account = await wait(async () => {
      const value = await invoke('account_snapshot');
      return value.status === 'authenticated' && one('.playerbar') && value;
    }, 'sealed account');
    epoch = account.epoch;
    const first = await select('1');
    await wait(() => one('.lyrics-panel [data-lyric-contribution]'), 'ordinary empty document');
    if (!window.__lyricBountyEnabled) {
      assert(!(await invoke('lyric_bounty_actions', { epoch, playId: first })).length && !buttons().length,
        'ordinary desktop has no bounty authority');
      await reject('lyric_bounty_prepare', { epoch, playId: first, action: 'missing_lyrics' });
      await reject('lyric_bounty_confirm', { id: '0'.repeat(32) });
      await capture('ordinary_desktop_closed', '1');
      await emit('complete', { ok: true, stageCount: count, shippedGateClosed: true });
      return;
    }
    const trigger = await show('missing_lyrics', 'confirm');
    assert(dialog().textContent.includes('悬赏 1 云贝让大家来帮你补歌词，是否继续？') &&
      dialog().textContent.includes('（若30天内歌词未补充，云贝将退还给您）'), 'official confirmation copy');
    assert(document.activeElement === dismiss() && confirm() && !confirm().disabled,
      'Cancel is initially focused, confirmation is explicit');
    await capture('confirm_one_credit', '1');
    await close();
    assert(document.activeElement === trigger, 'cancel restores trigger focus');
    await capture('cancel_no_write', '1');

    await show('missing_lyrics', 'confirm');
    await route('unavailable');
    confirm().click();
    await state('not_sent', 'missing_lyrics');
    assert(!confirm(), 'unavailable native connectivity never sends');
    await capture('native_route_unknown', '1');
    await close();
    await route('present');

    await show('missing_lyrics', 'confirm');
    setBrowserOnline(false);
    await state('not_sent', 'missing_lyrics');
    assert(!confirm(), 'browser offline invalidates the outstanding confirmation');
    await capture('browser_offline', '1');
    await close();
    setBrowserOnline(true);

    await show('missing_lyrics', 'confirm');
    const accepted = confirm();
    accepted.click(); accepted.click();
    await state('accepted', 'missing_lyrics');
    assert(!confirm() && dialog().textContent.includes('反馈信息已发送'), 'accepted receipt, no repeat button');
    await capture('accepted_once', '1');
    await close();
    await show('missing_lyrics', 'accepted');
    assert(!confirm(), 'accepted journal survives reopening');
    await capture('accepted_reopened', '1');
    await close();

    await select('2');
    await wait(() => buttons().length === 2, 'both populated options');
    assert(buttons().map(button => button.textContent.trim()).join('/') === '求滚动/求翻译', 'official body order');
    await show('scrolling', 'confirm');
    confirm().click();
    await state('unknown', 'scrolling');
    assert(!confirm() && dialog().textContent.includes('请勿重复提交'), 'lost acknowledgement cannot be replayed');
    await capture('unknown_no_repeat', '2');
    await close();
    await show('scrolling', 'unknown');
    assert(!confirm(), 'unknown receipt survives reopening');
    await capture('unknown_reopened', '2');
    await close();
    await show('translation', 'confirm');
    assert(dialog().textContent.includes('悬赏 1 云贝让大家来帮你翻译歌词，是否继续？'), 'translation wording');
    confirm().click();
    await state('accepted', 'translation');
    await capture('translation_confirmed', '2');
    await close();

    const third = await select('3');
    await wait(() => action('translation'), 'third song is eligible');
    const draft = await invoke('lyric_bounty_prepare', { epoch, playId: third, action: 'translation' });
    assert(draft.id, 'native draft is prepared without dispatch');
    await select('4');
    await wait(() => one('.lyrics-panel .empty-state')?.textContent.includes('纯音乐，请欣赏'), 'instrumental');
    await reject('lyric_bounty_confirm', { id: draft.id });
    await reject('lyric_bounty_prepare', { epoch, playId: third, action: 'translation' });
    await reject('lyric_bounty_confirm', { id: '0'.repeat(32) });
    assert(!buttons().length, 'instrumental has no bounty controls');
    await capture('stale_and_forged_closed', '4');
    await emit('complete', { ok: true, stageCount: count, staleRejected: true, noAutoDispatch: true });
  } catch (error) {
    await emit('failed', { error: String(error), stack: error.stack,
      dialog: dialog()?.innerText, panel: one('.lyrics-panel')?.innerText.slice(0, 400), stageCount: count });
  }
})();
