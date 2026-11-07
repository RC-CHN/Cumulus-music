// Real WebView interaction for the purchased SongCard path, with sealed native transfer.
(async () => {
  const fixtureMode = window.__voiceSongDownloadCase;
  const directoryAction = window.__voiceSongPurchasedDirectoryAction;
  const noDirectory = directoryAction === 'cancel' || directoryAction === 'settings';
  const mode = directoryAction === 'settings' ? 'purchased_directory_settings' :
    noDirectory ? 'purchased_no_directory' : fixtureMode;
  const expected = mode === 'purchased_existing' ? 'lossless' : 'exhigh';
  const qualityOrder = ['jymaster', 'hires', 'lossless', 'exhigh', 'standard'];
  const q = (selector, root = document) => root?.querySelector(selector) ?? null;
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])];
  const panel = () => q('.lyrics-panel:not(.lyrics-panel-leave-active)');
  const row = () => q('[data-voice-song-row][data-row-index="0"][data-track-id="1710"]', panel());
  const modal = () => q('dialog[open][data-voice-purchased-download]:not(.modal-leave-active)');
  const notice = () => q('[data-voice-song-download-notice]', panel());
  const picker = () => q('dialog[open][data-voice-download-stage="pick"]:not(.modal-leave-active)');
  const directory = () => q('dialog[open][data-voice-download-stage="directory"]:not(.modal-leave-active)');
  const options = 'dialog.download-dialog:not([data-voice-purchased-download])';
  const settings = () => all(`${options}[open]:not(.modal-leave-active)`).find((node) =>
    q('h2', node)?.textContent.trim() === '音质与下载');
  const directoryInput = () => q('input[aria-label="下载目录"]', settings() ?? null);
  const radios = () => all('[data-voice-purchased-quality]', modal());
  const currentQuality = () => radios().find((node) => node.checked)?.dataset.voicePurchasedQuality;
  const button = (name, root = document) => q(`button[aria-label="${name}"]`, root);
  const textButton = (name, root) => all('button', root).find((node) => node.textContent.trim() === name);
  const visible = (node) => !!node?.getClientRects().length && !node.closest('[inert]');
  const enabled = (node) => visible(node) && !node.disabled && node.getAttribute('aria-disabled') !== 'true';
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, detail = {}) => invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...detail } });
  const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const frame = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const assert = (ok, reason) => { if (!ok) throw Error(reason); };
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const preferences = { quality: 'standard', tags: false, cover: false, lyrics: false, container: 'audio' };
  const calls = [];
  let epoch, count = 0, nonce = 0, last, restore, watcher, baseline, unexpectedOptions = false,
    settingsRequested = false;

  const state = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  const stateView = (value) => ({
    kind: value.resourceKind, trackId: value.track?.id, voiceId: value.track?.voiceId,
    playId: value.playId, entryId: value.entryId, state: value.state, position: value.position,
    seeking: value.nativeSeeking, error: value.error,
  });
  async function downloadViews() {
    const query = { category: 'all', keyword: '', sort: 'completed', descending: true, offset: 0, limit: 10 };
    const active = await invoke('downloads', { epoch, query: { ...query, tab: 'active' } });
    const done = await invoke('downloads', { epoch, query: { ...query, tab: 'completed' } });
    assert(active.epoch === epoch && done.epoch === epoch, 'download snapshots belong to synthetic account');
    const task = (row) => ({ id: row.id, trackId: row.track.id, source: row.source, status: row.status,
      quality: row.actualQuality, requestedQuality: row.requestedQuality, bytes: row.bytes,
      path: row.path, encrypted: row.encrypted });
    assert(same(active.directory, done.directory), 'both task views agree on configured directory');
    return { active: active.tasks.map(task), done: done.tasks.map(task), preferences: done.options,
      directory: done.directory };
  }
  async function wait(predicate, message) {
    last = message;
    const deadline = performance.now() + 18000;
    while (performance.now() < deadline) {
      const value = await predicate();
      if (value) return value;
      await delay(55);
    }
    throw Error(message);
  }
  async function click(find, message) {
    const node = await wait(() => enabled(find()) && find(), message);
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    node.click();
  }
  async function nativeTrace() {
    const id = ++nonce;
    await emit('voice_song_download_poll', { nonce: id });
    return wait(() => window.__voiceSongDownloadNative?.[0] === id && window.__voiceSongDownloadNative[1],
      'native sealed transport trace recorded');
  }
  function action(kind) {
    return calls.find((call) => call.name === 'playerVoiceSongPurchasedDownload' && call.args[1]?.kind === kind);
  }
  function unexpectedDownloadDialog(node) {
    return node instanceof Element && [node.matches(options) ? node : null, ...all(options, node)]
      .some((dialog) => dialog && !(settingsRequested && q('h2', dialog)?.textContent.trim() === '音质与下载'));
  }
  function dom() {
    return {
      title: q('h2', modal())?.textContent.trim() ?? null,
      qualityModal: visible(modal()), qualityRows: radios().map((node) => node.dataset.voicePurchasedQuality),
      quality: currentQuality() ?? null, picker: visible(picker()),
      pickerRows: all('.collect-row', picker()).map((node) => node.getAttribute('aria-label')),
      directory: visible(directory()), directoryTitle: q('h2', directory())?.textContent.trim() ?? null,
      directoryButtons: all('footer button', directory()).map((node) => node.textContent.trim()),
      optionsEver: unexpectedOptions || unexpectedDownloadDialog(document.body), downloads: visible(q('.downloads-page')),
      settings: visible(settings()), directoryInput: directoryInput()?.value ?? null,
      chooserButton: enabled(textButton('选择目录', settings() ?? null)),
      saveDisabled: textButton('保存设置', settings() ?? null)?.disabled ?? null,
      settingsFocus: !!settings()?.contains(document.activeElement),
      vinyl: visible(panel()), notice: notice()?.textContent.trim() ?? null, focusedRow: document.activeElement?.getAttribute('data-row-index') ?? null,
    };
  }
  async function capture(name) {
    await wait(() => !q('dialog.modal-enter-active,dialog.modal-leave-active,.lyrics-panel-enter-active,.lyrics-panel-leave-active,.page-enter-active,.page-leave-active'),
      'page and modal transitions settle');
    await frame();
    const player = stateView(await state());
    const nativeQueue = await queue();
    const records = await downloadViews();
    if (!baseline) baseline = { player, queue: nativeQueue };
    assert(same({ player, queue: nativeQueue }, baseline), 'original paused episode and queue unchanged');
    assert(same(records.preferences, preferences), 'saved default options remain unchanged');
    if (noDirectory) assert(records.directory === null, 'no download directory was configured');
    if (name !== 'transferred') assert(!records.active.length && !records.done.length, 'no download task before accepted start');
    assert(!dom().optionsEver, 'purchased flow never opened a second options dialog');
    const id = `ux_${String(++count).padStart(2, '0')}_${name}`;
    window.__podcastProgressStage = '';
    await emit(id, { mode, actual: document.visibilityState === 'visible', player, queue: nativeQueue, records,
      dom: dom(), commands: structuredClone(calls) });
    await wait(() => window.__podcastProgressStage === id, 'native wrote stage before next action');
  }
  function openSongMenu() {
    const item = row();
    assert(visible(item), 'current displayed SongCard row exists');
    item.scrollIntoView({ block: 'nearest' });
    item.focus({ preventScroll: true });
    const box = item.getBoundingClientRect();
    item.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2,
      clientX: box.left + Math.min(40, box.width / 2), clientY: box.top + box.height / 2 }));
  }
  try {
    assert(['purchased_liked', 'purchased_cancel', 'purchased_existing'].includes(fixtureMode) &&
      (!noDirectory || fixtureMode === 'purchased_liked'), 'exact purchased case');
    const root = window.__voiceSongDownloadRoot;
    assert(typeof root === 'string', 'private synthetic download directory');
    await wait(async () => {
      const account = await invoke('account_snapshot');
      if (account.status !== 'authenticated' || !enabled(button('打开设置'))) return false;
      epoch = account.epoch;
      return enabled(q('input[aria-label="搜索音乐"]'));
    }, 'authenticated sealed fixture ready');

    // Establish saved settings with normal local IPC before exercising the
    // actual SongCard UI. They are not part of the tested action trace.
    if (!noDirectory) {
      const configured = await invoke('download_command', { epoch, command: {
        kind: 'configure', root, path_type: 0, file_type: 1,
      } });
      assert(!configured.failed.length, 'private download directory is configured');
    }
    const saved = await invoke('download_quality_preferences', { epoch, refresh: false });
    const savedPreferences = await invoke('download_command', { epoch, command: {
      kind: 'preferences', revision: saved.revision, options: preferences,
    } });
    assert(!savedPreferences.failed.length, 'save exact synthetic default options');

    const provides = q('#app').__vue_app__._context.provides;
    const core = provides[Reflect.ownKeys(provides).find((key) => typeof key === 'symbol' && key.description === 'core-client')];
    const methods = ['playerVoiceSongs', 'playerVoiceSongDownloadPrepare', 'playerVoiceSongPurchasedDownload',
      'playlistCollectPage', 'downloadCommand', 'playerVoiceSongDownloadStart', 'playerVoiceSongDownloadStartFromPlaylist',
      'playerVoiceSongDownloadChoosePlaylist', 'playerVoiceSongDownloadCreatePlaylist', 'pickDownloadDirectory'];
    const originals = methods.map((name) => core[name]);
    assert(originals.every((method) => typeof method === 'function'), 'actual typed adapters are installed');
    methods.forEach((name, index) => {
      core[name] = async (...args) => {
        const call = { name, args: structuredClone(args) };
        calls.push(call);
        try {
          const result = await originals[index].apply(core, args);
          assert(!JSON.stringify(result).includes('SYNTHETIC_INCLUDED_PRIVATE') &&
            !JSON.stringify(result).includes('m1.music.126.net'), 'no private rights or URLs in typed reply');
          call.result = result;
          return result;
        } catch (error) {
          call.error = error?.code ?? String(error);
          throw error;
        }
      };
    });
    restore = () => methods.forEach((name, index) => { core[name] = originals[index]; });
    watcher = new MutationObserver((changes) => {
      if (unexpectedDownloadDialog(document.body) || changes.some((change) =>
        [...change.addedNodes].some(unexpectedDownloadDialog))) unexpectedOptions = true;
    });
    watcher.observe(document.body, { childList: true, subtree: true });

    await click(() => q('input[aria-label="搜索音乐"]'), 'open discovery');
    await click(() => button('播客榜：更多', q('#search-discovery-panel:not(.menu-leave-active)')), 'open classic podcasts');
    await wait(() => q('.podcast-discovery .podcast-top[data-layout="classic"]') &&
      q('[data-podcast-scene-fm] [data-scene-fm-title]'), 'native Scene FM ready');
    await click(() => button('播放随心听FM', q('[data-podcast-scene-fm]')), 'start the actual episode');
    const playing = await wait(async () => {
      const value = await state();
      return value.track?.voiceId === '601' && value.state === 'playing' &&
        value.position > .2 && !value.nativeSeeking && !value.error && value;
    }, 'actual libmpv episode advances');
    await click(() => button('暂停'), 'park the actual episode decoder');
    let position;
    await wait(async () => {
      const value = await state();
      if (value.state !== 'paused' || value.playId !== playing.playId || value.nativeSeeking ||
          value.error || !(value.position > .2)) return false;
      const settled = value.position === position;
      position = value.position;
      return settled;
    }, 'current episode clock fully settles paused');
    await click(() => button('打开播放详情'), 'open current episode vinyl');
    const card = await wait(() => {
      const page = calls.find((call) => call.name === 'playerVoiceSongs')?.result;
      return page?.receipt && page.playId === playing.playId && page.voiceId === '601' &&
        page.tracks[0]?.id === '1710' && visible(row()) && page;
    }, 'purchased row from verified current SongCard');
    const liked = fixtureMode === 'purchased_liked';
    await wait(async () => (await nativeTrace()).likedReads > 0 && q('[data-voice-song-heart]', row())
      ?.getAttribute('aria-pressed') === String(liked), 'current account liked state ready');
    await capture('song_card');

    openSongMenu();
    await click(() => q('[data-menu-key="download"]'), 'real SongCard context Download action');
    const prepared = await wait(() => action('prepare')?.result, 'native purchased offer read');
    assert(prepared.kind === 'plan' && prepared.plan.decision === 'purchased_quality_required' &&
      prepared.plan.trackId === '1710' && prepared.plan.epoch === epoch &&
      prepared.plan.playId === playing.playId && same(prepared.plan.purchasedQualities, qualityOrder),
      'native exact purchased offer, no raw privilege data');
    await wait(() => visible(modal()) && q('h2', modal())?.textContent.trim() === '选择下载音质' &&
      same(radios().map((node) => node.dataset.voicePurchasedQuality), qualityOrder) &&
      currentQuality() === qualityOrder[0] && enabled(textButton('确定', modal())),
      'official quality order and first option preselected');
    assert(!action('select'), 'merely opening quality never selects natively');
    await capture('quality');

    const chosen = q(`input[type="radio"][data-voice-purchased-quality="${expected}"]`, modal());
    assert(chosen && !chosen.disabled, 'actual selectable purchased radio');
    chosen.click();
    await wait(() => currentQuality() === expected, 'explicit local radio choice');
    assert(!action('select') && !action('start'), 'local radio change never dispatches');
    await capture('chosen');
    if (mode === 'purchased_cancel') {
      await click(() => button('关闭音质选择', modal()), 'dismiss official quality choice');
      await wait(() => !modal() && !picker() && document.activeElement === row(),
        'quality dismisses without entering picker and restores source focus');
      await capture('cancelled');
    } else {
      await click(() => textButton('确定', modal()), 'explicit confirm sends the chosen native quality');
      const selected = await wait(() => action('select')?.result, 'native select reply recorded');
      assert(selected.kind === 'selection' && selected.quality === expected && selected.selection &&
        selected.decision === (liked ? 'ready' : 'playlist_required'), 'native confirms exact immutable quality');
      if (!liked) {
        await wait(() => visible(picker()) && enabled(q('[data-playlist-id="81"]', picker())) &&
          !q('[data-playlist-id="82"],[data-playlist-id="83"]', picker()),
          'after Confirm, destination picker shows only editable non-favorite list');
        assert(!action('start'), 'unliked song cannot start before destination');
        await capture('picker');
        await click(() => q('[data-playlist-id="81"]', picker()), 'explicit owned destination choice');
        const chosen = await wait(() => action('choose_playlist')?.result, 'actual playlist write reconciles');
        assert(chosen.kind === 'picker' && chosen.picker.phase === 'confirmed' && chosen.picker.continuation,
          'native destination returns matching continuation after readback');
      }
      const accepted = await wait(() => action('start')?.result, 'native purchased download start reply');
      if (noDirectory) {
        assert(accepted.kind === 'directory_required', 'native rejects missing configured directory');
        await wait(() => visible(panel()) && !modal() && !picker() && !q('.downloads-page') && !notice() &&
          visible(directory()) && q('h2', directory())?.textContent.trim() === '更改下载目录' &&
          enabled(textButton('取消下载', directory())) && enabled(textButton('更改目录', directory())),
          'missing directory asks for an explicit decision without navigating');
        await capture('directory');
        if (directoryAction === 'settings') {
          settingsRequested = true;
          await click(() => textButton('更改目录', directory()), 'explicitly request download settings');
          await wait(() => {
            assert(!calls.some((call) => call.name === 'pickDownloadDirectory'), 'no native chooser opened automatically');
            return action('cancel')?.result?.kind === 'cancelled' && !directory() && !visible(panel()) &&
              visible(q('.downloads-page')) && visible(settings()) && directoryInput()?.value === '' &&
              enabled(textButton('选择目录', settings())) && textButton('保存设置', settings())?.disabled &&
              settings()?.contains(document.activeElement);
          }, 'explicit change opens focused download settings with no configured directory');
          await capture('settings');
          await click(() => button('关闭下载对话框', settings() ?? null), 'close settings without selecting or saving');
          await wait(() => !settings() && visible(q('.downloads-page')) && !visible(panel()),
            'closing settings leaves download management without re-opening the episode');
          await capture('settings_closed');
        } else {
          await click(() => textButton('取消下载', directory()), 'explicitly cancel missing-directory download');
          await wait(() => action('cancel')?.result?.kind === 'cancelled' && !directory() &&
            document.activeElement === row(), 'native selection is cancelled and source focus restored');
          await capture('directory_cancelled');
        }
      } else {
        assert(accepted.kind === 'started' && accepted.result.accepted.length === 1 &&
          !accepted.result.failed.length, 'exact task admitted');
        await wait(() => visible(panel()) && !modal() && !picker() && !q('.downloads-page') &&
          visible(notice()) && notice().textContent.trim() === '已添加到下载',
          'purchased task queues in place with visible episode feedback');
        await wait(async () => {
          const tasks = await downloadViews();
          return !tasks.active.length && tasks.done.length === 1 &&
            tasks.done[0].id === accepted.result.accepted[0] && tasks.done[0].status === 'complete' && tasks.done[0];
        }, 'real downloaded file reaches complete');
        await capture('transferred');
      }
    }
    restore();
    watcher.disconnect();
    await emit('complete', { ok: true, count, mode, realMusicRequests: 0, realAccountWrites: 0, physicalAudioVerified: false });
  } catch (error) {
    restore?.();
    watcher?.disconnect();
    await emit('failed', { ok: false, mode, last, count, error: String(error), dom: dom(),
      commands: calls, text: (document.querySelector('main')?.innerText ?? '').slice(0, 400) });
  }
})();
