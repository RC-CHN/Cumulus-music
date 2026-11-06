// Real production Vue + native IPC; the proof only observes adapters, never supplies a reply.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const panel = () => one('.lyrics-panel:not(.lyrics-panel-leave-active)');
  const notice = () => one('[data-voice-page-notice]', panel());
  const visible = (node) => !!node?.getClientRects().length && !node.closest('[inert]');
  const button = (label, root = document) =>
    [...(root?.querySelectorAll('button') ?? [])].find((node) => node.getAttribute('aria-label') === label);
  const assert = (value, reason) => { if (!value) throw Error(reason); };
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const frame = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const reads = { detail: [], recommendations: [], songs: [] };
  let epoch, baseline, nonce = 0, count = 0, last = 'startup';
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
      queue: { revision: queue.revision, current: queue.current,
        rows: queue.tracks.map((row) => [row.entryId, row.track.id, row.track.voiceId]) },
    };
  }
  function dom() {
    const root = panel();
    const podcast = one('[data-voice-podcast-open]', root);
    const subscribe = one('[data-voice-podcast-subscribe]', root);
    return {
      state: root?.dataset.voicePageState ?? null,
      notice: visible(notice()), title: one('h1', root)?.textContent.trim() ?? null,
      cover: visible(one('[data-voice-cover]', root)),
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
  async function capture(name) {
    if (window.__CUMULUS_PROOF_INVOKE__) {
      await wait(() => {
        const root = panel();
        return visible(root) && getComputedStyle(root).opacity === '1' &&
          !root.getAnimations().some((animation) => animation.pending || animation.playState === 'running');
      }, 'voice page compositor settles');
      await frame();
    }
    const current = await playback();
    assert(same(current, baseline), 'joined read must not move the paused native decoder or its queue');
    const stage = `ux_${String(++count).padStart(2, '0')}_${name}`;
    window.__podcastProgressStage = '';
    await emit(stage, { actual: document.visibilityState === 'visible', playback: current, dom: dom(), reads });
    await wait(() => window.__podcastProgressStage === stage, 'native stage acknowledgement');
  }
  try {
    await wait(async () => {
      const account = await invoke('account_snapshot');
      if (account.status !== 'authenticated') return false;
      epoch = account.epoch;
      return visible(one('input[aria-label="搜索音乐"]'));
    }, 'authenticated sealed shell');
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
          call.rows = result.cards?.length ?? result.tracks?.length;
          return result;
        } catch (error) {
          call.outcome = 'failure';
          throw error;
        }
      };
    }
    restore = () => methods.forEach(([, name], index) => { core[name] = originals[index]; });
    await click(() => one('input[aria-label="搜索音乐"]'), 'open discovery');
    await click(() => button('播客榜：更多'), 'open classic podcast page');
    await click(() => button('播放随心听FM', one('[data-podcast-scene-fm]')), 'play sealed Scene FM program');
    await wait(async () => {
      const current = await playback();
      return current.state === 'playing' && current.voice === '601' && current.position > .2;
    }, 'real decoder advances before opening panel');
    await click(() => button('暂停'), 'park the decoder');
    let lastPosition;
    baseline = await wait(async () => {
      const current = await playback();
      if (current.state !== 'paused' || current.voice !== '601' || current.seeking ||
          current.error || !Number.isFinite(current.position)) return false;
      const settled = lastPosition === current.position;
      lastPosition = current.position;
      return settled && current;
    }, 'pause clock settles');
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
    await wait(() => {
      const current = dom();
      return current.state === 'ready' && current.plainCreator === '合成仅展示主播' &&
        current.radioName === '合成仅展示播单' &&
        reads.detail[2]?.creatorId === '56' && reads.detail[2]?.radioId === '81';
    }, 'fresh creator and radio names display independently of navigation authority');
    const mismatch = dom();
    assert(!mismatch.creatorLink && !mismatch.radioOpenEnabled && !mismatch.radioSubscribeEnabled,
      'new read-only identity cannot acquire profile, radio or write authority');
    one('[data-voice-podcast-open]', panel()).click();
    one('[data-voice-podcast-subscribe]', panel()).click();
    await frame();
    assert(panel() && one('.podcast-discovery') && !one('.playlist-creator-page'),
      'disabled controls did not navigate away or open a profile');
    await capture('mismatch_read_only');
    await emit('complete', { ok: true, stageCount: count, realMusicRequests: 0, realAccountWrites: 0,
      physicalAudioVerified: false });
  } catch (error) {
    await emit('failed', { ok: false, last, error: String(error), reads, dom: dom(), stageCount: count });
  } finally {
    restore();
  }
})();
