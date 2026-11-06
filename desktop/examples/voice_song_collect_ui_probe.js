// Sealed WebView -> native SongCard collection; selected rows use trusted XTest.
(async () => {
  const q = (selector, root = document) => root?.querySelector(selector) ?? null;
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])];
  const panel = () => q('.lyrics-panel:not(.lyrics-panel-leave-active)');
  const shelf = () => q('[data-voice-song-card]', panel());
  const row = (index) => q(`[data-voice-song-row][data-row-index="${index}"]`, shelf());
  const title = (index) => q('.voice-song-title', row(index));
  const dialog = () => q('dialog[open][data-voice-song-collect]:not(.modal-leave-active)');
  const label = (name, root = document) => q(`button[aria-label="${name}"]`, root);
  const textButton = (name, root) => all('button', root).find((node) => node.textContent.trim() === name);
  const menu = () => q('[data-menu-key="collect"]');
  const playlist = () => q('[data-playlist-id="81"]', dialog());
  const selected = () => all('[data-voice-song-row][aria-selected="true"]', shelf())
    .map((item) => Number(item.dataset.rowIndex));
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, detail = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...detail },
  });
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const clone = (value) => value === undefined ? null : JSON.parse(JSON.stringify(value));
  const enabled = (node) => !!node?.getClientRects().length && !node.closest('[inert]') &&
    !node.disabled && node.getAttribute('aria-disabled') !== 'true';
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const frame = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const assert = (ok, message) => { if (!ok) throw Error(message); };
  const commands = [], events = [];
  let epoch, last, baseline, restore, count = 0;

  const hit = (target) => {
    if (!(target instanceof Element)) return null;
    if (target.closest('[data-menu-key="collect"]')) return 'menu:collect';
    const index = target.closest('[data-voice-song-row]')?.dataset.rowIndex;
    return index === undefined ? null : `row:${index}`;
  };
  const observe = (event) => {
    const target = hit(event.target);
    if (target && events.length < 64) {
      events.push({ target, type: event.type, trusted: event.isTrusted,
        button: event.button, ctrl: event.ctrlKey, shift: event.shiftKey });
    }
  };
  const observedTypes = ['click', 'contextmenu'];
  for (const type of observedTypes) document.addEventListener(type, observe, true);

  const state = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  const native = (value) => ({
    kind: value.resourceKind, trackId: value.track?.id, voiceId: value.track?.voiceId,
    playId: value.playId, entryId: value.entryId, state: value.state, position: value.position,
    queueRevision: value.queueRevision, seeking: value.nativeSeeking, error: value.error,
  });
  const results = (name) => commands.filter((command) => command.name === name).map((command) => command.result);
  const planName = 'playerVoiceSongsCollectPrepare';
  const existingName = 'playerVoiceSongsCollectExisting';
  const statusName = 'playerVoiceSongsCollectStatus';
  const destinationsName = 'playerVoiceSongsCollectDestinations';
  const expectedIndices = [1, 2, 3, 0];

  async function wait(check, reason) {
    last = reason;
    const end = performance.now() + 15000;
    while (performance.now() < end) {
      const result = await check();
      if (result) return result;
      await sleep(45);
    }
    throw Error(reason);
  }
  async function click(find, reason) {
    const node = await wait(() => enabled(find()) && find(), reason);
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    node.click();
  }
  async function capture(name, detail = {}) {
    await wait(() => panel() && !q('.lyrics-panel-enter-active,.lyrics-panel-leave-active') &&
      !q('[data-voice-song-collect].modal-enter-active,[data-voice-song-collect].modal-leave-active') &&
      Number(getComputedStyle(panel()).opacity) >= .999, 'settled playback and picker transition');
    await frame();
    const current = { player: native(await state()), queue: await queue() };
    if (!baseline) baseline = current;
    assert(same(current, baseline), 'paused episode clock, decoder and queue stay unchanged');
    const stage = `ux_${String(++count).padStart(2, '0')}_${name}`;
    window.__podcastProgressStage = '';
    await emit(stage, {
      ...current, actual: document.visibilityState === 'visible', selected: selected(),
      focusRow: document.activeElement?.closest('[data-voice-song-row]')?.dataset.rowIndex ?? null,
      dom: { picker: !!dialog(), stage: dialog()?.dataset.voiceSongCollectStage ?? null,
        title: q('h2', dialog())?.textContent.trim() ?? null, summary: q('[data-voice-song-collect-summary]', dialog())?.textContent.trim() ?? null,
        status: q('[data-voice-song-collect-status]', dialog())?.textContent.trim() ?? null,
        playlistIds: all('[data-playlist-id]', dialog()).map((item) => item.dataset.playlistId),
        reconcile: enabled(textButton('核对结果', dialog())) },
      events: clone(events), commands: clone(commands), ...detail,
    });
    await wait(() => window.__podcastProgressStage === stage, 'native stage persisted before next input');
  }
  async function input(name, find, target, { button = 1, ctrl = false } = {}, observed) {
    const node = await wait(() => enabled(find()) && find(), `visible input target ${target}`);
    node.scrollIntoView({ block: 'nearest' });
    await frame();
    const rect = node.getBoundingClientRect();
    const x = Math.floor(rect.left + rect.width / 2);
    const y = Math.floor(rect.top + rect.height / 2);
    assert(hit(document.elementFromPoint(x, y)) === target, `actual visible hit ${target}`);
    const prior = events.length;
    await capture(name, { pointer: { x, y, width: innerWidth, height: innerHeight, ratio: devicePixelRatio },
      input: { target, button, ctrl, shift: false, count: 1 } });
    const expected = { target, type: button === 3 ? 'contextmenu' : 'click', trusted: true,
      button: button === 3 ? 2 : 0, ctrl, shift: false };
    await wait(() => events.slice(prior).some((event) => same(event, expected)), `trusted pointer ${target}`);
    if (observed) await wait(observed, `actual visible result after ${target}`);
  }

  try {
    await wait(async () => {
      const account = await invoke('account_snapshot');
      if (account.status !== 'authenticated' || !enabled(label('打开设置'))) return false;
      epoch = account.epoch;
      return enabled(q('input[aria-label="搜索音乐"]'));
    }, 'synthetic account ready');
    const provides = q('#app').__vue_app__._context.provides;
    const core = provides[Reflect.ownKeys(provides).find((key) => typeof key === 'symbol' && key.description === 'core-client')];
    const names = ['playerVoiceSongs', planName, destinationsName, existingName, statusName,
      'playlistCollectPage', 'mutatePlaylist', 'playerVoiceSongsSelection', 'playerVoiceSongPlay'];
    const originals = names.map((name) => core[name]);
    assert(originals.every((method) => typeof method === 'function'), 'actual typed command adapters');
    for (const [index, name] of names.entries()) {
      core[name] = async (...args) => {
        const command = { name, args: clone(args) };
        commands.push(command);
        try {
          const value = await originals[index].apply(core, args);
          assert(!JSON.stringify(value).includes('SYNTHETIC_INCLUDED_PRIVATE'), 'no private wire fields in projection');
          command.result = clone(value);
          return value;
        } catch (error) {
          command.error = error?.code ?? String(error);
          throw error;
        }
      };
    }
    restore = () => names.forEach((name, index) => { core[name] = originals[index]; });

    await click(() => q('input[aria-label="搜索音乐"]'), 'open real search discovery');
    await click(() => label('播客榜：更多', q('#search-discovery-panel:not(.menu-leave-active)')), 'open Classic podcast');
    await wait(() => q('.podcast-discovery .podcast-top[data-layout="classic"]') &&
      q('[data-podcast-scene-fm] [data-scene-fm-title]'), 'Classic Scene FM ready');
    await click(() => label('播放随心听FM', q('[data-podcast-scene-fm]')), 'start native episode');
    const playing = await wait(async () => {
      const view = await state();
      return view.track?.voiceId === '601' && view.state === 'playing' && view.position > .2 &&
        !view.nativeSeeking && !view.error && view;
    }, 'actual libmpv episode advances');
    await click(() => label('暂停'), 'park real decoder');
    let prior;
    await wait(async () => {
      const view = await state();
      const stable = view.position === prior;
      prior = view.position;
      return stable && view.playId === playing.playId && view.state === 'paused' && !view.nativeSeeking && !view.error;
    }, 'actual episode clock is fully paused');
    await click(() => label('打开播放详情'), 'open real episode vinyl');
    const page = await wait(() => {
      const value = results('playerVoiceSongs')[0];
      return value?.receipt && value.playId === playing.playId && value.voiceId === '601' && row(3) && value;
    }, 'real four-row native SongCard');
    assert(same(page.tracks.map((track) => track.id), ['1710', '1601', '1790', '1791']), 'expected synthetic rows');
    await capture('baseline');
    await input('row_first', () => title(1), 'row:1', {}, () => same(selected(), [1]));
    await input('row_denied', () => title(2), 'row:2', { ctrl: true }, () => same(selected(), [1,2]));
    await input('row_no_play', () => title(3), 'row:3', { ctrl: true }, () => same(selected(), [1,2,3]));
    await input('row_last', () => title(0), 'row:0', { ctrl: true }, () => same(selected(), [0,1,2,3]));

    async function open(attempt) {
      await input(`menu_${attempt}`, () => title(0), 'row:0', { button: 3 }, () => enabled(menu()));
      assert(same(selected(), [0,1,2,3]), 'right click retains frozen selection');
      await input(`collect_${attempt}`, menu, 'menu:collect', {}, () => enabled(playlist()));
      const ready = await wait(() => results(planName)[attempt - 1], 'exact native prepared result');
      const [target, indices] = commands.filter((command) => command.name === planName)[attempt - 1].args;
      assert(target.epoch === epoch && target.playId === playing.playId && target.cardReceipt === page.receipt &&
        same(indices, expectedIndices), 'native prepare sees original selection order, not display order');
      assert(ready.selectedCount === 4 && ready.eligibleCount === 3 && same(ready.omittedIndices, [2]) &&
        !ready.usedFirstFallback && ready.selectionReceipt, 'collection filters subp independent of playback');
      assert(all('[data-playlist-id]', dialog()).length === 1 &&
        q('[data-voice-song-collect-summary]', dialog())?.textContent.trim(), 'owned only and visible group counts');
      return ready;
    }
    const cancelled = await open(1);
    await capture('picker');
    await click(() => label('关闭收藏', dialog()), 'cancel actual collector');
    await wait(() => !dialog() && document.activeElement === row(0), 'cancel returns exact invoking row focus');
    await capture('cancelled');

    const active = await open(2);
    assert(active.selectionReceipt !== cancelled.selectionReceipt, 'new modal has its own selection receipt');
    await click(playlist, 'choose observed regular owned playlist');
    await wait(() => results(existingName)[0]?.phase === 'uncertain' && dialog() &&
      q('[data-voice-song-collect-status]', dialog())?.textContent.trim() &&
      enabled(textButton('核对结果', dialog())), 'actual lost acknowledgement shows readback affordance');
    await capture('pending');
    await click(() => textButton('核对结果', dialog()), 'explicit user readback without resubmission');
    await wait(() => results(statusName)[0]?.phase === 'confirmed' && !dialog() &&
      document.activeElement === row(0), 'confirmed readback closes and restores row focus');
    await capture('reconciled');
    assert(commands.every((command) => !['playlistCollectPage','mutatePlaylist','playerVoiceSongsSelection',
      'playerVoiceSongPlay'].includes(command.name)), 'collector cannot fall back to generic or playback commands');
    await emit('complete', { count, ok: true, realMusicRequests: 0, realAccountWrites: 0, physicalAudioVerified: false });
  } catch (error) {
    await emit('failed', { count, last, error: String(error?.stack ?? error?.message ?? error), commands, events });
  } finally {
    restore?.();
    for (const type of observedTypes) document.removeEventListener(type, observe, true);
  }
})();
