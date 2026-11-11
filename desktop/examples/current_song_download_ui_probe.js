// Real production PlayerBar controls with sealed media, account and playlist transport.
(async () => {
  const mode = window.__currentSongDownloadMode;
  const directory = window.__currentSongDownloadDirectory;
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, fields = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...fields } });
  const one = (selector, root = document) => root?.querySelector(selector);
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])];
  const bar = () => one('.playerbar');
  const badge = () => one('.player-current-download', bar());
  const modal = (selector) => one(`dialog[open]${selector}:not(.modal-leave-active)`);
  const quality = () => modal('[data-voice-purchased-download]');
  const picker = () => modal('[data-search-group-download-picker]');
  const download = () => modal('.download-dialog');
  const downloadSettings = () => one('section[aria-labelledby="settings-download"]');
  const notice = () => one('.player-download-notice', bar());
  const byLabel = (label, root) => one(`[aria-label="${label}"]`, root);
  const byText = (text, root) => all('button', root).find((node) => node.textContent.trim() === text);
  const enabled = (node) =>
    !!node?.getClientRects().length && !node.disabled &&
    node.getAttribute('aria-disabled') !== 'true' && !node.closest('[inert]');
  function renderedSettings() {
    const section = downloadSettings();
    const heading = one('#settings-download', section);
    const edit = byText('更改目录', section);
    const visible = (node) => {
      if (!enabled(node)) return false;
      const rect = node.getBoundingClientRect();
      return node.contains(document.elementFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2));
    };
    if (!visible(heading) || !visible(edit)) return false;
    for (let node = section; node; node = node.parentElement) {
      if (node.matches('.page-enter-active,.page-leave-active') || getComputedStyle(node).opacity !== '1')
        return false;
    }
    return true;
  }
  const assert = (value, message) => { if (!value) throw Error(message); };
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  let epoch, checkpoint = 0, last, incumbent;
  const actions = [], legacyWrites = [];
  const preferences = { quality: 'standard', tags: false, cover: false, lyrics: false, container: 'audio' };
  const expectedTiers = ['hires', 'lossless', 'exhigh', 'standard'];

  async function wait(check, message) {
    last = message;
    const deadline = performance.now() + 16000;
    while (performance.now() < deadline) {
      const value = await check();
      if (value) return value;
      await sleep(50);
    }
    throw Error(message);
  }
  async function click(get, message) {
    const node = await wait(() => {
      const candidate = get();
      return enabled(candidate) && candidate;
    }, message);
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    node.click();
  }
  const nativePlayer = () => invoke('player_snapshot', { epoch });
  const nativeQueue = () => invoke('player_queue', { epoch });
  const command = (command) => invoke('download_command', { epoch, command });
  async function capture(label) {
    await wait(() => !one('dialog.modal-enter-active,dialog.modal-leave-active'), 'settled dialog');
    const [player, queue] = await Promise.all([nativePlayer(), nativeQueue()]);
    const current = { epoch: player.epoch, playId: player.playId, entryId: player.entryId,
      trackId: player.track?.id, state: player.state, position: player.position, kind: player.resourceKind,
      queueRevision: player.queueRevision, queueCurrent: queue.current,
      queue: queue.tracks.map((row) => [row.entryId, row.track.id]) };
    if (incumbent) assert(JSON.stringify(current) === JSON.stringify(incumbent), 'purchase flow preserves paused player and queue');
    const id = `ux_${String(++checkpoint).padStart(2, '0')}_${label}`;
    await emit(id, { mode, current, visible: document.visibilityState === 'visible',
      dom: { quality: !!quality(), ordinary: !!download(), picker: !!picker(),
        status: badge()?.dataset.downloadState, focus: document.activeElement === badge(),
        settings: label === 'settings' && renderedSettings(),
        settingsFocus: document.activeElement === one('#settings-download', downloadSettings() ?? null),
        settingsDirectoryAction: enabled(byText('更改目录', downloadSettings() ?? null)),
        tiers: all('[data-voice-purchased-quality]', quality() ?? null).map((row) => row.value),
        selected: one('[data-voice-purchased-quality]:checked', quality() ?? null)?.value ?? null,
        destinations: all('[data-playlist-id]', picker() ?? null).map((row) => row.dataset.playlistId),
        readback: enabled(one('[data-search-group-reconcile]', picker() ?? null)),
        notice: notice()?.textContent.trim() ?? null },
      actions: structuredClone(actions), legacyWrites: [...legacyWrites] });
    return current;
  }
  async function pauseOn(id, playId) {
    await wait(() => bar()?.dataset.trackId === id &&
      (bar()?.dataset.state === 'playing' || (!playId && bar()?.dataset.state === 'paused')) &&
      (!playId || bar().dataset.playId === playId), 'selected catalogue audio really loaded');
    const paused = bar().dataset.state === 'paused'
      ? await nativePlayer()
      : await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    await wait(() => bar()?.dataset.playId === paused.playId && bar()?.dataset.state === 'paused' &&
      badge()?.dataset.downloadState === 'none', 'paused real current player and badge');
    assert(paused.track?.id === id && paused.entryId && paused.playId && paused.timeline?.mode === 'full',
      'ordinary full-scope playback');
    return paused;
  }
  async function playPaid() {
    const played = await invoke('play_selection', { epoch,
      selection: { kind: 'songs', ids: ['2', '3'], index: 0, quality: 'standard' } });
    return pauseOn('2', played.playId);
  }
  async function openPaid() {
    await click(badge, 'visible current-song Download');
    await wait(quality, 'purchased quality from native prepare');
    assert(!download(), 'never use ordinary options for a verified purchase');
    const values = all('[data-voice-purchased-quality]', quality()).map((node) => node.value);
    assert(JSON.stringify(values) === JSON.stringify(expectedTiers), 'native purchased tiers in recovered order');
    assert(one('[data-voice-purchased-quality]:checked', quality())?.value === 'hires', 'highest tier preselected');
  }
  function sessionTaskRows(tab) {
    return invoke('downloads', { epoch, query: { tab, category: tab === 'active' ? 'all' : 'song',
      keyword: '', sort: 'completed', descending: true, offset: 0, limit: 10 } });
  }
  function observeCore() {
    const provides = one('#app').__vue_app__._context.provides;
    const client = provides[Reflect.ownKeys(provides).find((key) => typeof key === 'symbol' && key.description === 'core-client')];
    const native = client.playerCurrentSongDownload;
    const oldDownload = client.downloadCommand;
    const guide = client.openCurrentSongPurchasedQualityGuide;
    assert(typeof native === 'function' && typeof guide === 'function', 'typed production desktop adapter');
    client.playerCurrentSongDownload = async (owner, target, action) => {
      const current = { kind: action.kind, epoch: owner, ...target,
        quality: action.quality ?? action.options?.quality, options: action.options };
      actions.push(current);
      try {
        const reply = await native.call(client, owner, target, action);
        current.result = reply.kind;
        current.decision = reply.decision;
        current.trackId = reply.trackId;
        current.startedCount = reply.result?.accepted?.length;
        assert(!JSON.stringify(reply).includes('m1.music.126.net'), 'no authorized URL enters renderer');
        return reply;
      } catch (issue) {
        current.error = issue?.code ?? String(issue);
        throw issue;
      }
    };
    client.openCurrentSongPurchasedQualityGuide = async (owner, target, offer) => {
      const current = { kind: 'guide', epoch: owner, ...target };
      actions.push(current);
      await guide.call(client, owner, target, offer);
      current.result = 'opened';
    };
    client.downloadCommand = async (...args) => {
      legacyWrites.push(args[1]?.kind);
      return oldDownload.apply(client, args);
    };
  }

  try {
    assert(['liked', 'playlist', 'directory'].includes(mode) && typeof directory === 'string', 'isolated proof mode');
    await wait(async () => {
      const account = await invoke('account_snapshot');
      if (account.status !== 'authenticated' || !enabled(one('input[aria-label="搜索音乐"]'))) return false;
      epoch = account.epoch;
      return true;
    }, 'authenticated synthetic desktop');
    // Read the same synthetic liked endpoint into the real native account
    // journal before opening the chooser; the picker must distinguish known
    // empty membership from unavailable membership.
    const liked = await invoke('liked_ids');
    assert(liked.epoch === epoch && liked.value.includes('2') === (mode !== 'playlist'),
      'exact known account membership for current Song');
    if (mode !== 'directory') {
      const result = await command({ kind: 'configure', root: directory, path_type: 0, file_type: 1 });
      assert(!result.failed.length, 'configured isolated directory');
    }
    const settings = await invoke('download_quality_preferences', { epoch, refresh: false });
    assert(!(await command({ kind: 'preferences', revision: settings.revision, options: preferences })).failed.length,
      'saved exact default download options');
    observeCore();

    await playPaid();
    await openPaid();
    if (mode === 'liked') {
      await click(() => byLabel('了解音质', quality()), 'explicit scoped quality help');
      await wait(() => actions.some((row) => row.kind === 'guide' && row.result === 'opened'), 'fixed native help handoff');
    }
    await capture('purchased');
    await click(() => byLabel('关闭音质选择', quality()), 'close quality before submitting');
    await wait(() => !quality() && document.activeElement === badge() && actions.some((row) => row.kind === 'cancel'),
      'unsubmitted offer retired and original control regains focus');
    await capture('cancelled');

    await openPaid();
    await invoke('player_action', { epoch, action: { kind: 'next' } });
    await pauseOn('3');
    await wait(() => !quality() && !picker() && !download(), 'switching the exact song invalidates old offer');
    assert(!actions.some((row) => row.kind === 'select' || row.kind === 'start'), 'cancel and song change sent no writes');
    await capture('stale');

    await click(badge, 'open actual new ordinary song Download');
    await wait(() => download() && one('h2', download())?.textContent.trim() === '下载 1 首歌曲',
      'verified ordinary source uses existing options');
    assert(!quality() && !picker(), 'ordinary is independent of purchased offer');
    await capture('ordinary');
    await click(() => byLabel('关闭下载对话框', download()), 'cancel ordinary dialog');
    await wait(() => !download() && document.activeElement === badge(), 'ordinary focus restored');

    await playPaid();
    incumbent = await capture('baseline');
    await openPaid();
    await click(() => one('[data-voice-purchased-quality="lossless"]', quality()), 'choose non-default native tier');
    await capture('selected');
    await click(() => byText('确定', quality()), 'explicit purchased quality confirmation');

    if (mode === 'playlist') {
      await wait(() => enabled(one('[data-playlist-id="81"]', picker() ?? null)), 'exact owned native destination listed');
      assert(!one('[data-playlist-id="83"]', picker() ?? null), 'outside playlist omitted');
      assert(!actions.some((row) => row.kind === 'choose_playlist' || row.kind === 'start'), 'opening picker is read only');
      await capture('picker');
      await click(() => one('[data-playlist-id="81"]', picker() ?? null), 'select exact owned playlist');
      await wait(() => enabled(one('[data-search-group-reconcile]', picker() ?? null)) &&
        picker().textContent.includes('加入歌单的结果尚未确认'), 'lost add presents status only');
      assert(!actions.some((row) => row.kind === 'start'), 'uncertain add does not start transfer');
      await capture('uncertain');
      await emit('current-download-show-readback');
      await wait(() => window.__currentSongDownloadReadback === true, 'native readback fixture enabled');
      await click(() => one('[data-search-group-reconcile]', picker() ?? null), 'explicit read-only reconciliation');
    }
    if (mode === 'directory') {
      await wait(() => notice()?.textContent.includes('请先设置下载目录') && !quality(), 'directory missing, no admission');
      await capture('directory');
      await click(() => byText('设置目录', notice()), 'explicit handoff to directory settings');
      await wait(() => document.activeElement === one('#settings-download', downloadSettings() ?? null) &&
        renderedSettings() && !download(),
      'settled and visible download settings with heading focus');
      await capture('settings');
      const edit = byText('更改目录', downloadSettings());
      await click(() => edit, 'open the directory form directly from Settings');
      await wait(() => download() && one('h2', download())?.textContent.trim() === '音质与下载',
        'existing native directory configuration opened');
      const input = one('[aria-label="下载目录"]', download());
      assert(input && !input.value, 'missing directory remains unconfigured');
      await click(() => byLabel('关闭下载对话框', download()), 'leave settings without selecting directory');
      await wait(() => !download() && document.activeElement === edit, 'configuration closes back to Settings');
    } else {
      await wait(async () => badge()?.dataset.downloadState === 'complete' &&
        (await sessionTaskRows('completed')).tasks.some((row) => row.track.id === '2' && row.status === 'complete'),
      'native download published and PlayerBar status refreshed');
      assert(!quality() && !picker() && !download(), 'no third options modal or navigation after transfer');
      await capture('transferred');
    }
    const complete = await sessionTaskRows('completed');
    const active = await sessionTaskRows('active');
    assert(JSON.stringify(complete.options) === JSON.stringify(preferences), 'selected quality did not change saved preferences');
    assert(!active.tasks.length && legacyWrites.length === 0, 'no ordinary download write from paid source');
    const task = ({ id, track, status, source, requestedQuality, actualQuality }) =>
      ({ id, trackId: track.id, status, source, requestedQuality, actualQuality });
    await emit('complete', { ok: true, mode, stageCount: checkpoint, realAccountWrites: 0,
      realMusicRequests: 0, actions, legacyWrites, tasks: complete.tasks.map(task) });
  } catch (error) {
    await emit('failed', { ok: false, mode, stageCount: checkpoint, last, error: String(error),
      stack: error.stack, actions, legacyWrites, active: document.activeElement?.outerHTML.slice(0, 300) });
  }
})();
