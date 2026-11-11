// Real production PlayerBar controls with sealed media, account and playlist transport.
(async () => {
  const mode = window.__currentSongDownloadMode;
  const directory = window.__currentSongDownloadDirectory;
  const directorySetup = window.__currentSongDownloadSetupDirectory === true;
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
  const directoryConfirmation = () => modal('.settings-confirm');
  const settingsDirectoryPath = () =>
    one('[data-download-directory-path]', downloadSettings() ?? null)?.textContent.trim() ?? null;
  const radio = (field, value) => one(`fieldset[data-download-layout-field="${field}"] input[value="${value}"]`, downloadSettings());
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
  const directoryChooserCalls = [], directoryPickerResults = [], directoryPreviewCalls = [], directoryChangeCalls = [];
  const layoutCalls = [];
  let resolveNativeChooser, directoryChooserCancelled = false, directoryRepickCancelled = false;
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
    let paint = {};
    const confirmStage = label === 'directory_confirm' || label === 'directory_same';
    if (label === 'settings' || confirmStage) {
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      assert(confirmStage ? directoryConfirmation() : renderedSettings(), 'settings still visible after paint frames');
      const heading = confirmStage ? one('h2', directoryConfirmation()) : one('#settings-download', downloadSettings());
      const text = document.createRange();
      text.selectNodeContents(heading);
      // Sample only the first two characters of the longer confirmation title.
      if (confirmStage) text.setEnd(heading.firstChild, 2);
      const rect = text.getBoundingClientRect();
      paint = {
        screenX, screenY, ratio: devicePixelRatio,
        paintRegions: [{ name: confirmStage ? 'download-directory-confirmation' : 'download-settings-heading', left: rect.left, top: rect.top,
          width: rect.width, height: rect.height, color: getComputedStyle(heading).color, present: true }],
      };
    }
    const directoryStages = ['directory_confirm', 'directory_saved', 'directory_same', 'directory_repick_cancelled'];
    const directoryEvidence = directoryStages.includes(label)
      ? { directoryAudit: await directoryAudit() } : {};
    const layoutEvidence = label.startsWith('layout_') ? { layoutAudit: {
      native: await invoke('download_layout', { epoch }), calls: structuredClone(layoutCalls),
      path: settingsDirectoryPath(), chooserCount: directoryChooserCalls.length,
      dialog: !!one('dialog[open]:not(.modal-leave-active)'),
      checked: ['path_type', 'file_type'].map((field) =>
        all(`fieldset[data-download-layout-field="${field}"] input:checked`, downloadSettings()).map((node) => Number(node.value))),
    } } : {};
    const [player, queue] = await Promise.all([nativePlayer(), nativeQueue()]);
    const current = { epoch: player.epoch, playId: player.playId, entryId: player.entryId,
      trackId: player.track?.id, state: player.state, position: player.position, kind: player.resourceKind,
      queueRevision: player.queueRevision, queueCurrent: queue.current,
      queue: queue.tracks.map((row) => [row.entryId, row.track.id]) };
    if (incumbent) assert(JSON.stringify(current) === JSON.stringify(incumbent), 'purchase flow preserves paused player and queue');
    const id = `ux_${String(++checkpoint).padStart(2, '0')}_${label}`;
    await emit(id, { mode, current, visible: document.visibilityState === 'visible', ...paint, ...directoryEvidence, ...layoutEvidence,
      dom: { quality: !!quality(), ordinary: !!download(), picker: !!picker(),
        status: badge()?.dataset.downloadState, focus: document.activeElement === badge(),
        settings: label === 'settings' && renderedSettings(),
        settingsFocus: document.activeElement === one('#settings-download', downloadSettings() ?? null),
        settingsDirectoryAction: enabled(byText('更改目录', downloadSettings() ?? null)),
        settingsDirectoryPath: settingsDirectoryPath(),
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
  async function directoryAudit() {
    const saved = await sessionTaskRows('completed');
    const confirmation = directoryConfirmation();
    return {
      epoch: saved.epoch,
      nativeDirectory: saved.directory,
      migration: saved.migration,
      activeCount: saved.activeCount,
      completedCount: saved.completedCount,
      path: settingsDirectoryPath(),
      focus: document.activeElement === byText('更改目录', downloadSettings() ?? null),
      confirmation: confirmation ? {
        title: one('h2', confirmation)?.textContent.trim(),
        message: one('p', confirmation)?.textContent.trim(),
        action: one('.primary', confirmation)?.textContent.trim(),
        ownsFocus: confirmation.contains(document.activeElement),
      } : null,
      chooserCalls: [...directoryChooserCalls],
      pickerResults: [...directoryPickerResults],
      previewCalls: structuredClone(directoryPreviewCalls),
      changeCalls: structuredClone(directoryChangeCalls),
    };
  }
  function observeCore() {
    const provides = one('#app').__vue_app__._context.provides;
    const client = provides[Reflect.ownKeys(provides).find((key) => typeof key === 'symbol' && key.description === 'core-client')];
    const native = client.playerCurrentSongDownload;
    const oldDownload = client.downloadCommand;
    const guide = client.openCurrentSongPurchasedQualityGuide;
    assert(typeof native === 'function' && typeof guide === 'function', 'typed production desktop adapter');
    if (mode === 'directory') {
      const preview = client.downloadDirectoryPreview;
      assert(typeof client.pickDownloadDirectory === 'function' && typeof preview === 'function',
        'typed native directory selection and preview adapters are installed');
      // Replace only the OS picker result. Preview, confirmation and setting
      // persistence continue through the actual typed desktop IPC.
      client.pickDownloadDirectory = (owner) => {
        directoryChooserCalls.push(owner);
        return new Promise((resolve) => {
          resolveNativeChooser = (path) => {
            directoryPickerResults.push(path);
            resolveNativeChooser = undefined;
            resolve(path);
          };
        });
      };
      client.downloadDirectoryPreview = async (owner, root) => {
        const record = { epoch: owner, root };
        directoryPreviewCalls.push(record);
        const result = await preview.call(client, owner, root);
        record.result = result;
        return result;
      };
      const change = client.changeDownloadLayout;
      assert(typeof change === 'function', 'typed local layout mutation is installed');
      client.changeDownloadLayout = async (owner, changeInput) => {
        const record = { epoch: owner, change: structuredClone(changeInput) };
        layoutCalls.push(record);
        const result = await change.call(client, owner, changeInput);
        record.result = result;
        return result;
      };
    }
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
    client.downloadCommand = async (owner, command) => {
      if (mode === 'directory' && directorySetup && command?.kind === 'change_directory') {
        const record = { epoch: owner, command: structuredClone(command) };
        directoryChangeCalls.push(record);
        const result = await oldDownload.call(client, owner, command);
        record.result = result;
        return result;
      }
      legacyWrites.push(command?.kind);
      return oldDownload.call(client, owner, command);
    };
  }

  async function applyLayout() {
    await wait(() => enabled(radio('file_type', 0)) && radio('file_type', 0).checked &&
      radio('path_type', 0)?.checked, 'first account reads official untouched defaults');
    await click(() => radio('file_type', 0), 'reselect existing naming');
    assert(!layoutCalls.length, 'reselecting current naming does not call native mutation');
    await click(() => radio('file_type', 2), 'change only naming in Settings');
    await wait(() => layoutCalls[0]?.result && enabled(radio('file_type', 2)) &&
      radio('file_type', 2).checked && radio('path_type', 0)?.checked &&
      document.activeElement === radio('file_type', 2), 'naming persists and keeps focus with organization intact');
    await capture('layout_naming');
    await click(() => radio('path_type', 1), 'change only organization in Settings');
    await wait(() => layoutCalls[1]?.result && enabled(radio('path_type', 1)) &&
      radio('path_type', 1).checked && radio('file_type', 2)?.checked &&
      document.activeElement === radio('path_type', 1), 'organization persists and keeps focus with naming intact');
    await capture('layout_organization');
    await click(() => byText('音质与下载选项', downloadSettings()), 'open independent quality settings');
    await wait(() => download() && one('[aria-label="下载音质"]', download()), 'real download preferences dialog');
    assert(!one('[aria-label="下载目录"],[aria-label="文件命名"],[aria-label="文件夹组织"]', download()),
      'quality dialog does not duplicate directory and layout controls');
    await capture('layout_quality');
    await click(() => byLabel('关闭下载对话框', download()), 'cancel independent quality settings');
    await wait(() => !download() && !one('dialog.modal-leave-active'), 'layout still displayed after quality Cancel');
  }

  async function applyDirectorySetup(edit) {
    resolveNativeChooser(directory);
    await wait(() => enabled(byText('继续更改', directoryConfirmation() ?? null)),
      'ready native preview opens a confirmation without committing');
    const ready = directoryPreviewCalls[0]?.result;
    assert(directoryPreviewCalls.length === 1 && ready?.decision === 'ready' &&
      typeof ready.receipt === 'string' && ready.receipt.length === 32 && /^[0-9a-f]+$/.test(ready.receipt) &&
      directoryChangeCalls.length === 0 && settingsDirectoryPath() === '未设置',
      'selecting a folder only previews and receives its native receipt');
    await capture('directory_confirm');
    await click(() => byText('继续更改', directoryConfirmation() ?? null), 'explicitly save the initial directory');
    await wait(() => !directoryConfirmation() && !edit.disabled && document.activeElement === edit &&
      settingsDirectoryPath() === directory && downloadSettings()?.textContent.includes('下载目录已更改。'),
      'saved native directory is rendered inline and returns focus');
    assert(directoryChangeCalls.length === 1 && directoryChangeCalls[0].result?.epoch === epoch &&
      directoryChangeCalls[0].command.receipt === ready.receipt,
      'exactly one same-account initial directory command used the preview receipt');
    await capture('directory_saved');

    await click(() => edit, 'pick the current directory again');
    await wait(() => directoryChooserCalls.length === 2 && resolveNativeChooser, 'second native picker request');
    resolveNativeChooser(directory);
    await wait(() => enabled(byText('重新选择', directoryConfirmation() ?? null)),
      'the same directory receives only a repick action');
    assert(directoryPreviewCalls.length === 2 && directoryPreviewCalls[1].result?.decision === 'same_directory' &&
      directoryPreviewCalls[1].result.receipt === null &&
      directoryChangeCalls.length === 1, 'same directory is rejected before changing it');
    await capture('directory_same');
    await click(() => byText('重新选择', directoryConfirmation() ?? null), 'explicitly request a replacement folder');
    await wait(() => directoryChooserCalls.length === 3 && !directoryConfirmation() && resolveNativeChooser,
      'repick waits for the confirmation to close');
    resolveNativeChooser(null);
    await wait(() => !edit.disabled && document.activeElement === edit && !one('dialog[open]'),
      'cancelling the replacement restores the directory button');
    directoryRepickCancelled = true;
    assert(directoryPreviewCalls.length === 2 && directoryChangeCalls.length === 1 &&
      settingsDirectoryPath() === directory, 'replacement cancellation preserves the saved directory');
    await capture('directory_repick_cancelled');
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
      if (directorySetup) await applyLayout();
      const edit = byText('更改目录', downloadSettings());
      assert(settingsDirectoryPath() === '未设置', 'current empty directory is shown inline');
      await click(() => edit, 'open system directory picker directly from Settings');
      await wait(() => directoryChooserCalls.length === 1 && edit.disabled && resolveNativeChooser,
        'one native chooser call, pending control disabled');
      assert(directoryChooserCalls[0] === epoch && !one('dialog[open]'),
        'exact account goes directly to native picker without another Settings dialog');
      edit.click();
      if (directorySetup) await applyDirectorySetup(edit);
      else {
        resolveNativeChooser(null);
        await wait(() => !edit.disabled && document.activeElement === edit && !one('dialog[open]'),
          'native Cancel restores exact directory button focus');
        directoryChooserCancelled = true;
        assert(directoryChooserCalls.length === 1 && !directoryPreviewCalls.length && !legacyWrites.length &&
          settingsDirectoryPath() === '未设置', 'cancel never previews or commits, preserving the inline path');
      }
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
    await emit('complete', { ok: true, mode, directorySetup, stageCount: checkpoint, realAccountWrites: 0,
      realMusicRequests: 0, actions, legacyWrites, tasks: complete.tasks.map(task),
      directoryChooserCalls, directoryPickerResults, directoryPreviewCalls, directoryChangeCalls,
      directoryChooserCancelled, directoryRepickCancelled, nativeDirectory: complete.directory, layoutCalls,
      settingsDirectoryPath: settingsDirectoryPath() });
  } catch (error) {
    await emit('failed', { ok: false, mode, stageCount: checkpoint, last, error: String(error),
      stack: error.stack, actions, legacyWrites, directoryChooserCalls, directoryPickerResults,
      directoryPreviewCalls, directoryChangeCalls, active: document.activeElement?.outerHTML.slice(0, 300) });
  }
})();
