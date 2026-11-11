// Real production Vue + native IPC; the proof only observes adapters, never supplies a reply.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const panel = () => one('.lyrics-panel:not(.lyrics-panel-leave-active)');
  const notice = () => one('[data-voice-page-notice]', panel());
  const visible = (node) => !!node?.getClientRects().length && !node.closest('[inert]');
  const button = (label, root = document) =>
    [...(root?.querySelectorAll('button') ?? [])].find((node) => node.getAttribute('aria-label') === label);
  const search = () => one('input[aria-label="搜索音乐"]');
  const searchPanel = () => one('#search-discovery-panel:not(.menu-leave-active):not([inert])');
  const classicHome = () => {
    const root = one('.podcast-discovery:not(.page-leave-active)');
    return visible(one('.podcast-top[data-layout="classic"]', root)) && root;
  };
  const sceneCard = () => {
    const root = classicHome();
    return root ? one('[data-podcast-scene-fm]', root) : undefined;
  };
  function navigationReady() {
    const settings = button('打开设置');
    return visible(settings) && !settings.disabled;
  }
  function startupState() {
    const card = sceneCard();
    const cover = card?.querySelector('.scene-fm-cover');
    return {
      navigationReady: navigationReady(),
      searchExpanded: search()?.getAttribute('aria-expanded') ?? null,
      activeSearchPanel: visible(searchPanel()),
      classicHome: !!classicHome(),
      sceneCard: visible(card),
      sceneTitle: card?.querySelector('[data-scene-fm-title]')?.textContent.trim() ?? null,
      sceneError: card?.querySelector('[role="alert"]')?.textContent.trim().slice(0, 200) ?? null,
      sceneTransport: cover?.getAttribute('aria-label') ?? null,
      sceneTransportDisabled: cover?.disabled ?? null,
    };
  }
  const assert = (value, reason) => { if (!value) throw Error(reason); };
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const frame = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const signedActions = window.__voiceFreshActionsProof === true;
  const reads = { detail: [], recommendations: [], songs: [] };
  const actionReceipts = [];
  const actions = { targets: [], changes: [], stale: [] };
  const startup = {};
  let epoch, baseline, initialCoverBox, nonce = 0, count = 0, last = 'startup';
  let playbackMismatch;
  let restore = () => {};
  async function wait(test, label) {
    last = label;
    const deadline = performance.now() + 18000;
    while (performance.now() < deadline) {
      const value = await test();
      if (value) return value;
      await sleep(55);
    }
    throw Error(label);
  }
  async function click(find, label) {
    const node = await wait(() => {
      const candidate = find();
      return visible(candidate) && !candidate.disabled && candidate;
    }, label);
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    node.click();
  }
  async function nativeReads() {
    const request = ++nonce;
    await emit('voice_page_poll', { nonce: request });
    return wait(() => window.__voicePageNative?.[0] === request && window.__voicePageNative[1],
      'native triad trace');
  }
  async function playback() {
    const [player, queue] = await Promise.all([
      invoke('player_snapshot', { epoch }),
      invoke('player_queue', { epoch }),
    ]);
    return {
      playId: player.playId, entryId: player.entryId, voice: player.track?.voiceId,
      state: player.state, position: player.position, seeking: player.nativeSeeking,
      title: player.track?.name, snapshotCount: player.voiceDetail?.playCount,
      snapshotCover: player.voiceDetail?.coverUrl, error: player.error,
      snapshotCreator: player.voiceDetail?.creator?.id, snapshotRadio: player.voiceDetail?.radio?.id,
      queue: { revision: queue.revision, current: queue.current,
        rows: queue.tracks.map((row) => [row.entryId, row.track.id, row.track.voiceId]) },
    };
  }
  function dom() {
    const root = panel();
    const cover = one('[data-voice-cover]', root);
    const rect = cover?.getBoundingClientRect();
    const podcast = one('[data-voice-podcast-open]', root);
    const subscribe = one('[data-voice-podcast-subscribe]', root);
    return {
      state: root?.dataset.voicePageState ?? null,
      notice: visible(notice()), title: one('h1', root)?.textContent.trim() ?? null,
      cover: visible(cover),
      coverBox: rect ? [rect.x, rect.y, rect.width, rect.height] : null,
      count: one('[data-voice-play-count]', root)?.textContent.trim() ?? null,
      header: visible(one('[data-voice-detail]', root)),
      plainCreator: one('[data-voice-creator-name]', root)?.textContent.trim() ?? null,
      creatorLink: visible(one('[data-voice-creator]', root)),
      description: one('[data-voice-description-text]', root)?.textContent.trim() ?? null,
      radio: visible(podcast), radioName: podcast?.querySelector('.voice-podcast-name')?.textContent.trim() ?? null,
      radioOpenEnabled: visible(podcast) && !podcast.disabled,
      radioSubscribeEnabled: visible(subscribe) && !subscribe.disabled,
      recommendations: one('[data-voice-recommendations]', root)?.textContent.trim() ?? null,
      songs: one('[data-voice-song-card]', root)?.textContent.trim() ?? null,
    };
  }
  function assertUnpublished(expected) {
    const view = dom();
    assert(view.state === expected && view.notice && view.cover && view.title === baseline.title &&
      !view.header && view.count === null && view.description === null && !view.radio &&
      view.recommendations === null && view.songs === null,
    'pending or failed triad leaves only the basic title and current cover');
  }
  async function capture(name, route = null) {
    if (!route) {
      await wait(() => {
        const root = panel();
        return visible(root) && getComputedStyle(root).opacity === '1' &&
          !root.getAnimations().some((animation) => animation.pending || animation.playState === 'running');
      }, 'voice page compositor settles');
    }
    await frame();
    const current = await playback();
    if (!same(current, baseline)) {
      playbackMismatch = { stage: name, baseline, current };
      throw Error('joined read must not move the paused native decoder or its queue');
    }
    const view = dom();
    if (!route) {
      initialCoverBox ??= view.coverBox;
      assert(view.coverBox?.length === 4 && initialCoverBox?.every((edge, i) =>
        Math.abs(edge - view.coverBox[i]) < 0.5), 'cover stays anchored while metadata and count appear');
    }
    const stage = `ux_${String(++count).padStart(2, '0')}_${name}`;
    window.__podcastProgressStage = '';
    await emit(stage, { actual: document.visibilityState === 'visible', playback: current, dom: view, reads,
      ...(signedActions ? { actions, route } : {}) });
    await wait(() => window.__podcastProgressStage === stage, 'native stage acknowledgement');
  }
  const back = () => click(() => button('返回上一页', one('.topbar')), 'return using actual topbar Back');
  const discovery = () => visible(one('.podcast-discovery')) && !panel();
  async function signedReady(ordinal) {
    await wait(() => {
      const current = dom();
      return current.state === 'ready' && current.creatorLink && current.radioOpenEnabled &&
        current.radioName === '合成新播单' && reads.detail[ordinal]?.creatorId === '56' &&
        reads.detail[ordinal]?.radioId === '81' && actionReceipts[ordinal];
    }, 'changed fresh identity owns the visible actions');
  }
  async function exerciseSignedActions() {
    assert(baseline.snapshotCreator === '55' && baseline.snapshotRadio === '80', 'actual playback still has the old targets');
    await signedReady(2);
    const retired = actionReceipts[2];
    assert(retired && retired !== actionReceipts[1], 'fresh changed receipt replaces the previous visit');
    await capture('fresh_targets');

    await click(() => one('[data-voice-creator]', panel()), 'open fresh creator from actual card');
    const profile = await wait(() => {
      const heading = one('.playlist-creator-page h1');
      return !panel() && visible(heading) && heading.textContent.trim() === '合成新主播主页' && heading;
    }, 'fresh creator 56 renders its profile');
    await capture('creator_target', { kind: 'creator', title: profile.textContent.trim() });
    await back();
    await wait(discovery, 'Back restores classic podcast discovery after creator');
    await click(() => one('.player-cover'), 'reopen same episode after profile');
    await signedReady(3);
    assert(actionReceipts[3] !== retired, 'same play and changed IDs have a new visit receipt');

    const oldScope = { epoch, playId: baseline.playId, receipt: retired };
    const staleCalls = [
      ['player_vinyl_voice_detail_target', { target: { ...oldScope, kind: 'radio' } }],
      ['change_fresh_vinyl_podcast_subscription', { scope: oldScope, expected: false }],
      ['reconcile_fresh_vinyl_podcast_subscription', { scope: oldScope }],
    ];
    for (const [command, body] of staleCalls) {
      const code = await invoke(command, body).then(() => 'accepted', (error) => error?.code);
      assert(code === 'stale', `previous visit ${command} must be rejected by native IPC`);
      actions.stale.push({ command, code });
    }
    await capture('stale_receipt');

    await click(() => one('[data-voice-podcast-open]', panel()), 'open fresh radio from actual card');
    const radio = await wait(() => {
      const heading = one('.podcast-summary h2');
      return !panel() && visible(heading) && heading.textContent.trim() === '合成新播单' && heading;
    }, 'fresh radio 81 renders its page');
    await capture('radio_target', { kind: 'radio', title: radio.textContent.trim() });
    await back();
    await wait(discovery, 'Back restores classic podcast discovery after radio');
    await click(() => one('.player-cover'), 'reopen same episode after radio');
    await signedReady(4);
    assert(actionReceipts[4] !== actionReceipts[3], 'radio Back and reopen has a distinct visit receipt');
    const subscribe = () => one('[data-voice-podcast-subscribe]', panel());
    await wait(() => subscribe()?.getAttribute('aria-pressed') === 'false' && !subscribe().disabled,
      'new radio membership is loaded before the explicit action');
    await click(subscribe, 'subscribe to the changed fresh radio');
    await wait(() => subscribe()?.getAttribute('aria-pressed') === 'true' && !subscribe().disabled &&
      actions.changes[0]?.phase === 'confirmed', 'signed exact radio write is verified and reflected by Vue');
    await capture('fresh_subscription');
  }
  try {
    await wait(async () => {
      const account = await invoke('account_snapshot');
      if (account.status !== 'authenticated') return false;
      epoch = account.epoch;
      // Native account restoration can finish before useMusic permits routes.
      return visible(search()) && navigationReady();
    }, 'authenticated sealed shell can navigate');
    startup.shellReady = true;
    const provides = one('#app').__vue_app__._context.provides;
    const core = provides[Reflect.ownKeys(provides).find((key) => typeof key === 'symbol' && key.description === 'core-client')];
    const methods = [
      ['detail', 'playerVinylVoiceDetail'],
      ['recommendations', 'playerVinylVoiceRecommendations'],
      ['songs', 'playerVoiceSongs'],
    ];
    const originals = methods.map(([, name]) => core[name]);
    assert(originals.every((method) => typeof method === 'function'), 'registered production IPC adapters');
    for (const [index, [lane, name]] of methods.entries()) {
      core[name] = async (...args) => {
        const call = { args, outcome: 'pending' };
        reads[lane].push(call);
        try {
          const result = await originals[index].apply(core, args);
          call.outcome = 'success';
          call.receipt = result.receipt;
          call.title = result.title;
          call.cover = result.detail?.coverUrl;
          call.creatorId = result.detail?.creator?.id;
          call.radioId = result.detail?.radio?.id;
          if (lane === 'detail') {
            actionReceipts[reads.detail.indexOf(call)] = result.actionReceipt;
            call.actionReceipt = typeof result.actionReceipt === 'string';
          }
          call.rows = result.cards?.length ?? result.tracks?.length;
          return result;
        } catch (error) {
          call.outcome = 'failure';
          throw error;
        }
      };
    }
    restore = () => methods.forEach(([, name], index) => { core[name] = originals[index]; });
    if (signedActions) {
      const originalTarget = core.playerVinylVoiceDetailTarget;
      const originalChange = core.changeFreshVinylPodcastSubscription;
      assert(typeof originalTarget === 'function' && typeof originalChange === 'function', 'registered action adapters');
      const currentScope = (scope, keys) => scope.epoch === epoch && scope.playId === baseline.playId &&
        scope.receipt === actionReceipts.at(-1) && same(Object.keys(scope).sort(), keys);
      core.playerVinylVoiceDetailTarget = async (target) => {
        assert(currentScope(target, ['epoch', 'kind', 'playId', 'receipt']), 'UI route accepts only the latest receipt');
        const id = await originalTarget.call(core, target);
        actions.targets.push({ kind: target.kind, id });
        return id;
      };
      core.changeFreshVinylPodcastSubscription = async (scope, expected) => {
        assert(currentScope(scope, ['epoch', 'playId', 'receipt']), 'UI write accepts only the latest receipt');
        const result = await originalChange.call(core, scope, expected);
        const { id, observed, phase, acknowledged } = result.value;
        actions.changes.push({ expected, epoch: result.epoch, id, observed, phase, acknowledged });
        return result;
      };
      const restoreReads = restore;
      restore = () => {
        restoreReads();
        core.playerVinylVoiceDetailTarget = originalTarget;
        core.changeFreshVinylPodcastSubscription = originalChange;
      };
    }
    await click(search, 'open discovery');
    await click(() => {
      const panel = searchPanel();
      if (!navigationReady() || search()?.getAttribute('aria-expanded') !== 'true' || !visible(panel)) return;
      const more = button('播客榜：更多', panel);
      if (visible(more) && !more.disabled) startup.activeSearchChart = true;
      return more;
    }, 'active podcast chart More');
    await wait(classicHome, 'More opens the classic podcast home');
    startup.classicHome = true;
    const title = await wait(() => {
      const card = sceneCard();
      const episode = card?.querySelector('[data-scene-fm-title]');
      return visible(card) && visible(episode) && episode.textContent.trim();
    }, 'classic Scene FM episode is rendered');
    startup.sceneTitle = title;
    await click(() => {
      const card = sceneCard();
      return card ? button('播放随心听FM', card) : undefined;
    }, 'play sealed Scene FM program');
    const playing = await wait(async () => {
      const current = await playback();
      return current.state === 'playing' && current.voice === '601' &&
        current.playId && current.entryId === current.queue.current && current.position > .2 && current;
    }, 'real decoder advances before opening panel');
    startup.decoderVoice = '601';
    await click(() => button('暂停'), 'park the decoder');
    // WebView reads 55 ms apart can both precede the next 200 ms native
    // libmpv poll. Require the same decoder, queue and clock across polls.
    let sample, stableSince, samples = 0;
    baseline = await wait(async () => {
      const current = await playback();
      const now = performance.now();
      if (current.state !== 'paused' || current.voice !== '601' || current.seeking ||
          current.playId !== playing.playId || current.entryId !== playing.entryId ||
          !same(current.queue, playing.queue) || current.error ||
          !Number.isFinite(current.position) || current.position <= .2) {
        sample = undefined;
        samples = 0;
        return false;
      }
      if (!same(sample, current)) {
        sample = current;
        stableSince = now;
        samples = 0;
      }
      return ++samples >= 3 && now - stableSince >= 450 && current;
    }, 'same paused decoder, queue and clock settle across native polls');
    assert(!panel() && (await nativeReads()).length === 0 && baseline.snapshotCount === 123456,
      'closed page has no triad requests and old playback count is distinguishable');
    await click(() => one('.player-cover'), 'open current paused episode');
    await wait(async () => {
      const trace = await nativeReads();
      return dom().state === 'loading' && trace.length === 3 &&
        reads.detail[0]?.outcome === 'pending' &&
        reads.recommendations[0]?.outcome === 'success' && reads.recommendations[0].rows === 4 &&
        reads.songs[0]?.outcome === 'success' && reads.songs[0].rows === 1;
    }, 'two nonempty real sibling results arrived before held fresh detail');
    assertUnpublished('loading');
    await capture('held_no_partial');
    await emit('voice_page_release_failure');
    await wait(() => dom().state === 'failed' && reads.detail[0]?.outcome === 'failure',
      'held native detail fails without exposing successful siblings');
    assertUnpublished('failed');
    await capture('failed_no_partial');
    assert(Object.values(reads).every((lane) => lane.length === 1), 'failure does not replay reads');
    await click(() => button('收起播放详情', panel()), 'close failed visit');
    await wait(() => !panel(), 'old visit disposed');
    await click(() => one('.player-cover'), 'reopen the same paused episode');
    await wait(() => {
      const current = dom();
      return current.state === 'ready' && current.title === '合成新读节目标题' &&
        current.count === '0' && current.description === '本段只来自重新读取的节目详情。' &&
        current.recommendations?.includes('合成三路推荐 91') &&
        current.songs?.includes('合成三路包含歌曲') &&
        reads.detail[1]?.outcome === 'success' && reads.detail[1].cover === baseline.snapshotCover &&
        reads.recommendations[1]?.receipt && reads.songs[1]?.receipt;
    }, 'new visit publishes fresh title, string count and both receipt-backed siblings together');
    assert(!dom().notice && dom().header && dom().radio && dom().cover, 'ready detail keeps approved cover and metadata');
    assert(Object.values(reads).every((lane) => lane.length === 2 &&
      lane.every((call) => same(call.args, [epoch, baseline.playId]))),
    'reopening performs exactly one new native triad for the same play');
    await capture('reopened_fresh');
    await click(() => button('收起播放详情', panel()), 'close successful visit before mismatch');
    await wait(() => !panel(), 'successful visit disposed');
    await click(() => one('.player-cover'), 'reopen the current episode with changed detail identities');
    if (signedActions) {
      await exerciseSignedActions();
    } else {
      await wait(() => {
        const current = dom();
        return current.state === 'ready' && current.plainCreator === '合成仅展示主播' &&
          current.radioName === '合成仅展示播单' &&
          reads.detail[2]?.creatorId === '56' && reads.detail[2]?.radioId === '81';
      }, 'fresh creator and radio names display independently of navigation authority');
      const mismatch = dom();
      assert(!actionReceipts.some(Boolean) && !mismatch.creatorLink && !mismatch.radioOpenEnabled && !mismatch.radioSubscribeEnabled,
        'new read-only identity cannot acquire profile, radio or write authority');
      one('[data-voice-podcast-open]', panel()).click();
      one('[data-voice-podcast-subscribe]', panel()).click();
      await frame();
      assert(panel() && one('.podcast-discovery') && !one('.playlist-creator-page'),
        'disabled controls did not navigate away or open a profile');
      await capture('mismatch_read_only');
    }
    await emit('complete', { ok: true, stageCount: count, realMusicRequests: 0, realAccountWrites: 0,
      physicalAudioVerified: false, startup, ...(signedActions ? { actions } : {}) });
  } catch (error) {
    await emit('failed', { ok: false, last, error: String(error), reads, dom: dom(), stageCount: count,
      startup, startupState: startupState(), playbackMismatch,
      ...(signedActions ? { actions } : {}) });
  } finally {
    restore();
  }
})();
