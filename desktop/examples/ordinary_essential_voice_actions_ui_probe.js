// Closed Choice WebView, synthetic account and local null audio. Never submit a download.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, extra = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...extra },
  });
  const assert = (condition, reason) => { if (!condition) throw Error(reason); };
  async function wait(test, label) {
    for (let n = 0; n < 160; n++) {
      const result = await test();
      if (result) return result;
      await sleep(80);
    }
    throw Error(label);
  }
  const focus = (node) => node.focus({ preventScroll: true });
  function click(node) {
    assert(node && !node.disabled, 'control unavailable');
    focus(node);
    node.click();
  }
  const key = (node, key, shiftKey = false) => node.dispatchEvent(new KeyboardEvent(
    'keydown', { key, shiftKey, bubbles: true, cancelable: true },
  ));
  const button = (label, root = document) => [...root.querySelectorAll('button')].find(
    (node) => node.getAttribute('aria-label') === label || node.textContent.trim() === label,
  );
  const voice = (id, action) => one(`[data-essential-focus="voices:${id}:${action}"]`);
  const menu = () => one('.action-menu[role="menu"]:not([inert])');
  const item = (name) => menu()?.querySelector(`[data-menu-key="${name}"]`);
  const download = () => one('.download-dialog[open]:not([inert])');
  const states = [];
  const isAnimating = (node) => node.getAnimations().some(
    (animation) => animation.pending || animation.playState === 'running',
  );
  let contextAnchor;
  const contextFocus = () => ({
    anchorConnected: contextAnchor?.isConnected ?? false,
    currentAnchor: contextAnchor === voice(701, 'menu'),
    menuOpen: Boolean(menu()),
    activeOpener: document.activeElement?.dataset.essentialFocus ?? null,
    activeItem: document.activeElement?.dataset.menuKey ?? null,
    page: contextAnchor?.closest('.page-view')?.className ?? null,
    scrollTop: contextAnchor?.closest('.ordinary-essential')?.scrollTop ?? null,
  });
  let downloads = 0, queueVoiceResponses = 0;
  const fetchIpc = window.fetch;
  window.fetch = function (url, options) {
    const path = String(url);
    if (path.endsWith('/download_command')) downloads++;
    const response = fetchIpc.call(window, url, options);
    if (path.endsWith('/player_action') && JSON.parse(options.body).action.kind === 'queue_voice') {
      return response.then((value) => {
        if (value.ok) queueVoiceResponses++;
        return value;
      });
    }
    return response;
  };
  async function capture(name, extra = {}) {
    const stage = 'ux_' + name;
    await sleep(80);
    await wait(() => all('.page-view,.action-menu,dialog').every((node) => !isAnimating(node)),
      'settled Choice actions');
    await emit(stage, extra);
    await wait(() => window.__CUMULUS_ALBUM_CAPTURE_ACK__ === stage, 'capture ' + stage);
    states.push(name);
  }
  async function openMenu(id, gesture) {
    const anchor = voice(id, 'menu');
    assert(anchor, 'voice menu trigger ' + id);
    const page = anchor.closest('.page-view');
    const ready = () => anchor === voice(id, 'menu') && anchor.isConnected && page &&
      !anchor.closest('[inert],.page-enter-active,.page-leave-active') && !isAnimating(page);
    // The menu intentionally closes when a scroll moves its opener. Let the
    // route transition finish before revealing and opening an off-screen row.
    await wait(ready, 'Choice page entrance settles');
    anchor.scrollIntoView({ block: 'center', inline: 'nearest', behavior: 'instant' });
    let previous;
    await wait(() => {
      const bounds = anchor.getBoundingClientRect();
      const { x, y, width, height } = bounds;
      const layout = [x, y, width, height].join(':');
      const settled = layout === previous;
      previous = layout;
      return ready() && settled && width > 0 && height > 0 &&
        x >= 0 && y >= 0 && bounds.right <= innerWidth && bounds.bottom <= innerHeight;
    }, 'voice menu opener is visible and stable after scrolling');
    if (gesture === 'pointer') {
      const r = anchor.getBoundingClientRect();
      anchor.dispatchEvent(new MouseEvent('contextmenu', {
        button: 2, clientX: r.left + 2, clientY: r.top + 2, bubbles: true, cancelable: true,
      }));
    } else if (gesture === 'keyboard') {
      focus(anchor);
      key(anchor, 'F10', true);
    } else click(anchor);
    await wait(() => item('next') && menu().contains(document.activeElement), 'focus enters the voice menu');
    return anchor;
  }
  async function cancelDownload(anchor) {
    click(button('关闭下载对话框', download()));
    await wait(() => !one('.download-dialog') && document.activeElement === anchor,
      'download cancel restores the precise opener');
  }
  try {
    click(await wait(() => {
      const choice = button('精选', one('.sidebar') ?? document);
      return choice && !choice.disabled && choice;
    }, 'Choice navigation available'));
    await wait(() => voice(701, 'menu') && !voice(701, 'play').disabled, 'synthetic episode cards and player');
    const menuAnchor = await openMenu(701, 'keyboard');
    contextAnchor = menuAnchor;
    assert(document.activeElement === item('next'), 'first context item focused');
    assert(all('.action-menu [data-menu-key]').map((node) => node.dataset.menuKey).join(',') ===
      'next,download', 'available operations are the implemented official subset');
    await capture('context_menu');
    const escapeTarget = item('next');
    assert(escapeTarget && document.activeElement === escapeTarget,
      'the first context item still owns focus before Escape');
    key(escapeTarget, 'Escape');
    await wait(() => !menu() && document.activeElement === menuAnchor, 'Escape restores More');

    const { epoch } = await invoke('account_snapshot');
    const snapshot = () => invoke('player_snapshot', { epoch });
    const queue = () => invoke('player_queue', { epoch });
    const song = one('[data-essential-focus="songs:2:play"]');
    song.scrollIntoView({ block: 'center' });
    click(song);
    await wait(async () => {
      const view = await snapshot();
      return view.state === 'playing' && view.track?.id === '2' && view.position > .3;
    }, 'current catalog song is audibly advancing in the native null engine');
    click(song);
    const paused = await wait(async () => {
      const view = await snapshot();
      return view.state === 'paused' && view;
    }, 'catalog song paused');
    const before = await queue();
    assert(before.tracks.length === 9, 'selected catalog rail queue');

    let inserted;
    for (const gesture of ['pointer', 'more']) {
      const origin = await openMenu(701, gesture);
      const expected = queueVoiceResponses + 1;
      click(item('next'));
      await wait(() => queueVoiceResponses === expected, 'episode next completes in native code');
      const updated = await queue();
      assert(inserted ? updated.revision === inserted.revision : updated.revision !== before.revision,
        'repeating an already-next episode leaves the native queue revision unchanged');
      assert(updated.tracks.length === 10, 'exactly one program is inserted, also on repeated action');
      assert(updated.tracks[1].track.id === '2' && !updated.tracks[1].track.voiceId,
        'the catalog song keeps its resource namespace');
      assert(updated.tracks[2].track.id === '2' && updated.tracks[2].track.voiceId === '701',
        'the episode with the same media id is still the next independent entry');
      assert(!inserted || updated.tracks[2].entryId === inserted.tracks[2].entryId,
        'repeat moves/reuses the existing episode entry');
      assert(updated.tracks.slice(0, 2).concat(updated.tracks.slice(3)).every(
        (row, index) => row.entryId === before.tracks[index].entryId),
      'every original entry stays in place with its identity');
      const after = await snapshot();
      assert(after.playId === paused.playId && after.state === 'paused' &&
        Math.abs(after.position - paused.position) < .05, 'paused decoder and position unchanged');
      assert(document.activeElement === origin, 'next returns focus to its trigger');
      inserted = updated;
    }
    await capture('native_next', { retainedCatalogRows: before.tracks.length });

    const origin = await openMenu(701, 'more');
    click(item('download'));
    await wait(() => download()?.querySelector('h2')?.textContent.trim() === '下载 1 个单集',
      'menu opens the one-episode download confirmation');
    assert(downloads === 0, 'opening the confirmation does not start a transfer');
    await capture('download_menu');
    await cancelDownload(origin);
    const direct = voice(701, 'download');
    click(direct);
    await wait(() => download()?.querySelector('h2')?.textContent.trim() === '下载 1 个单集',
      'hover action opens the same one-episode confirmation');
    await capture('download_direct');
    await cancelDownload(direct);
    assert(downloads === 0 && (await queue()).revision === inserted.revision,
      'both cancellations preserve the queue and send no download command');

    click(button('下一首'));
    await wait(async () => {
      const view = await snapshot();
      return view.state === 'playing' && view.track?.voiceId === '701' && view.position > .2;
    }, 'the enqueued episode plays through the native player');
    await capture('native_voice', { playing: true });

    const liveQueue = await queue();
    await invoke('player_action', { epoch, action: {
      kind: 'queue_for', expected_queue_revision: liveQueue.revision, edit: { kind: 'clear' },
    } });
    await wait(async () => (await queue()).tracks.length === 0, 'clear the synthetic queue only');
    await openMenu(702, 'more');
    click(item('next'));
    await wait(async () => {
      const view = await snapshot();
      return view.state === 'playing' && view.track?.voiceId === '702' &&
        view.queueLen === 1 && view.position > .2;
    }, 'next in an empty queue starts just the selected program');
    await capture('empty_next', { playing: true });
    await emit('complete', { ok: true, states, downloads, realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', { ok: false, error: String(error), states, contextFocus: contextFocus() });
  }
})();
