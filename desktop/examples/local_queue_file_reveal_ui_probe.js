// Real queue menu and native IPC using isolated synthetic local audio.
(async () => {
  const { root, capture: captureEnabled } = window.__localProof;
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const command = 'player_queue_open_local_directory';
  const emit = (stage, value = {}) =>
    invoke('plugin:event|emit', { event: 'local-proof', payload: { stage, ...value } });
  const assert = (value, label) => { if (!value) throw Error(label); };
  const button = (label, root = document) => [...root.querySelectorAll('button')].find(
    (item) => item.getAttribute('aria-label') === label || item.textContent.trim() === label,
  );
  async function wait(test, label) {
    for (let i = 0; i < 200; i++) {
      const result = await test();
      if (result) return result;
      await new Promise((done) => setTimeout(done, 100));
    }
    throw Error(label);
  }
  function click(item) {
    assert(item && !item.disabled, 'missing or disabled control');
    item.focus({ preventScroll: true });
    item.click();
  }
  const key = (node, value, shiftKey = false) => node.dispatchEvent(new KeyboardEvent('keydown', {
    key: value, shiftKey, bubbles: true, cancelable: true,
  }));
  const menu = () => one('.action-menu[aria-label="播放列表歌曲操作"]');
  const action = (label) => menu() && button(label, menu());
  const locate = () => action('打开文件所在目录');
  const row = (entry) => one(`.queue-row[data-entry-id="${CSS.escape(entry)}"]`);
  const more = (entry) => row(entry)?.querySelector('.queue-row-more');
  async function show(entry, method) {
    const anchor = await wait(() => more(entry), 'local queue More');
    if (method === 'more') click(anchor);
    else {
      const title = row(entry).querySelector('.queue-song');
      title.focus({ preventScroll: true });
      if (method === 'key') key(title, 'F10', true);
      else {
        const box = title.getBoundingClientRect();
        title.dispatchEvent(new MouseEvent('contextmenu', {
          bubbles: true, cancelable: true, button: 2, clientX: box.x + 2, clientY: box.y + 2,
        }));
      }
    }
    await wait(() => locate() && !locate().disabled, 'queue file action');
    return anchor;
  }
  async function close(anchor) {
    await wait(() => !menu() && document.activeElement === anchor, 'queue menu restores More focus');
  }
  async function capture(stage, target) {
    if (!captureEnabled) return;
    await wait(() => !one('.menu-enter-active,.menu-leave-active'), 'menu transition settled');
    await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));
    const box = target.getBoundingClientRect();
    assert(box.width > 0 && box.height > 0, 'capture target has layout');
    await emit(stage);
    await wait(() => window.__folderCaptureAck === stage, 'capture acknowledged');
  }
  async function rejected(args, code) {
    try {
      await invoke(command, args);
    } catch (error) {
      assert(error?.code === code, JSON.stringify(error));
      return error.code;
    }
    throw Error('unbound request accepted');
  }
  try {
    await wait(() => one('.welcome'), 'guest startup');
    const { epoch } = await wait(async () => {
      const value = await invoke('account_snapshot');
      return ['signed_out', 'expired'].includes(value.status) && value;
    }, 'guest epoch');
    click(button('本地音乐'));
    await wait(() => one('.local-page'), 'local page');
    await invoke('local_command', { command: { kind: 'watch', path: root + '/media' } });
    await wait(() => all('.local-tracks tbody tr').length === 5, 'native library scan');
    const cueTitle = all('.local-title').find((node) => node.textContent.includes('夜行 · 下篇'));
    assert(cueTitle, 'CUE title');
    cueTitle.dispatchEvent(new MouseEvent('dblclick', { bubbles: true, cancelable: true }));
    await wait(async () => {
      const value = await invoke('player_snapshot', { epoch });
      return value.state === 'playing' && value.position > 0.2 && value.timeline?.mode === 'cue';
    }, 'real native CUE clock');
    click(button('播放队列'));
    const before = await invoke('player_queue', { epoch });
    const named = (title) => before.tracks.find((entry) => entry.track.name === title);
    const cue = named('夜行 · 下篇'), ncm = named('归途'), wav = named('雨声');
    assert(cue?.entryId === before.current && ncm && wav &&
      [cue, ncm, wav].every((entry) => entry.localMenu && !entry.voiceMenu && !entry.catalogMenu),
      'only native indexed local row menu capability');
    assert(!JSON.stringify(before).includes(root) && !JSON.stringify(before).includes('归途.ncm'),
      'queue IPC exposes no physical source');
    const cueMore = await show(cue.entryId, 'more');
    assert(all('.action-menu [role="menuitem"]').map((item) => item.textContent.trim()).join('|') ===
      '暂停|收藏|歌手: 北京|专辑: 北京|来自：本地音乐|打开文件所在目录|从列表中删除',
      'official local menu order and evidence-backed actions');
    assert(action('歌手: 北京').disabled && action('专辑: 北京').disabled, 'local tags cannot be routes');
    await capture('ux_01_queue_local_cue', locate());
    click(action('暂停'));
    await close(cueMore);
    const paused = await wait(async () => {
      const value = await invoke('player_snapshot', { epoch });
      return value.state === 'paused' && value;
    }, 'native pause through the queue menu');
    const args = (entryId, revision = before.revision) => ({ epoch, revision, entryId });
    const awaitShell = async (count) => {
      await emit('await_queue_file_call', { count });
      await wait(() => window.__queueFileCalls === count, 'native FileManager1 receipt');
    };
    const cueAnchor = await show(cue.entryId, 'more');
    click(locate());
    await close(cueAnchor);
    await awaitShell(1);
    const ncmMore = await show(ncm.entryId, 'key');
    assert(action('播放') && !action('暂停'), 'noncurrent NCM retains its own menu');
    await capture('ux_02_queue_local_ncm', locate());
    click(locate());
    await close(ncmMore);
    await awaitShell(2);

    const changedAnchor = await show(ncm.entryId, 'right');
    await emit('replace_queue_ncm');
    await wait(() => window.__queueNcmChanged, 'fixture file changed after opening its menu');
    click(locate());
    await close(changedAnchor);
    const notice = await wait(() => {
      const value = one('.queue-panel .queue-notice[role="status"]');
      return value?.textContent.includes('原文件已移动、变化或无法读取') && value;
    }, 'visible changed-file explanation');
    await capture('ux_03_queue_local_changed', notice);
    const retained = await invoke('player_snapshot', { epoch });
    const same = await invoke('player_queue', { epoch });
    assert(retained.playId === paused.playId && retained.entryId === paused.entryId &&
      retained.position === paused.position && same.revision === before.revision,
      'native reveal preserves playback and queue');

    await show(wav.entryId, 'right');
    click(action('从列表中删除'));
    const after = await wait(async () => {
      const value = await invoke('player_queue', { epoch });
      return value.revision !== before.revision && value;
    }, 'queue removal updates revision');
    assert(!after.tracks.some((entry) => entry.entryId === wav.entryId) && after.tracks.length === before.tracks.length - 1,
      'only chosen noncurrent row removed from the queue');
    const final = await invoke('player_snapshot', { epoch });
    assert(final.playId === paused.playId && final.position === paused.position, 'remove keeps loaded CUE');
    const negative = [
      await rejected(args(cue.entryId), 'snapshot_changed'),
      await rejected({ ...args(cue.entryId, after.revision), epoch: epoch + '0' }, 'stale'),
      await rejected(args(cue.track.id, after.revision), 'invalid_input'),
    ];
    await emit('complete', { ok: true, nativeCUE: true, actualNcmSelection: true, focusRestored: true,
      samePlay: true, negative, realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', { ok: false, error: error instanceof Error ? error.message : String(error), stack: error?.stack });
  }
})();
