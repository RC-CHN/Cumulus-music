// Sealed WebView SongCard group download; the host sends trusted XTest inputs.
(async () => {
  const mode = window.__voiceSongDownloadCase;
  const purchased = mode === 'group_purchased';
  const tiers = ['sky', 'hires', 'exhigh', 'standard'];
  const q = (selector, root = document) => root?.querySelector(selector) ?? null;
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])];
  const panel = () => q('.lyrics-panel:not(.lyrics-panel-leave-active)');
  const row = (index) => q(`[data-voice-song-row][data-row-index="${index}"]`, panel());
  const title = (index) => q('.voice-song-title', row(index));
  const label = (name, root = document) => q(`button[aria-label="${name}"]`, root);
  const download = () => q('[data-menu-key="download"]');
  const collect = () => q('[data-menu-key="collect"]');
  const notice = () => q('[data-voice-song-group-download-notice]', panel());
  const settings = () => q('dialog[open].download-dialog:not(.modal-leave-active)');
  const action = () => q('[data-voice-song-group-download-action]', notice());
  const quality = () => q('dialog[open][data-voice-purchased-scope="group"]:not(.modal-leave-active)');
  const choice = (tier) => q(`input[data-voice-purchased-quality="${tier}"]`, quality());
  const picked = () => q('input[data-voice-purchased-quality]:checked', quality())?.value ?? null;
  const help = () => label('了解音质', quality());
  const close = () => label('关闭音质选择', quality());
  const confirm = () => q('button[type="submit"]', quality());
  const selected = () => all('[data-voice-song-row][aria-selected="true"]', panel())
    .map((node) => Number(node.dataset.rowIndex));
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, detail = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...detail },
  });
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const clone = (value) => value === undefined ? null : JSON.parse(JSON.stringify(value));
  const visible = (node) => !!node?.getClientRects().length && !node.closest('[inert]');
  const enabled = (node) => visible(node) && !node.disabled && node.getAttribute('aria-disabled') !== 'true';
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const frame = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const assert = (ok, message) => { if (!ok) throw Error(message); };
  const commands = [], events = [];
  let epoch, last, baseline, restore, count = 0;

  const hit = (target) => {
    if (!(target instanceof Element)) return null;
    if (target.closest('[data-menu-key="download"]')) return 'menu:download';
    if (target.closest('[data-voice-song-group-download-action]')) return 'directory:settings';
    if (quality()) {
      if (target.closest('button[aria-label="了解音质"]') === help() && help()) return 'quality:guide';
      if (target.closest('button[aria-label="关闭音质选择"]') === close() && close()) return 'quality:close';
      if (target.closest('button[type="submit"]') === confirm() && confirm()) return 'quality:confirm';
      const tier = target.closest('[data-voice-purchased-quality]')?.dataset.voicePurchasedQuality;
      if (tier) return `quality:${tier}`;
    }
    const index = target.closest('[data-voice-song-row]')?.dataset.rowIndex;
    return index === undefined ? null : `row:${index}`;
  };
  const observe = (event) => {
    const target = hit(event.target);
    if (target && events.length < 64)
      events.push({ target, type: event.type, trusted: event.isTrusted,
        button: event.button, ctrl: event.ctrlKey, shift: event.shiftKey });
  };
  const types = ['click', 'contextmenu'];
  for (const type of types) document.addEventListener(type, observe, true);

  const state = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  const native = (value) => ({
    kind: value.resourceKind, trackId: value.track?.id, voiceId: value.track?.voiceId,
    playId: value.playId, entryId: value.entryId, state: value.state, position: value.position,
    queueRevision: value.queueRevision, seeking: value.nativeSeeking, error: value.error,
  });
  const result = (name) => commands.find((command) => command.name === name)?.result;
  const prepareName = 'playerVoiceSongsDownloadPrepare';
  const startName = 'playerVoiceSongsDownloadStart';
  const guideName = 'openVoiceSongGroupPurchasedQualityGuide';
  const confirmName = 'playerVoiceSongsDownloadConfirmQuality';
  const cancelName = 'playerVoiceSongsDownloadCancel';
  const completed = (name) => commands.find((command) => command.name === name && 'result' in command);

  async function tasks() {
    const query = { category: 'all', keyword: '', sort: 'completed', descending: true, offset: 0, limit: 10 };
    const active = await invoke('downloads', { epoch, query: { ...query, tab: 'active' } });
    const complete = await invoke('downloads', { epoch, query: { ...query, tab: 'completed' } });
    assert(active.epoch === epoch && complete.epoch === epoch, 'native tasks belong to the isolated account');
    const rows = (value) => value.tasks.map((task) => ({ id: task.id, songId: task.track.id,
      source: task.source, status: task.status, quality: task.actualQuality, bytes: task.bytes, encrypted: task.encrypted }));
    return { active: rows(active), complete: rows(complete) };
  }

  async function wait(check, reason) {
    last = reason;
    const end = performance.now() + 18000;
    while (performance.now() < end) {
      const value = await check();
      if (value) return value;
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

  async function capture(name, details = {}) {
    await wait(() => !q('.lyrics-panel-enter-active,.lyrics-panel-leave-active,dialog.modal-enter-active,dialog.modal-leave-active')
      && (!panel() || Number(getComputedStyle(panel()).opacity) >= .999), 'settled panel/settings transition');
    await frame();
    const current = { player: native(await state()), queue: await queue() };
    if (!baseline) baseline = current;
    assert(same(current, baseline), 'same paused episode clock, decoder and queue');
    const stage = `ux_${String(++count).padStart(2, '0')}_${name}`;
    window.__podcastProgressStage = '';
    await emit(stage, { ...current, mode, actual: document.visibilityState === 'visible',
      selected: selected(), tasks: await tasks(),
      dom: { vinyl: visible(panel()), collect: enabled(collect()), download: enabled(download()),
        focusRow: document.activeElement?.closest('[data-voice-song-row]')?.dataset.rowIndex ?? null,
        notice: q('[role="status"]', notice())?.textContent.trim() ?? null,
        noticeKind: notice()?.dataset.kind ?? null,
        action: enabled(action()) ? action().textContent.trim() : null,
        settings: q('h2', settings())?.textContent.trim() ?? null,
        settingsDirectory: visible(q('input[aria-label="下载目录"]', settings())),
        singlePicker: !!q('dialog[open][data-voice-download-picker]'),
        quality: quality() && { heading: q('h2', quality())?.textContent.trim(), selected: picked(),
          tiers: all('input[data-voice-purchased-quality]', quality()).map((node) => node.value),
          confirm: enabled(confirm()) },
        downloads: visible(q('.downloads-page')) },
      commands: clone(commands), events: clone(events), ...details });
    await wait(() => window.__podcastProgressStage === stage, 'native capture acknowledgement');
  }

  async function input(name, find, target, { button = 1, ctrl = false } = {}) {
    const node = await wait(() => enabled(find()) && find(), `visible target ${target}`);
    node.scrollIntoView({ block: 'nearest' });
    await frame();
    const rect = node.getBoundingClientRect();
    const x = Math.floor(rect.left + rect.width / 2);
    const y = Math.floor(rect.top + rect.height / 2);
    assert(hit(document.elementFromPoint(x, y)) === target, `actual hit ${target}`);
    const prior = events.length;
    await capture(name, { pointer: { x, y, width: innerWidth, height: innerHeight, ratio: devicePixelRatio },
      input: { target, button, ctrl, shift: false, count: 1 } });
    const event = { target, type: button === 3 ? 'contextmenu' : 'click', trusted: true,
      button: button === 3 ? 2 : 0, ctrl, shift: false };
    await wait(() => events.slice(prior).some((actual) => same(actual, event)), `trusted input ${target}`);
  }

  try {
    assert(purchased || mode === 'group_liked' || mode === 'group_directory', 'known isolated group mode');
    await wait(async () => {
      const account = await invoke('account_snapshot');
      if (account.status !== 'authenticated' || !enabled(label('打开设置'))) return false;
      epoch = account.epoch;
      return enabled(q('input[aria-label="搜索音乐"]'));
    }, 'synthetic account ready');
    if (mode !== 'group_directory') {
      await invoke('download_command', { epoch, command: {
        kind: 'configure', root: window.__voiceSongDownloadRoot, path_type: 0, file_type: 1,
      } });
      const saved = await invoke('download_quality_preferences', { epoch, refresh: false });
      await invoke('download_command', { epoch, command: {
        kind: 'preferences', revision: saved.revision,
        options: { ...saved.options, quality: 'exhigh', tags: false, cover: false, lyrics: false },
      } });
    }
    const provides = q('#app').__vue_app__._context.provides;
    const core = provides[Reflect.ownKeys(provides).find((key) => typeof key === 'symbol' && key.description === 'core-client')];
    const names = ['playerVoiceSongs', prepareName, startName, cancelName, guideName, confirmName,
      'playerVoiceSongDownloadPrepare', 'playerVoiceSongPurchasedDownload', 'playerVoiceSongsCollectPrepare',
      'pickDownloadDirectory', 'downloadCommand', 'mutatePlaylist', 'playerVoiceSongsSelection'];
    const originals = names.map((name) => core[name]);
    assert(originals.every((method) => typeof method === 'function'), 'actual typed command adapters');
    for (const [index, name] of names.entries()) {
      core[name] = async (...args) => {
        const command = { name, args: clone(args) };
        commands.push(command);
        try {
          const value = await originals[index].apply(core, args);
          command.result = clone(value);
          return value;
        } catch (error) {
          command.error = error?.code ?? String(error);
          throw error;
        }
      };
    }
    restore = () => names.forEach((name, index) => { core[name] = originals[index]; });

    await click(() => q('input[aria-label="搜索音乐"]'), 'open search discovery');
    await click(() => label('播客榜：更多', q('#search-discovery-panel:not(.menu-leave-active)')), 'open Classic podcast');
    await wait(() => q('.podcast-discovery .podcast-top[data-layout="classic"]') &&
      q('[data-podcast-scene-fm] [data-scene-fm-title]'), 'Classic Scene FM ready');
    await click(() => label('播放随心听FM', q('[data-podcast-scene-fm]')), 'start native episode');
    const playing = await wait(async () => {
      const view = await state();
      return view.track?.voiceId === '601' && view.state === 'playing' && view.position > .2 &&
        !view.nativeSeeking && !view.error && view;
    }, 'actual libmpv episode advances');
    await click(() => label('暂停'), 'park native episode');
    let prior;
    await wait(async () => {
      const view = await state();
      const stable = view.position === prior;
      prior = view.position;
      return stable && view.playId === playing.playId && view.state === 'paused' && !view.nativeSeeking && !view.error;
    }, 'actual episode is paused');
    await click(() => label('打开播放详情'), 'open episode vinyl');
    const card = await wait(() => {
      const value = result('playerVoiceSongs');
      return value?.receipt && value.playId === playing.playId && value.voiceId === '601' && row(3) && value;
    }, 'native SongCard projected');
    assert(same(card.tracks.map((item) => item.id), ['1710', '1712', '1790', '1791']) &&
      same(card.availability.map((item) => item.groupDownload),
        purchased ? ['ordinary', 'purchased', 'omitted', 'purchased'] :
          ['ordinary', 'ordinary', 'omitted', 'unsupported']),
      'native safe group hints');

    await capture('baseline');
    await input('select_second', () => title(1), 'row:1');
    await wait(() => same(selected(), [1]), 'second row selected first');
    await input('select_first', () => title(0), 'row:0', { ctrl: true });
    await wait(() => same(selected(), [0,1]), 'first row joins in second place');
    await input('select_anchor', () => title(2), 'row:2', { ctrl: true });
    await wait(() => same(selected(), [0,1,2]), 'copyright-denied liked trigger is in the group');
    if (purchased) await input('select_purchased', () => title(3), 'row:3', { ctrl: true });
    const display = purchased ? [0,1,2,3] : [0,1,2];
    await wait(() => same(selected(), display), 'complete selected group');
    await input('open_unliked', () => title(0), 'row:0', { button: 3 });
    await wait(() => enabled(collect()) && !download(), 'unliked trigger keeps Collect and hides group Download');
    await capture('unliked_hidden');
    document.activeElement.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await wait(() => !visible(collect()), 'context menu closed without an action');

    await input('open_liked', () => title(2), 'row:2', { button: 3 });
    await wait(() => enabled(download()) && enabled(collect()), 'liked filtered trigger offers actual group download');
    assert(same(selected(), display) && !result(prepareName), 'opening menu does not prepare or mutate');
    await input('click_download', download, 'menu:download');

    const plan = await wait(() => result(prepareName), 'actual native group prepare result');
    const prepared = { epoch, playId: playing.playId, cardReceipt: card.receipt, anchorIndex: 2,
      indices: purchased ? [1,0,2,3] : [1,0,2] };
    assert(same(commands.find((item) => item.name === prepareName)?.args, [prepared]) &&
      same(plan.eligibleIndices, purchased ? [1,0,3] : [1,0]) &&
      same(plan.omittedIndices, [2]), 'exact ordered preparation and filtered trigger');
    assert(!settings() && !q('dialog[open][data-voice-download-picker]') && (purchased || !quality()),
      'no automatic settings or picker');
    if (purchased) {
      const exactChooser = () => quality() && q('h2', quality())?.textContent.trim() === '选择下载音质' &&
        same(all('input[data-voice-purchased-quality]', quality()).map((node) => node.value), tiers) &&
        picked() === 'sky' && enabled(confirm());
      const target = ({ selectionReceipt }) => ({ epoch, playId: playing.playId, cardReceipt: card.receipt, selectionReceipt });
      assert(plan.decision === 'purchased_quality_required' && plan.selectionReceipt &&
        same(plan.purchasedQualities, tiers), 'verified descending union of the two purchases');
      await wait(exactChooser, 'actual shared modal initially selects highest without confirming');
      assert(!result(startName) && !completed(confirmName), 'no implicit confirmation');
      await input('open_guide', help, 'quality:guide');
      await wait(() => completed(guideName) && picked() === 'sky' && enabled(help()),
        'fixed browser handoff keeps quality selection');
      await input('close_chooser', close, 'quality:close');
      await wait(() => completed(cancelName) && !quality() && document.activeElement === row(2),
        'actual cancel restores exact invoking row');
      await capture('cancelled');
      assert(same(completed(guideName).args, [target(plan)]) &&
        same(completed(cancelName).args, [target(plan)]), 'guide and cancel use only first group receipt');
      await input('reopen_menu', () => title(2), 'row:2', { button: 3 });
      await input('reopen_chooser', download, 'menu:download');
      const again = await wait(() => {
        const next = commands.filter((item) => item.name === prepareName)[1];
        return next?.result && exactChooser() && next;
      }, 'new chooser opens with highest preselected again');
      assert(again.result.selectionReceipt !== plan.selectionReceipt && same(again.args, [prepared]) &&
        same(again.result.purchasedQualities, tiers), 'different native receipt, same frozen selection');
      await input('choose_hires', () => choice('hires'), 'quality:hires');
      await wait(() => picked() === 'hires' && !completed(confirmName), 'radio choice remains local');
      await input('confirm_hires', confirm, 'quality:confirm');
      const started = await wait(() => !quality() && notice()?.dataset.kind === 'queued' && completed(confirmName),
        'one native confirmation closes the chooser and reports in place');
      assert(started.result.kind === 'started' && same(started.args, [target(again.result), 'hires']) &&
        started.result.result.accepted.length === 3 &&
        started.result.result.operationId === again.result.selectionReceipt,
        'one explicit selected-tier confirmation accepts the frozen mixed batch');
      await capture('queued');
      await wait(async () => {
        const view = await tasks();
        return view.active.length === 0 && view.complete.length === 3 &&
          view.complete.every((task) => task.status === 'complete' && task.source === 'song' && task.quality === 'standard');
      }, 'all three sealed MP3 tasks publish');
      const saved = await invoke('download_quality_preferences', { epoch, refresh: false });
      assert(saved.options.quality === 'exhigh', 'single group tier does not change saved preference');
      await capture('transferred', { savedQuality: saved.options.quality });
    } else if (mode === 'group_liked') {
      const started = await wait(() => notice()?.dataset.kind === 'queued' && result(startName),
        'accepted batch reports in place');
      assert(plan.decision === 'ready' && started.accepted.length === 2 &&
        started.operationId === plan.selectionReceipt &&
        same(commands.find((item) => item.name === startName)?.args,
          [{ epoch, playId: playing.playId, cardReceipt: card.receipt, selectionReceipt: plan.selectionReceipt }]),
        'exact one receipt-only start');
      assert(q('[role="status"]', notice()).textContent.trim() === '已添加到下载列表（2 首，跳过 1 首）' &&
        !q('.downloads-page'), 'visible in-place actual counts and no route change');
      await capture('queued');
      await wait(async () => {
        const view = await tasks();
        return view.active.length === 0 && view.complete.length === 2 &&
          view.complete.every((task) => task.status === 'complete' && task.source === 'song' && task.quality === 'standard');
      }, 'both synthetic MP3 tasks publish');
      await capture('transferred');
    } else {
      await wait(() => notice()?.dataset.kind === 'directory' && enabled(action()), 'directory notice and explicit action');
      assert(plan.decision === 'directory_required' && plan.selectionReceipt === null && !result(startName),
        'missing directory prevents start before any chooser');
      await capture('missing_directory');
      await input('open_settings', action, 'directory:settings');
      await wait(() => !panel() && q('h2', settings())?.textContent.trim() === '音质与下载' &&
        visible(q('input[aria-label="下载目录"]', settings())), 'explicit action opens existing Cumulus settings');
      await capture('settings');
    }
    const allowed = purchased ? ['playerVoiceSongs', prepareName, guideName, cancelName, confirmName] :
      ['playerVoiceSongs', prepareName, startName];
    assert(commands.every((item) => allowed.includes(item.name)),
      'no single/collect/generic write or native picker');
    await emit('complete', { ok: true, mode, count, realMusicRequests: 0,
      realAccountWrites: 0, physicalAudioVerified: false });
  } catch (error) {
    await emit('failed', { error: String(error), last, count, commands, events });
  } finally {
    restore?.();
    for (const type of types) document.removeEventListener(type, observe, true);
  }
})();
