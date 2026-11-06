// Actual vinyl included-song interactions and native queue/transport receipts.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector) ?? null;
  const panel = () => one('.lyrics-panel:not(.lyrics-panel-leave-active)');
  const shelf = () => one('[data-voice-song-card]', panel());
  const rows = () => [...(shelf()?.querySelectorAll('[data-voice-song-row]') ?? [])];
  const row = (index) => one(`[data-voice-song-row][data-row-index="${index}"]`, shelf());
  const art = (index) => one('[data-voice-song-cover]', row(index));
  const heart = (index) => one('[data-voice-song-heart]', row(index));
  const next = () => one('[data-menu-key="next"]') ??
    [...document.querySelectorAll('[role="menuitem"]')].find((n) => n.textContent.trim() === '下一首播放');
  const menuPlay = () => one('[data-menu-key="play"]');
  const label = (name, root = document) => one(`button[aria-label="${name}"]`, root);
  const visible = (node) => !!node?.getClientRects().length && !node.closest('[inert]');
  const enabled = (node) => visible(node) && !node.disabled && node.getAttribute('aria-disabled') !== 'true';
  const disabled = (node) => visible(node) && (node.disabled || node.getAttribute('aria-disabled') === 'true');
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, values = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...values },
  });
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const frame = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const assert = (value, message) => { if (!value) throw Error(message); };
  const equal = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const errorCode = (error) => typeof error?.code === 'string' ? error.code : String(error);
  const mode = window.__voiceSongsCase;
  const reads = [], plays = [], likes = [];
  let epoch, last, count = 0, nonce = 0, restore, reject;
  const state = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  const stateView = (value) => ({
    kind: value.resourceKind, trackId: value.track?.id, voiceId: value.track?.voiceId ?? null,
    playId: value.playId, entryId: value.entryId, queueRevision: value.queueRevision,
    queueLen: value.queueLen, state: value.state, position: value.position,
    seeking: value.nativeSeeking, error: value.error,
  });
  const queueView = (value) => ({
    revision: value.revision, current: value.current,
    entries: value.tracks.map((entry) => ({
      entryId: entry.entryId, id: entry.track.id, voiceId: entry.track.voiceId ?? null,
    })),
  });
  const pressed = (index) => heart(index)?.getAttribute('aria-pressed');
  const selected = (index) => row(index)?.getAttribute('aria-selected') === 'true';

  async function wait(predicate, message) {
    last = message;
    const deadline = performance.now() + 18000;
    while (performance.now() < deadline) {
      const value = await predicate();
      if (value) return value;
      await sleep(55);
    }
    throw Error(message);
  }
  async function click(find, message) {
    const node = await wait(() => {
      const current = find();
      return enabled(current) && current;
    }, message);
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    node.click();
  }
  function expectedRows(names) {
    return visible(shelf()) && rows().length === names.length &&
      names.every((name, index) => row(index)?.textContent.includes(name));
  }
  function trackIds(value) { return value.entries.map((entry) => entry.id); }
  async function park(playId) {
    await click(() => label('暂停'), 'park current actual decoder');
    let previous;
    return wait(async () => {
      const current = await state();
      if (current.state !== 'paused' || current.playId !== playId || current.nativeSeeking ||
          current.error || !Number.isFinite(current.position)) return false;
      const settled = previous === current.position;
      previous = current.position;
      return settled && current;
    }, 'paused clock and same decoder settle');
  }
  async function waitPlaying(kind, id) {
    return wait(async () => {
      const current = await state();
      const resource = kind === 'voice' ? current.track?.voiceId : current.track?.id;
      return current.state === 'playing' && current.resourceKind === kind && resource === id &&
        current.position > .2 && !current.nativeSeeking && !current.error && current;
    }, `actual ${kind} ${id} advances`);
  }
  async function nativeTrace() {
    const current = ++nonce;
    await emit('voice_songs_poll', { nonce: current });
    return wait(() => window.__voiceSongsNative?.[0] === current && window.__voiceSongsNative[1],
      'sealed transport observation');
  }
  async function capture(name, extras = {}) {
    await wait(() => {
      if (one('.lyrics-panel-enter-active,.lyrics-panel-leave-active')) return false;
      return !panel() || Number(getComputedStyle(panel()).opacity) >= .999;
    }, 'vinyl panel visually settled');
    await frame();
    const id = `ux_${String(++count).padStart(2, '0')}_${name}`;
    const player = stateView(await state());
    const nativeQueue = queueView(await queue());
    const dom = { shelf: visible(shelf()), title: shelf()?.textContent.trim() ?? null,
      rows: rows().map((node) => ({
        index: node.dataset.rowIndex, text: node.textContent.trim(),
        selected: node.getAttribute('aria-selected'),
        coverDisabled: disabled(one('[data-voice-song-cover]', node)),
      })),
      nextDisabled: disabled(next()), playDisabled: disabled(menuPlay()),
      focusedRow: document.activeElement?.closest('[data-voice-song-row]')?.dataset.rowIndex ?? null,
    };
    window.__podcastProgressStage = '';
    await emit(id, { ...extras, actual: document.visibilityState === 'visible', mode, player,
      queue: nativeQueue, dom, reads: structuredClone(reads), plays: structuredClone(plays), likes: structuredClone(likes) });
    await wait(() => window.__podcastProgressStage === id, 'native saved stage before next input');
    return { player, queue: nativeQueue };
  }
  async function context(index) {
    const item = await wait(() => visible(row(index)) && row(index), 'row can open actual context menu');
    item.scrollIntoView({ block: 'nearest' });
    const box = item.getBoundingClientRect();
    item.focus({ preventScroll: true });
    item.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true,
      button: 2, clientX: box.left + Math.min(40, box.width / 2), clientY: box.top + box.height / 2 }));
    await wait(() => visible(next()), 'actual 下一首播放 context entry');
  }
  function dbl(index) {
    row(index).dispatchEvent(new MouseEvent('dblclick', { bubbles: true, cancelable: true, detail: 2 }));
  }
  function observedLike(index, desired, receipt, playId) {
    const call = likes[index];
    return equal(call?.args, [epoch, playId, receipt, 0, desired]) &&
      call.result?.epoch === epoch && call.result.value?.id === '1710' &&
      call.result.value.phase === 'confirmed' && call.result.value.observed === desired &&
      pressed(0) === String(desired);
  }
  async function stale(cmd, body) {
    try { await invoke(cmd, body); } catch (error) { return errorCode(error); }
    return null;
  }
  try {
    assert(['cover', 'double', 'dense'].includes(mode), 'explicit isolated case');
    await wait(async () => {
      const account = await invoke('account_snapshot');
      if (account.status !== 'authenticated' || !enabled(label('打开设置'))) return false;
      epoch = account.epoch;
      return enabled(one('input[aria-label="搜索音乐"]'));
    }, 'authenticated synthetic shell ready');
    const provides = one('#app').__vue_app__._context.provides;
    const core = provides[Reflect.ownKeys(provides).find((key) => typeof key === 'symbol' && key.description === 'core-client')];
    const methods = [['playerVoiceSongs', reads], ['playerVoiceSongPlay', plays], ['playerVoiceSongLike', likes]];
    const originals = methods.map(([name]) => core[name]);
    assert(originals.every((method) => typeof method === 'function'), 'real typed native included-song adapters');
    for (const [i, [name, trace]] of methods.entries()) {
      core[name] = async (...args) => {
        const call = { args: structuredClone(args) };
        trace.push(call);
        try {
          const value = await originals[i].apply(core, args);
          assert(!JSON.stringify(value).includes('SYNTHETIC_INCLUDED_PRIVATE'), 'native never projects private raw fields');
          call.result = name === 'playerVoiceSongPlay' ? stateView(value) : value;
          return value;
        } catch (error) {
          call.error = errorCode(error);
          throw error;
        }
      };
    }
    restore = () => { methods.forEach(([name], i) => { core[name] = originals[i]; }); };
    const preferences = await invoke('queue_click_preferences', { epoch });
    assert(preferences.value.replaceQueue === true, 'actual default double-click replaces queue');
    await click(() => one('input[aria-label="搜索音乐"]'), 'open discovery');
    await click(() => label('播客榜：更多', one('#search-discovery-panel:not(.menu-leave-active)')),
      'open classic podcast page');
    await wait(() => one('.podcast-discovery .podcast-top[data-layout="classic"]') &&
      one('[data-podcast-scene-fm] [data-scene-fm-title]'), 'classic Scene FM card ready');
    await click(() => label('播放随心听FM', one('[data-podcast-scene-fm]')), 'play first native episode');
    const first = await waitPlaying('voice', '601');
    const firstPaused = await park(first.playId);
    assert(!(await nativeTrace()).songReads.length && !reads.length, 'closed vinyl never reads included songs');
    await click(() => label('打开播放详情'), 'open first exact episode vinyl');
    if (mode === 'dense') {
      const start = await wait(() => {
        const page = reads[0]?.result;
        const grid = one('[role="grid"]', shelf());
        const scroller = shelf()?.closest('.voice-playback-cards');
        return page?.tracks.length === 100 && grid?.getAttribute('aria-rowcount') === '100' &&
          visible(row(0)) && scroller && { page, grid, scroller };
      }, '100 rows observed through one native read and bounded actual shelf');
      const { page, grid, scroller } = start;
      assert(page.playId === first.playId && page.voiceId === '601' && page.receipt &&
        page.tracks.every((item, index) => item.id === String(2201 + index)), 'native preserves all 100 IDs in order');
      let maximum = 0;
      const observe = () => { maximum = Math.max(maximum, rows().length); };
      const observer = new MutationObserver(observe);
      observer.observe(grid, { childList: true, subtree: true });
      observe();
      try {
        const key = (name) => {
          assert(document.activeElement?.matches('[data-voice-song-row]'), 'actual song row owns keyboard focus');
          document.activeElement.dispatchEvent(new KeyboardEvent('keydown', { key: name, bubbles: true, cancelable: true }));
        };
        const intersects = (index) => {
          const entry = row(index);
          if (!visible(entry)) return false;
          const box = scroller.getBoundingClientRect(), rect = entry.getBoundingClientRect();
          return rect.top >= box.top - 1 && rect.bottom <= box.bottom + 1;
        };
        const geometry = () => {
          observe();
          assert(scroller === shelf()?.closest('.voice-playback-cards') && getComputedStyle(scroller).overflowY === 'auto',
            'the same real outer shelf scrolls');
          const cap = Math.ceil(scroller.clientHeight / 60) + 17;
          const inner = [grid, shelf()].every((element) =>
            !['auto', 'scroll'].includes(getComputedStyle(element).overflowY));
          assert(inner && maximum <= cap && rows().length > 0 && scroller.scrollHeight > 6000,
            'bounded 60px rows without a second inner scroll');
          return { total: Number(grid.getAttribute('aria-rowcount')), rendered: rows().length,
            maxRendered: maximum, cap, top: scroller.scrollTop, height: scroller.clientHeight,
            firstRendered: Number(rows()[0].dataset.rowIndex), lastRendered: Number(rows().at(-1).dataset.rowIndex),
            firstVisible: intersects(0), lastVisible: intersects(99), noInnerScroll: inner };
        };
        row(0).focus({ preventScroll: true });
        await wait(() => document.activeElement === row(0) && intersects(0), 'focus first actual row in viewport');
        const initialGeometry = geometry();
        const initial = await capture('dense_first', { geometry: initialGeometry });
        key('End');
        await wait(() => document.activeElement === row(99) && selected(99) && intersects(99) && !row(0),
          'End mounts and focuses only the last viewport rows');
        const tailGeometry = geometry();
        assert(tailGeometry.top - initialGeometry.top > 4000, 'End scrolled the real outer shelf');
        const tail = await capture('dense_end', { geometry: tailGeometry });
        key('Home');
        await wait(() => document.activeElement === row(0) && selected(0) && intersects(0) && !row(99),
          'Home restores the first viewport rows and actual focus');
        const backGeometry = geometry();
        assert(backGeometry.top <= initialGeometry.top + 1, 'Home returned to the previous outer scroll start');
        const back = await capture('dense_home', { geometry: backGeometry });
        assert(!plays.length && !likes.length && reads.length === 1 &&
          [initial, tail, back].every((value) => equal(value.player, stateView(firstPaused)) && equal(value.queue, initial.queue)),
        'focus and scroll alone do not act on playback, membership or the wire');
      } finally { observer.disconnect(); }
      await emit('complete', { ok: true, mode, count, realMusicRequests: 0, realAccountWrites: 0,
        physicalAudioVerified: false });
      return;
    }
    const oldPage = await wait(() => expectedRows(['1710', '与播客音轨同编号的普通歌', '合成版权受限歌曲', '合成菜单版权受限歌曲']) &&
      reads[0]?.result, 'ordered first shelf with unavailable row and filtered missing album');
    assert(oldPage.playId === first.playId && oldPage.voiceId === '601' && oldPage.receipt &&
      equal(oldPage.tracks.map((item) => item.id), ['1710', '1601', '1790', '1791']) &&
      oldPage.availability[2].copyrightBlocked && !oldPage.availability[2].onlineEligible &&
      shelf().textContent.includes('包含歌曲'), 'real source-bound first page');
    const baseline = await capture('first_page');
    assert(equal(baseline.player, stateView(firstPaused)), 'reading shelf preserves paused player');

    if (mode === 'cover') {
      await click(() => art(1), 'click only the ordinary song artwork matching the episode media ID');
      const ordinary = await waitPlaying('catalog', '1601');
      const after = await capture('cover_single');
      assert(plays.length === 1 && equal(plays[0].args, [epoch, first.playId, oldPage.receipt, { kind: 'cover', index: 1 }, 'standard']),
        'actual artwork submits only the exact scoped single-row action');
      assert(ordinary.playId !== first.playId && ordinary.entryId !== first.entryId &&
        after.queue.entries[0].entryId === baseline.queue.entries[0].entryId &&
        after.queue.entries[2].entryId === baseline.queue.entries[1].entryId &&
        equal(after.queue.entries.map((entry) => [entry.id, entry.voiceId]), [['1601','601'], ['1601',null], ['1602','602']]),
      'matching song and episode stay distinct; cover inserted one item and started only that item');
    } else {
      await click(() => row(0), 'single-click actual song row');
      await wait(() => selected(0), 'single click visibly selects the first row');
      const selectedStage = await capture('selected_only');
      assert(!plays.length && equal(selectedStage.player, baseline.player) && equal(selectedStage.queue, baseline.queue),
        'row selection sent no playback and kept native queue/clock');
      assert(disabled(art(2)), 'hard-denied artwork visibly unavailable');
      dbl(2);
      art(2).click();
      await context(2);
      assert(disabled(next()) && disabled(menuPlay()), 'copyright-disabled context play and next');
      next().click();
      document.activeElement?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
      await wait(() => !visible(next()), 'dismiss first copyright menu');
      await context(3);
      assert(oldPage.availability[3].onlineEligible && oldPage.availability[3].copyrightBlocked &&
        disabled(next()) && disabled(menuPlay()),
        'both context actions are disabled by the copyright status independently of online eligibility');
      menuPlay().click();
      next().click();
      const blocked = await capture('blocked');
      assert(!plays.length && equal(blocked.player, baseline.player) && equal(blocked.queue, baseline.queue),
        'denied row gestures sent no playback');
      document.activeElement?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
      await wait(() => !visible(next()), 'dismiss disabled context menu before the heart');

      await wait(() => pressed(0) === 'false', 'actual song heart is initially unliked');
      await click(() => heart(0), 'click native-guarded heart on first ordinary song');
      await wait(() => observedLike(0, true, oldPage.receipt, first.playId), 'confirmed synthetic like returned and pressed');
      const liked = await capture('liked', { pressed: pressed(0) });
      await click(() => heart(0), 'revert the same synthetic song like');
      await wait(() => observedLike(1, false, oldPage.receipt, first.playId), 'confirmed synthetic unlike returned and unpressed');
      const unliked = await capture('unliked', { pressed: pressed(0) });
      assert([liked, unliked].every((item) => equal(item.player, baseline.player) && equal(item.queue, baseline.queue)),
        'likes never change decoder, clock, or queue');

      await click(() => label('播放'), 'resume first decoder before next');
      await waitPlaying('voice', '601');
      await click(() => label('下一首'), 'actual next steps to second episode before using old row');
      const second = await waitPlaying('voice', '602');
      const newPage = await wait(() => expectedRows(['1811','1810']) && reads[1]?.result, 'new current episode has distinct included songs');
      assert(newPage.playId === second.playId && newPage.voiceId === '602' && newPage.receipt !== oldPage.receipt,
        'native changed page ownership after episode change');
      const secondPaused = await park(second.playId);
      const oldScope = { epoch, playId: first.playId, receipt: oldPage.receipt };
      reject = {
        play: await stale('player_voice_song_play', { ...oldScope, action: { kind: 'cover', index: 0 }, quality: 'standard' }),
        like: await stale('player_voice_song_like', { ...oldScope, index: 0, liked: true }),
      };
      assert(reject.play === 'stale' && reject.like === 'stale', 'old song row play and heart rejected by native');
      const retired = await capture('stale_rejected', { reject });
      assert(equal(retired.player, stateView(secondPaused)) && !plays.length && likes.length === 2, 'stale row did not act');

      await context(0);
      await click(next, 'choose actual context-menu 下一首播放');
      await wait(async () => plays[0]?.result && equal(trackIds(queueView(await queue())), ['1601','1602','1811']),
        'context next inserts only selected row immediately after current episode');
      const nextStage = await capture('context_next');
      assert(equal(plays[0].args, [epoch, second.playId, newPage.receipt, { kind: 'next_play', index: 0 }, 'standard']),
        'actual context action is source-bound');
      assert(nextStage.player.state === 'paused' && nextStage.player.playId === second.playId &&
        nextStage.player.entryId === second.entryId && nextStage.player.position === secondPaused.position &&
        nextStage.queue.entries.slice(0,2).every((entry, index) => equal(entry, retired.queue.entries[index])),
      'context next preserves exact paused decoder, clock and old queue entries');

      dbl(1);
      const ordinary = await waitPlaying('catalog', '1810');
      const final = await capture('double_order');
      assert(plays.length === 2 &&
        equal(plays[1].args, [epoch, second.playId, newPage.receipt, { kind: 'double_click', index: 1 }, 'standard']),
      'actual double click used selected ordered index');
      assert(equal(trackIds(final.queue), ['1811','1810']) && final.queue.entries.every((entry) => !entry.voiceId) &&
        final.queue.current === ordinary.entryId && ordinary.playId !== second.playId,
      'default double click replaces with exact visible list and starts the clicked song');
    }
    await emit('complete', { ok: true, mode, count, realMusicRequests: 0, realAccountWrites: 0,
      physicalAudioVerified: false });
  } catch (error) {
    await emit('failed', { ok: false, mode, last, error: String(error), count, reject, reads, plays, likes,
      rows: rows().map((node) => node.textContent.trim()), player: epoch ? await state().then(stateView, () => null) : null });
  } finally { restore?.(); }
})();
