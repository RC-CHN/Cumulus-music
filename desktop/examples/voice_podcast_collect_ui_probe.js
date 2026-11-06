// Real Scene FM playback, podcast card actions and closed native mutation readbacks.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector) ?? null;
  const panel = () => one('.lyrics-panel:not(.lyrics-panel-leave-active)');
  const card = () => one('.voice-podcast-card', panel());
  const open = () => one('[data-voice-podcast-open]', card());
  const subscribe = () => one('[data-voice-podcast-subscribe]', card());
  const status = () => one('[data-voice-podcast-status]', card());
  const confirm = () => one('dialog[data-voice-podcast-uncollect][open]');
  const choice = (text) => [...(confirm()?.querySelectorAll('button') ?? [])]
    .find((node) => node.textContent.trim() === text);
  const button = (label, root = document) => one(`button[aria-label="${label}"]`, root);
  const valid = (node) => !!node?.getClientRects().length && !node.closest('[inert]');
  const enabled = (node) => valid(node) && !node.disabled && node.getAttribute('aria-disabled') !== 'true';
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, values = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...values },
  });
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const frame = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const assert = (test, message) => { if (!test) throw Error(message); };
  let epoch, last = 'startup', count = 0, first, parked, restoreAdapters;
  const commands = [];
  const state = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  const detail = () => one('.podcast-cover h2', one('.main-content'))?.textContent.trim() ?? null;
  async function wait(predicate, label) {
    last = label;
    const deadline = performance.now() + 18000;
    while (performance.now() < deadline) {
      const value = await predicate();
      if (value) return value;
      await sleep(55);
    }
    throw Error(label);
  }
  async function click(locate, label) {
    const node = await wait(() => {
      const value = locate();
      return enabled(value) && value;
    }, label);
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    node.click();
  }
  function currentCard() {
    const toggle = subscribe();
    const action = open();
    return {
      shown: valid(card()),
      text: card()?.innerText.trim() ?? null,
      label: action?.textContent.trim() ?? null,
      openEnabled: enabled(action),
      subscribePresent: !!toggle,
      subscribeEnabled: enabled(toggle),
      subscribeLabel: toggle?.textContent.trim() ?? null,
      pressed: toggle?.getAttribute('aria-pressed') ?? null,
      subscribeFocused: document.activeElement === toggle,
      status: status()?.textContent.trim() ?? null,
      confirm: confirm()?.textContent.trim() ?? null,
      confirmCancelFocused: !!confirm() && document.activeElement === choice('取消'),
    };
  }
  function trackView(value) {
    return { state: value.state, playId: value.playId, entryId: value.entryId,
      queueRevision: value.queueRevision, voice: value.track?.voiceId, song: value.track?.id,
      position: value.position, seeking: value.nativeSeeking, error: value.error, radio: value.voiceDetail?.radio };
  }
  function samePaused(value) {
    return value.state === 'paused' && value.playId === parked.playId && value.entryId === parked.entryId &&
      value.queueRevision === parked.queueRevision && value.position === parked.position &&
      !value.nativeSeeking && !value.error && value.track?.voiceId === '601';
  }
  async function capture(label, extras = {}) {
    const stage = `ux_${String(++count).padStart(2, '0')}_${label}`;
    await frame();
    const player = await state();
    const rows = await queue();
    window.__podcastProgressStage = '';
    await emit(stage, { ...extras, actual: document.visibilityState === 'visible',
      title: one('h1', panel())?.textContent.trim() ?? null, card: currentCard(),
      state: trackView(player), page: detail(), commands: commands.slice(),
      queue: { revision: rows.revision, current: rows.current,
        entries: rows.tracks.map((entry) => [entry.entryId, entry.track.voiceId]) },
    });
    await wait(() => window.__podcastProgressStage === stage, `native records ${stage}`);
    return player;
  }
  function observedMutation(index, expected) {
    const call = commands[index];
    return call?.input.epoch === epoch && call.input.command.kind === 'change_vinyl_subscription' &&
      call.input.command.play_id === first.playId && call.input.command.expected === expected &&
      Object.keys(call.input.command).sort().join(',') === 'expected,kind,play_id' &&
      call.result?.epoch === epoch && call.result?.value?.id === '80' &&
      call.result.value.action === 'subscribe' && call.result.value.acknowledged === true &&
      call.result.value.phase === 'confirmed' && call.result.value.desired === !expected &&
      call.result.value.observed === !expected;
  }
  function shownSubscribed(value) {
    const control = subscribe();
    if (!enabled(control)) return false;
    const pressed = control.getAttribute('aria-pressed');
    if (pressed !== null) return pressed === String(value);
    const label = [control.textContent, control.getAttribute('aria-label'), control.getAttribute('title')]
      .filter(Boolean).join(' ');
    return value ? /已收藏|已订阅|取消收藏|取消订阅/.test(label)
      : /收藏|订阅/.test(label) && !/已收藏|已订阅|取消收藏|取消订阅/.test(label);
  }
  async function reject(playId, expected) {
    try {
      await invoke('podcast_command', { epoch, command: {
        kind: 'change_vinyl_subscription', play_id: playId, expected,
      } });
    } catch (error) {
      return typeof error?.code === 'string' ? error.code : String(error);
    }
    return null;
  }
  try {
    await wait(async () => {
      const account = await invoke('account_snapshot');
      if (account.status !== 'authenticated' || !enabled(button('打开设置'))) return false;
      epoch = account.epoch;
      return enabled(one('input[aria-label="搜索音乐"]'));
    }, 'authenticated closed playback shell');
    const provides = one('#app').__vue_app__._context.provides;
    const core = provides[Reflect.ownKeys(provides).find((key) => typeof key === 'symbol' && key.description === 'core-client')];
    assert(core?.podcastCommand, 'production native podcast adapter');
    const original = core.podcastCommand;
    const originalSnapshot = core.playerSnapshot;
    let holdOpen = false, resumeObservation, returnedVoice;
    core.playerSnapshot = async (owner) => {
      if (!holdOpen) return originalSnapshot.call(core, owner);
      holdOpen = false;
      await new Promise((resolve) => { resumeObservation = resolve; });
      const real = await originalSnapshot.call(core, owner);
      returnedVoice = real.track?.voiceId;
      return real;
    };
    core.podcastCommand = async (owner, command) => {
      const result = await original.call(core, owner, command);
      if (command.kind === 'change_vinyl_subscription') commands.push({
        input: { epoch: owner, command: { ...command } },
        result,
      });
      return result;
    };
    restoreAdapters = () => {
      core.podcastCommand = original;
      core.playerSnapshot = originalSnapshot;
      resumeObservation?.();
    };
    await click(() => one('input[aria-label="搜索音乐"]'), 'open discovery');
    await click(() => button('播客榜：更多', one('#search-discovery-panel:not(.menu-leave-active)')),
      'open classic podcast page');
    await wait(() => one('.podcast-discovery .podcast-top[data-layout="classic"]') &&
      one('[data-podcast-scene-fm] [data-scene-fm-title]'), 'classic Scene FM card ready');
    await click(() => button('播放随心听FM', one('[data-podcast-scene-fm]')), 'play first native episode');
    first = await wait(async () => {
      const value = await state();
      return value.state === 'playing' && value.track?.voiceId === '601' &&
        value.position > .2 && value.voiceDetail?.radio?.id === '80' && value;
    }, 'first native episode advances with its exact radio');
    assert(first.voiceDetail.radio.name === '合成来访播客' &&
      first.voiceDetail.radio.ownedByViewer === false && first.voiceDetail.radio.subscriberCount === 42 &&
      !JSON.stringify(first.voiceDetail.radio).includes('SYNTHETIC_RADIO_PRIVATE'),
    'native whitelist and current radio identity');
    await click(() => one('[data-podcast-scene-fm] [data-scene-fm-title]'), 'open the playing episode details');
    await wait(() => enabled(open()) && card()?.textContent.includes('合成来访播客') &&
      shownSubscribed(false), 'other-owned current card with complete membership');
    await click(() => button('暂停'), 'park the first decoder');
    let previous;
    parked = await wait(async () => {
      const value = await state();
      if (value.state !== 'paused' || value.playId !== first.playId || value.nativeSeeking ||
          value.error || !Number.isFinite(value.position)) return false;
      const settled = previous === value.position;
      previous = value.position;
      return settled && value;
    }, 'same paused native decoder settles');
    open().scrollIntoView({ block: 'nearest' });
    assert(samePaused(await capture('ready')), 'opening the card preserved native playback');
    await click(subscribe, 'explicitly collect the current podcast');
    await wait(() => observedMutation(0, false) && shownSubscribed(true), 'native collect receipt applied to the actual card');
    assert(samePaused(await capture('subscribed')), 'collect did not replace or move the decoder');
    await click(subscribe, 'open the official uncollect confirmation');
    await wait(() => confirm()?.textContent.includes('确定不再收藏该播单？') &&
      enabled(choice('不再收藏')) && document.activeElement === choice('取消'), 'uncollect copy and safe default focus');
    assert(samePaused(await capture('uncollect_confirmation')), 'opening confirmation did not move playback');
    await click(() => choice('取消'), 'dismiss confirmation without a write');
    await wait(() => !confirm() && shownSubscribed(true), 'cancel keeps collection and closes the dialog');
    assert(commands.length === 1 && samePaused(await capture('cancel_kept')), 'cancel sent no action and preserved playback');
    await click(subscribe, 'open the same confirmation for the explicit uncollect');
    await click(() => choice('不再收藏'), 'explicitly confirm uncollect for the current podcast');
    await wait(() => !confirm() && observedMutation(1, true) && shownSubscribed(false),
      'native uncollect receipt applied to the actual card');
    assert(samePaused(await capture('unsubscribed')), 'uncollect did not replace or move the decoder');

    await click(open, 'navigate by current podcast card title');
    await wait(() => detail() === '合成来访播客' && !panel(), 'current radio details replace the vinyl page');
    assert(samePaused(await capture('linked_detail')), 'opening radio route did not restart playback');

    await click(() => button('打开播放详情'), 'reopen the same paused native episode details');
    await wait(() => enabled(open()) && shownSubscribed(false) && card()?.textContent.includes('合成来访播客'),
      'same episode card restores after route change');
    assert(samePaused(await capture('reopened')), 'opening vinyl again preserved exact native playback');
    // Hold only the old card's actual native observation until after transport
    // selects the other episode. Return the real native snapshot unchanged.
    holdOpen = true;
    await click(open, 'start another open from the old card');
    await wait(() => resumeObservation, 'old card started its real native snapshot read');
    await click(() => button('播放'), 'resume the same episode before stepping');
    await wait(async () => {
      const value = await state();
      return value.state === 'playing' && value.playId === parked.playId && value.position > parked.position;
    }, 'the same decoder advances after resume');
    await click(() => button('下一首'), 'advance to the own native episode');
    const next = await wait(async () => {
      const value = await state();
      return value.state === 'playing' && value.playId !== first.playId && value.track?.voiceId === '602' &&
        value.position > .2 && value.voiceDetail?.radio?.id === '81' && card()?.textContent.includes('合成自有播客') &&
        enabled(open()) && !enabled(subscribe()) && value;
    }, 'new current owned radio has no active collection control');
    assert(next.voiceDetail.radio.ownedByViewer && next.voiceDetail.radio.subscriberCount === 7 &&
      !card().textContent.includes('合成来访播客'), 'card belongs only to the new episode');
    resumeObservation();
    await wait(() => returnedVoice === '602', 'delayed old card received the actual new episode snapshot');
    await frame();
    assert(card()?.textContent.includes('合成自有播客'), 'a stale route action did not close the current vinyl');
    const staleRejected = await reject(first.playId, false);
    const ownRejected = await reject(next.playId, false);
    assert(staleRejected === 'stale' && ownRejected && commands.length === 2,
      'old and self-owned actions cannot write');
    const ended = await capture('owned_after_switch', { staleRejected, ownRejected, staleRouteObservedVoice: returnedVoice });
    assert(ended.playId === next.playId && ended.entryId === next.entryId, 'negative actions preserve current episode');
    await emit('complete', { ok: true, stageCount: count, realMusicRequests: 0, realAccountWrites: 0,
      physicalAudioVerified: false });
  } catch (error) {
    await emit('failed', { ok: false, last, error: String(error), stageCount: count,
      card: currentCard(), commands, page: detail(), state: epoch ? await state().then(trackView, () => null) : null });
  } finally {
    restoreAdapters?.();
  }
})();
