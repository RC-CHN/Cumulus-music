// Isolated real WebView -> typed SongCard commands -> Rust worker.
(async () => {
  const mode = window.__voiceSongDownloadCase;
  const output = window.__voiceSongDownloadRoot;
  const q = (selector, root = document) => root?.querySelector(selector) ?? null;
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])];
  const panel = () => q('.lyrics-panel:not(.lyrics-panel-leave-active)');
  const row = () => q('[data-voice-song-row][data-row-index="0"][data-track-id="1710"]', panel());
  const button = (name, root = document) => q(`button[aria-label="${name}"]`, root);
  const byText = (name, root) => all('button', root).find((node) => node.textContent.trim() === name);
  const picker = (stage) => q(`dialog[open][data-voice-download-picker]${stage ? `[data-voice-download-stage="${stage}"]` : ''}:not(.modal-leave-active)`);
  const options = () => q('dialog[open].download-dialog:not(.modal-leave-active)');
  const menu = () => q('[data-menu-key="download"]');
  const visible = (node) => !!node?.getClientRects().length && !node.closest('[inert]');
  const enabled = (node) => visible(node) && !node.disabled && node.getAttribute('aria-disabled') !== 'true';
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, detail = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...detail },
  });
  const waitTime = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const frame = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const assert = (ok, reason) => { if (!ok) throw Error(reason); };
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const trace = [];
  let epoch, count = 0, nonce = 0, last, baseline, reset;

  const state = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  const stateView = (value) => ({
    kind: value.resourceKind, trackId: value.track?.id, voiceId: value.track?.voiceId,
    playId: value.playId, entryId: value.entryId, state: value.state, position: value.position,
    seeking: value.nativeSeeking, error: value.error,
  });
  async function tasks() {
    const query = { category: 'all', keyword: '', sort: 'completed', descending: true, offset: 0, limit: 10 };
    const active = await invoke('downloads', { epoch, query: { ...query, tab: 'active' } });
    const done = await invoke('downloads', { epoch, query: { ...query, tab: 'completed' } });
    assert(active.epoch === epoch && done.epoch === epoch, 'download view belongs to synthetic account');
    const task = (row) => ({ id: row.id, trackId: row.track.id, source: row.source,
      status: row.status, quality: row.actualQuality, bytes: row.bytes, path: row.path, encrypted: row.encrypted });
    return { active: active.tasks.map(task), done: done.tasks.map(task) };
  }
  async function wait(predicate, message) {
    last = message;
    const deadline = performance.now() + 18000;
    while (performance.now() < deadline) {
      const result = await predicate();
      if (result) return result;
      await waitTime(55);
    }
    throw Error(message);
  }
  async function click(find, message) {
    const node = await wait(() => enabled(find()) && find(), message);
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    node.click();
  }
  function edit(node, value) {
    assert(enabled(node), 'actual dialog input is writable');
    node.value = value;
    node.dispatchEvent(new Event(node.tagName === 'SELECT' ? 'change' : 'input', { bubbles: true }));
  }
  async function nativeTrace() {
    const id = ++nonce;
    await emit('voice_song_download_poll', { nonce: id });
    return wait(() => window.__voiceSongDownloadNative?.[0] === id && window.__voiceSongDownloadNative[1],
      'synthetic server receipt saved');
  }
  function result(name) {
    return trace.find((call) => call.name === name)?.result;
  }
  function projection() {
    return { picker: picker()?.dataset.voiceDownloadStage ?? null,
      options: visible(options()), pickerRows: all('.collect-row', picker()).map((node) => node.getAttribute('aria-label')),
      title: (options() ?? picker())?.querySelector('h2')?.textContent.trim() ?? null,
      status: (options() ?? picker())?.querySelector('[role="status"]')?.textContent.trim() ?? null,
      focus: document.activeElement?.getAttribute('data-row-index') ?? null,
      downloads: visible(q('.downloads-page')), vinyl: visible(panel()) };
  }
  async function capture(name, extra = {}) {
    await frame();
    const current = { player: stateView(await state()), queue: await queue() };
    const records = await tasks();
    if (!baseline) baseline = current;
    assert(same(current, baseline), 'same paused episode clock, decoder and queue at every stage');
    if (name !== 'transferred') assert(!records.active.length && !records.done.length, 'no task before actual start');
    const id = `ux_${String(++count).padStart(2, '0')}_${name}`;
    window.__podcastProgressStage = '';
    await emit(id, { ...current, mode, actual: document.visibilityState === 'visible', records,
      dom: projection(), commands: structuredClone(trace), ...extra });
    await wait(() => window.__podcastProgressStage === id, 'native stage persisted before next interaction');
  }
  async function openPicker() {
    await wait(() => visible(picker('pick')) && enabled(q('[data-playlist-id="81"]', picker('pick'))) &&
      !q('[data-playlist-id="82"],[data-playlist-id="83"]', picker('pick')), 'only editable non-favorite destination shown');
    assert(enabled(button('创建新歌单', picker('pick'))), 'create row present');
    await capture('picker');
  }
  async function openOptions() {
    await wait(() => visible(options()) && !picker() && q('h2', options())?.textContent.trim() === '下载歌曲' &&
      q('.download-selection', options())?.textContent.trim() === '合成包含歌曲 1710',
      'ordinary song options follow exact card row and picker result');
  }
  async function closePicker() {
    await click(() => button('关闭歌曲下载', picker()), 'close scoped song picker');
    await wait(() => !picker() && !options() && document.activeElement === row(), 'dialog closes and returns row focus');
  }
  async function transfer() {
    edit(q('input[aria-label="下载目录"]', options()), output);
    edit(q('select[aria-label="下载音质"]', options()), 'standard');
    edit(q('select[aria-label="保存格式"]', options()), 'audio');
    edit(q('select[aria-label="文件夹组织"]', options()), '0');
    edit(q('select[aria-label="文件命名"]', options()), '1');
    for (const control of all('.download-checkboxes input', options())) {
      if (control.checked) control.click();
    }
    await click(() => byText('开始下载', options()), 'submit exact ordinary native download');
    const method = mode === 'liked' ? 'playerVoiceSongDownloadStart' : 'playerVoiceSongDownloadStartFromPlaylist';
    await wait(() => result(method)?.accepted.length === 1 && !options() && !panel() &&
      visible(q('.downloads-page')), 'typed start accepted and downloads management shown');
    const done = await wait(async () => {
      const view = await tasks();
      return view.done.length === 1 && !view.active.length && view.done[0].status === 'complete' && view.done[0];
    }, 'native downloaded file completed');
    assert(done.source === 'song' && done.trackId === '1710' && done.quality === 'standard' &&
      done.path?.endsWith('.mp3') && done.bytes > 0 && !done.encrypted, 'complete plain standard song artifact');
    await capture('transferred');
  }
  try {
    assert(['liked', 'cancel', 'existing_rejected', 'create_rejected', 'recover'].includes(mode) &&
      typeof output === 'string', 'exact sealed test case');
    await wait(async () => {
      const account = await invoke('account_snapshot');
      if (account.status !== 'authenticated' || !enabled(button('打开设置'))) return false;
      epoch = account.epoch;
      return enabled(q('input[aria-label="搜索音乐"]'));
    }, 'synthetic account and search shell ready');
    const provides = q('#app').__vue_app__._context.provides;
    const core = provides[Reflect.ownKeys(provides).find((key) => typeof key === 'symbol' && key.description === 'core-client')];
    const methods = ['playerVoiceSongs', 'playerVoiceSongDownloadPrepare', 'playerVoiceSongDownloadStart',
      'playerVoiceSongDownloadChoosePlaylist', 'playerVoiceSongDownloadPickerStatus',
      'playerVoiceSongDownloadCreatePlaylist', 'playerVoiceSongDownloadCreationStatus',
      'playerVoiceSongDownloadAddCreatedPlaylist', 'playerVoiceSongDownloadStartFromPlaylist',
      'downloadCommand', 'playlistCollectPage'];
    const originals = methods.map((name) => core[name]);
    assert(originals.every((method) => typeof method === 'function'), 'real typed adapters installed');
    methods.forEach((name, index) => {
      core[name] = async (...args) => {
        const call = { name, args: structuredClone(args) };
        trace.push(call);
        try {
          const value = await originals[index].apply(core, args);
          assert(!JSON.stringify(value).includes('SYNTHETIC_INCLUDED_PRIVATE') &&
            !JSON.stringify(value).includes('m1.music.126.net'), 'IPC omits private wire fields and authorization URL');
          call.result = value;
          return value;
        } catch (error) {
          call.error = error?.code ?? String(error);
          throw error;
        }
      };
    });
    reset = () => methods.forEach((name, index) => { core[name] = originals[index]; });
    await click(() => q('input[aria-label="搜索音乐"]'), 'open actual discovery');
    await click(() => button('播客榜：更多', q('#search-discovery-panel:not(.menu-leave-active)')), 'enter classic podcast');
    await wait(() => q('.podcast-discovery .podcast-top[data-layout="classic"]') &&
      q('[data-podcast-scene-fm] [data-scene-fm-title]'), 'Scene FM visible and ready');
    await click(() => button('播放随心听FM', q('[data-podcast-scene-fm]')), 'start native episode');
    const playing = await wait(async () => {
      const view = await state();
      return view.track?.voiceId === '601' && view.state === 'playing' && view.position > .2 &&
        !view.nativeSeeking && !view.error && view;
    }, 'actual libmpv episode advances');
    await click(() => button('暂停'), 'pause episode before opening SongCard');
    let prior;
    await wait(async () => {
      const view = await state();
      if (view.playId !== playing.playId || view.state !== 'paused' || view.nativeSeeking ||
        view.error || !(view.position > .2)) return false;
      const settled = prior === view.position;
      prior = view.position;
      return settled;
    }, 'same actual native clock fully paused');
    await click(() => button('打开播放详情'), 'open exact episode vinyl');
    const card = await wait(() => {
      const value = result('playerVoiceSongs');
      return value?.receipt && value.voiceId === '601' && value.playId === playing.playId &&
        value.tracks[0]?.id === '1710' && visible(row()) && value;
    }, 'native SongCard receipt supplies chosen row');
    await wait(async () => (await nativeTrace()).likedReads > 0 && q('[data-voice-song-heart]', row())
      ?.getAttribute('aria-pressed') === String(mode === 'liked'), 'authoritative liked membership observed');
    await wait(() => panel() && !q('.lyrics-panel-enter-active'), 'playback detail finished opening');
    await capture('song_card');

    const node = row();
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    const box = node.getBoundingClientRect();
    node.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true,
      button: 2, clientX: box.left + Math.min(40, box.width / 2), clientY: box.top + box.height / 2 }));
    await click(menu, 'actual ordinary row context Download action');
    await wait(() => result('playerVoiceSongDownloadPrepare'), 'typed native preparation settles');
    const target = { epoch, playId: playing.playId, receipt: card.receipt, index: 0 };
    const plan = result('playerVoiceSongDownloadPrepare');
    assert(plan.epoch === epoch && plan.playId === playing.playId && plan.trackId === '1710' &&
      plan.decision === (mode === 'liked' ? 'ready' : 'playlist_required'), 'exact native plan for intended ordinary song');
    assert(same(trace.find((call) => call.name === 'playerVoiceSongDownloadPrepare')?.args, [target]),
      'download carries only native page receipt and index');
    if (mode === 'liked') {
      await openOptions();
      await capture('options');
      await transfer();
    } else {
      await openPicker();
      if (mode === 'cancel') {
        await closePicker();
        await capture('cancelled');
      } else if (mode === 'create_rejected') {
        await click(() => button('创建新歌单', picker('pick')), 'open actual create form');
        await wait(() => visible(picker('create')) && document.activeElement === q('input[aria-label="歌单名称"]', picker('create')),
          'create form focuses name');
        const privateBox = q('input[type="checkbox"]', picker('create'));
        assert(privateBox && !privateBox.checked, 'official visible default public, never submitted automatically');
        await capture('create_form');
        edit(q('input[aria-label="歌单名称"]', picker('create')), '合成私密下载歌单');
        privateBox.click();
        await click(() => byText('创建', picker('create')), 'explicitly create synthetic private destination');
        const made = await wait(() => result('playerVoiceSongDownloadCreatePlaylist'), 'guarded create and add read back');
        await wait(() => visible(picker('notice')) && picker('notice').textContent.includes('下载未开始') && !options(),
          'new destination add rejected, options never opened');
        assert(made.createPhase === 'confirmed' && made.playlistId === '84' && made.add?.phase === 'rejected' &&
          made.add.addFailed && !made.add.continuation, 'create confirmation is distinct from add failure');
        await capture('add_rejected');
        await closePicker();
        await capture('closed');
      } else {
        await click(() => q('[data-playlist-id="81"]', picker('pick')), 'explicitly choose verified existing destination');
        const choice = await wait(() => result('playerVoiceSongDownloadChoosePlaylist'), 'guarded add outcome settles');
        if (mode === 'existing_rejected') {
          await openOptions();
          assert(choice.phase === 'rejected' && choice.addFailed && choice.continuation &&
            options().textContent.includes('加入歌单失败；仍可继续设置下载。'), 'known false exposes independent download warning');
          await capture('options');
          await transfer();
        } else {
          await wait(() => visible(picker('recover')) && enabled(byText('核对结果', picker('recover'))) && !options(),
            'lost add response offers only readback');
          assert(choice.phase === 'uncertain' && !choice.continuation, 'unknown write does not admit download');
          await capture('uncertain');
          await emit('voice_song_download_reveal');
          await wait(() => window.__voiceSongDownloadRevealed, 'only synthetic server membership changed');
          await click(() => byText('核对结果', picker('recover')), 'explicit read-only outcome check');
          const checked = await wait(() => result('playerVoiceSongDownloadPickerStatus'), 'guarded status response settles');
          await openOptions();
          assert(checked.phase === 'confirmed' && !checked.addFailed && checked.continuation &&
            checked.operationId === choice.operationId, 'status read confirms same original write, never replays it');
          await capture('reconciled');
          await click(() => byText('取消', options()), 'close options without starting');
          await wait(() => !options() && !picker() && document.activeElement === row(), 'cancel returns to exact row without starting');
          await capture('closed');
        }
      }
    }
    reset();
    await emit('complete', { ok: true, count, mode, realMusicRequests: 0, realAccountWrites: 0, physicalAudioVerified: false });
  } catch (error) {
    reset?.();
    await emit('failed', { ok: false, mode, last, count, error: String(error),
      dom: projection(), commands: trace, text: (document.querySelector('main')?.innerText ?? '').slice(0, 400) });
  }
})();
