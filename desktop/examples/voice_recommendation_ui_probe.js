// Production VoicePage recommendations, native read/page fences and radio navigation.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector) ?? null;
  const panel = () => one('.lyrics-panel:not(.lyrics-panel-leave-active)');
  const shelf = () => one('[data-voice-recommendations]', panel());
  const next = () => one('[data-voice-recommendations-next]', shelf());
  const card = (id) => one(`[data-voice-recommendation-card][data-radio-id="${id}"]`, shelf());
  const ids = () => [...(shelf()?.querySelectorAll('[data-voice-recommendation-card]') ?? [])]
    .map((node) => node.dataset.radioId);
  const open = () => button('打开播放详情');
  const button = (label, root = document) => one(`button[aria-label="${label}"]`, root);
  const shown = (node) => !!node?.getClientRects().length && !node.closest('[inert]');
  const enabled = (node) => shown(node) && !node.disabled && node.getAttribute('aria-disabled') !== 'true';
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, values = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...values },
  });
  const state = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  const detail = () => one('.main-content .podcast-cover h2')?.textContent.trim() ?? null;
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const frame = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const assert = (test, message) => { if (!test) throw Error(message); };
  const group = (start, length) => Array.from({ length }, (_, index) => String(start + index));
  const PAGE1 = group(90, 6), PAGE2 = group(96, 4), NEW = group(120, 4);
  let epoch, last = 'startup', nonce = 0, stageCount = 0, baseline, restore, oldReceipt, stalePage;
  const reads = [], pages = [], targets = [];
  let shell, negativeTargets;

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
  function player(value) {
    return { state: value.state, playId: value.playId, entryId: value.entryId, queueRevision: value.queueRevision,
      voice: value.track?.voiceId, song: value.track?.id, position: value.position,
      seeking: value.nativeSeeking, error: value.error };
  }
  function shelfView() {
    return { visible: shown(shelf()), title: shelf()?.textContent.trim() ?? null, ids: ids(),
      page: shelf()?.dataset.page ?? null, pageCount: shelf()?.dataset.pageCount ?? null,
      nextPresent: !!next(), nextEnabled: enabled(next()), nextFocused: document.activeElement === next() };
  }
  function receiptView(value) {
    return { voiceId: value.voiceId, playId: value.playId, receipt: value.receipt,
      page: value.page, pageCount: value.pageCount,
      cards: value.cards.map((card) => ({
        keys: Object.keys(card).sort(), id: card.radioId, name: card.name, playCount: card.playCount,
      })) };
  }
  function equal(a, b) { return JSON.stringify(a) === JSON.stringify(b); }
  function matchesPage(page, count, order) {
    return shown(shelf()) && shelf().dataset.page === String(page) &&
      shelf().dataset.pageCount === String(count) && equal(ids(), order);
  }
  function samePaused(value) {
    return value.state === 'paused' && value.playId === baseline.playId && value.entryId === baseline.entryId &&
      value.queueRevision === baseline.queueRevision && value.position === baseline.position &&
      !value.nativeSeeking && !value.error && value.track?.voiceId === '601';
  }
  async function sampleNative() {
    const id = ++nonce;
    await emit('voice_recommendation_poll', { nonce: id });
    return wait(() => window.__voiceRecommendationNative?.[0] === id && window.__voiceRecommendationNative[1],
      'native signed recommendation trace');
  }
  async function capture(label, extras = {}) {
    const stage = `ux_${String(++stageCount).padStart(2, '0')}_${label}`;
    await wait(() => {
      if (one('.lyrics-panel-enter-active,.lyrics-panel-leave-active')) return false;
      const active = panel();
      return !active || Number(getComputedStyle(active).opacity) >= .999;
    }, 'vinyl panel enter or leave is visually settled');
    shelf()?.scrollIntoView({ block: 'nearest' });
    await frame();
    const current = await state();
    const rows = await queue();
    window.__podcastProgressStage = '';
    await emit(stage, { ...extras, actual: document.visibilityState === 'visible', shelf: shelfView(),
      reads: structuredClone(reads), pages: structuredClone(pages), targets: structuredClone(targets),
      player: player(current), detail: detail(),
      queue: { revision: rows.revision, current: rows.current,
        entries: rows.tracks.map((entry) => [entry.entryId, entry.track.voiceId]) },
    });
    await wait(() => window.__podcastProgressStage === stage, `native records ${stage}`);
    return current;
  }
  function errCode(error) { return typeof error?.code === 'string' ? error.code : String(error); }
  async function rejectTarget(playId, page, radioId) {
    try {
      await invoke('player_vinyl_voice_recommendation_target', { epoch, playId, receipt: oldReceipt, page, radioId });
    } catch (error) { return errCode(error); }
    return null;
  }
  try {
    await wait(async () => {
      const account = await invoke('account_snapshot');
      shell = { accountStatus: account.status, appChildren: one('#app')?.childElementCount,
        settings: enabled(button('打开设置')), search: enabled(one('input[aria-label="搜索音乐"]')) };
      if (account.status !== 'authenticated' || !shell.settings) return false;
      epoch = account.epoch;
      return enabled(one('input[aria-label="搜索音乐"]'));
    }, 'authenticated closed playback shell');
    const provides = one('#app').__vue_app__._context.provides;
    const core = provides[Reflect.ownKeys(provides).find((key) => typeof key === 'symbol' && key.description === 'core-client')];
    const originalRead = core.playerVinylVoiceRecommendations;
    const originalNext = core.playerVinylVoiceRecommendationsNext;
    const originalTarget = core.playerVinylVoiceRecommendationTarget;
    assert(originalRead && originalNext && originalTarget, 'production recommendation IPC adapters');
    function trace(original, calls) {
      return async (...args) => {
        const call = { args: [...args] };
        calls.push(call);
        try {
          const result = await original.apply(core, args);
          call.result = receiptView(result);
          assert(!JSON.stringify(result).includes('SYNTHETIC_RECOMMENDATION_PRIVATE'), 'private fixture sentinel not returned by native');
          return result;
        } catch (error) {
          call.error = errCode(error);
          throw error;
        }
      };
    }
    core.playerVinylVoiceRecommendations = trace(originalRead, reads);
    core.playerVinylVoiceRecommendationsNext = trace(originalNext, pages);
    core.playerVinylVoiceRecommendationTarget = async (target) => {
      const call = { target: { ...target } };
      targets.push(call);
      try {
        const result = await originalTarget.call(core, target);
        call.result = result;
        return result;
      } catch (error) {
        call.error = errCode(error);
        throw error;
      }
    };
    restore = () => {
      core.playerVinylVoiceRecommendations = originalRead;
      core.playerVinylVoiceRecommendationsNext = originalNext;
      core.playerVinylVoiceRecommendationTarget = originalTarget;
    };
    await click(() => one('input[aria-label="搜索音乐"]'), 'open discovery');
    await click(() => button('播客榜：更多', one('#search-discovery-panel:not(.menu-leave-active)')),
      'open classic podcast page');
    await wait(() => one('.podcast-discovery .podcast-top[data-layout="classic"]') &&
      one('[data-podcast-scene-fm] [data-scene-fm-title]'), 'classic Scene FM card ready');
    await click(() => button('播放随心听FM', one('[data-podcast-scene-fm]')), 'play first native episode');
    const first = await wait(async () => {
      const value = await state();
      return value.state === 'playing' && value.track?.voiceId === '601' && value.position > .2 && value;
    }, 'first native episode advances while vinyl closed');
    await click(() => button('暂停'), 'park the first decoder');
    let previous;
    baseline = await wait(async () => {
      const value = await state();
      if (value.state !== 'paused' || value.playId !== first.playId || value.nativeSeeking ||
          value.error || !Number.isFinite(value.position)) return false;
      const settled = previous === value.position;
      previous = value.position;
      return settled && value;
    }, 'same paused native decoder settles');
    assert(!panel() && !reads.length && !(await sampleNative()).requests.length, 'closed vinyl never requested a recommendation');
    assert(samePaused(await capture('closed')), 'closed snapshot preserved paused decoder');
    await click(open, 'open the exact paused episode');
    await wait(() => matchesPage(1, 2, PAGE1) && reads[0]?.result, 'official first six current-episode cards');
    oldReceipt = reads[0].result.receipt;
    assert(oldReceipt && reads[0].result.playId === first.playId && reads[0].result.voiceId === '601' &&
      shelf().textContent.includes('听了该声音的人也在听') && enabled(next()), 'official title and scoped page receipt');
    assert(samePaused(await capture('first_page')), 'shelf initial read did not move playback');
    await click(next, 'cycle the real recommendation heading to page two');
    await wait(() => matchesPage(2, 2, PAGE2) && pages[0]?.result, 'second page retains four ordered cards');
    assert(samePaused(await capture('short_page')), 'short local page did not move playback');
    await click(next, 'cycle back to first page');
    await wait(() => matchesPage(1, 2, PAGE1) && pages[1]?.result, 'last short page wraps to first six');
    assert(samePaused(await capture('wrapped')), 'wrap did not move playback');
    await click(next, 'return to the page holding the target radio');
    await wait(() => matchesPage(2, 2, PAGE2) && pages[2]?.result, 'target page ready');
    assert((await sampleNative()).requests.length === 1 &&
      pages.every((call) => call.result.receipt === oldReceipt), 'three page cycles use the same local receipt');
    const wrongPage = await rejectTarget(first.playId, 1, '98');
    const wrongRadio = await rejectTarget(first.playId, 2, '90');
    const beforeRoute = await sampleNative();
    negativeTargets = { wrongPage, wrongRadio };
    assert(wrongPage === 'stale' && wrongRadio === 'stale' && matchesPage(2, 2, PAGE2) &&
      beforeRoute.routes.every(([path]) => path === '/api/content/my/liked/voice'),
    'native rejects mismatched page and off-page radio without navigation');
    await click(() => card('98'), 'open radio id 98 from its actual card');
    await wait(() => detail() === '合成推荐播单 98' && !panel() && targets[0]?.result === '98',
      'native resolves the actual shelf target before podcast route and vinyl close');
    assert(samePaused(await capture('linked_detail', { wrongPage, wrongRadio })),
      'radio navigation did not select its episode or change queue');
    await click(open, 'reopen the same episode shelf for a held real read');
    await wait(async () => {
      const native = await sampleNative();
      return native.oldPending && native.requests.length === 2 && reads[1] && !reads[1].result;
    }, 'actual old signed native read is held, not a fabricated renderer promise');
    assert(!shown(shelf()), 'no previous shelf may render while reopening');
    await click(() => button('播放'), 'resume the same episode before step');
    await wait(async () => {
      const value = await state();
      return value.state === 'playing' && value.playId === baseline.playId && value.position > baseline.position;
    }, 'native baseline decoder resumes');
    await click(() => button('下一首'), 'advance to the second real native episode');
    const second = await wait(async () => {
      const value = await state();
      return value.state === 'playing' && value.playId !== first.playId && value.track?.voiceId === '602' &&
        value.position > .2 && matchesPage(1, 1, NEW) && reads[2]?.result?.voiceId === '602' && value;
    }, 'new native episode has its own one-page shelf');
    await click(next, 'one-page official refresh remains actionable locally');
    await wait(() => matchesPage(1, 1, NEW) && pages[3]?.result?.voiceId === '602' &&
      pages[3].result.page === 1, 'one-page refresh retains its own four cards');
    await capture('new_episode');
    await emit('voice_recommendation_release');
    await wait(() => reads[1]?.error === 'stale', 'native rejected the late old episode response');
    try {
      await invoke('player_vinyl_voice_recommendations_next', { epoch, playId: first.playId, receipt: oldReceipt });
    } catch (error) { stalePage = errCode(error); }
    const staleTarget = await rejectTarget(first.playId, 2, '98');
    Object.assign(negativeTargets, { stalePage, staleTarget });
    await frame();
    assert(stalePage === 'stale' && staleTarget === 'stale' && matchesPage(1, 1, NEW) &&
      !shelf().textContent.includes('合成推荐播单 140'), 'old receipt and old native result cannot change the current shelf');
    const after = await capture('old_rejected', { stalePage, staleTarget });
    assert(after.playId === second.playId && after.entryId === second.entryId && !after.error,
      'stale recommendation did not disrupt the new decoder');
    assert(detail() === '合成推荐播单 98', 'earlier recommendation detail is still underneath the current vinyl');
    await click(() => button('暂停'), 'park the second native decoder before nested navigation');
    previous = undefined;
    const nestedBaseline = await wait(async () => {
      const value = await state();
      if (value.state !== 'paused' || value.playId !== second.playId || value.entryId !== second.entryId ||
          value.nativeSeeking || value.error || !Number.isFinite(value.position)) return false;
      const settled = previous === value.position;
      previous = value.position;
      return settled && value;
    }, 'same second paused native decoder settles');
    await click(() => card('121'), 'open a different actual recommendation from radio 98');
    await wait(() => detail() === '合成推荐播单 121' && !panel() && targets[1]?.result === '121',
      'scoped recommendation opens nested radio 121');
    const nested = await capture('nested_detail', { beforeNavigation: player(nestedBaseline) });
    assert(equal(player(nested), player(nestedBaseline)), 'nested route preserves paused decoder and clock');
    await click(() => button('返回上一页', one('.topbar')), 'use actual topbar Back to restore the prior radio');
    await wait(() => detail() === '合成推荐播单 98' && !panel(), 'Back restores exact prior radio 98');
    const restored = await capture('nested_back');
    assert(equal(player(restored), player(nestedBaseline)), 'Back preserves same paused decoder and clock');
    await emit('complete', { ok: true, stageCount, realMusicRequests: 0, realAccountWrites: 0,
      physicalAudioVerified: false });
  } catch (error) {
    await emit('failed', { ok: false, last, error: String(error), stageCount, shell, shelf: shelfView(),
      reads, pages, targets, negativeTargets, detail: detail(),
      player: epoch ? await state().then(player, () => null) : null });
  } finally { restore?.(); }
})();
