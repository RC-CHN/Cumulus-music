// Real recent-row menu with an actual held synthetic API response and native playback.
(async () => {
  const one = (s, root = document) => root?.querySelector(s);
  const row = (id) => one(`[data-recent-voice="${id}"]`);
  const trigger = (id) => one('[data-podcast-recent-more]', row(id));
  const menu = () => one('[role="menu"][aria-label^="单集菜单"]:not([inert])');
  const item = (key) => one(`[data-menu-key="${key}"]`, menu());
  const dialog = (kind) => one(`dialog.${kind}[open]:not([inert])`);
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, facts = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...facts },
  });
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const frame = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const assert = (test, label) => { if (!test) throw Error(label); };
  const button = (text, root = document) => [...(root?.querySelectorAll('button') ?? [])]
    .find((el) => el.textContent.trim() === text || el.getAttribute('aria-label') === text);
  const ready = (el) => !!el?.getClientRects().length && !el.disabled && !el.closest('[inert]');
  const settled = () => !one('.page-enter-active,.page-leave-active,.modal-enter-active,.modal-leave-active,.menu-enter-active,.menu-leave-active');
  let stageCount = 0, last = 'bootstrap', epoch, parked;
  function solidPadding(node, right = false) {
    const rect = node.getBoundingClientRect();
    const padding = parseFloat(getComputedStyle(node)[right ? 'paddingRight' : 'paddingLeft']);
    const offset = Math.min(5, padding / 2);
    const x = right ? rect.right - offset : rect.left + offset, y = rect.top + rect.height / 2;
    assert(padding >= 4 && document.elementFromPoint(x, y) === node, 'visible solid padding');
    for (let current = node; current; current = current.parentElement) {
      const style = getComputedStyle(current);
      assert(style.backgroundImage === 'none' && style.opacity === '1', 'opaque solid sample only');
      const rgb = /^rgb\((\d+),\s*(\d+),\s*(\d+)\)$/.exec(style.backgroundColor);
      if (rgb) return { x, y, color: 'rgb(' + rgb.slice(1).join(', ') + ')' };
      assert(style.backgroundColor === 'transparent' ||
        /^rgba\(\d+,\s*\d+,\s*\d+,\s*0\)$/.test(style.backgroundColor), 'transparent ancestor only');
    }
    throw Error('no opaque padding color');
  }
  const pages = [], menuReplies = [], selections = [], queueCalls = [], copied = [];
  async function wait(fn, label, budget = 12000) {
    last = label;
    const until = performance.now() + budget;
    while (performance.now() < until) {
      const value = await fn();
      if (value) return value;
      await sleep(25);
    }
    throw Error(label);
  }
  async function click(find, label) {
    const el = await wait(() => { const node = find(); return settled() && ready(node) && node; }, label);
    el.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    await frame();
    await wait(() => {
      if (el !== find() || !ready(el) || !settled()) return false;
      const r = el.getBoundingClientRect();
      return el.contains(document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2));
    }, label + ' hit target');
    el.focus({ preventScroll: true });
    el.click();
    return el;
  }
  const state = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  async function menuReady(id, count) {
    await wait(() => trigger(id)?.getAttribute('aria-expanded') === 'true' &&
      menu()?.getAttribute('aria-label') === `单集菜单：合成单集 ${id}` &&
      item('comments')?.textContent.trim() === `查看评论(${count})`, 'recent program menu ' + id);
  }
  async function capture(label, extra = {}) {
    await wait(settled, 'settled ' + label);
    await frame();
    const open = menu(), rect = open?.getBoundingClientRect();
    assert(!rect || (rect.left >= 7 && rect.top >= 7 && rect.right <= innerWidth - 7 &&
      rect.bottom <= innerHeight - 7), 'menu fits viewport');
    const active = await state(), rows = await queue();
    const stage = `ux_${String(++stageCount).padStart(2, '0')}_${label}`;
    window.__podcastProgressStage = '';
    const openDialog = one('dialog[open]:not([inert])');
    const paintSamples = window.__CUMULUS_PROOF_INVOKE__
      ? openDialog ? [solidPadding(openDialog), solidPadding(openDialog, true)]
        : [one('.sidebar-caption'), one('.playerbar')].map((node) => solidPadding(node))
      : [];
    await emit(stage, { ...extra, actual: document.visibilityState === 'visible',
      screenX, screenY, ratio: devicePixelRatio, paintSamples,
      menu: open ? [...open.querySelectorAll('[data-menu-key]')].map((el) =>
        [el.dataset.menuKey, el.textContent.trim()]) : null,
      native: { state: active.state, voice: active.track?.voiceId, song: active.track?.id,
        playId: active.playId, entryId: active.entryId, position: active.position,
        revision: rows.revision, origin: active.origin,
        rows: rows.tracks.map((entry) => entry.track.voiceId) },
    });
    await wait(() => window.__podcastProgressStage === stage, 'native capture ' + label);
  }
  try {
    epoch = (await wait(async () => {
      const value = await invoke('account_snapshot');
      return value.status === 'authenticated' && value;
    }, 'sealed account')).epoch;
    const provides = one('#app').__vue_app__._context.provides;
    const core = provides[Reflect.ownKeys(provides).find((key) =>
      typeof key === 'symbol' && key.description === 'core-client')];
    const observe = (name, onValue, onError = () => {}) => {
      const native = core[name];
      core[name] = async (...args) => {
        try { const value = await native.apply(core, args); onValue(args, value); return value; }
        catch (e) { onError(args, e); throw e; }
      };
    };
    observe('podcastRecent', ([owner, query], value) => pages.push({ owner, query, value }));
    observe('podcastRecentProgramMenu', ([owner, target], value) => menuReplies.push({ owner, target, value }),
      ([owner, target], error) => menuReplies.push({ owner, target, error: error?.code }));
    observe('playSelection', ([owner, selection]) => selections.push({ owner, selection }));
    observe('playerAction', ([owner, action], value) => {
      if (action.kind === 'queue_recent_program') queueCalls.push({
        owner, action, revision: value.queueRevision, playId: value.playId, entryId: value.entryId,
      });
    });
    observe('copyText', ([value]) => copied.push(value));

    await click(() => button('我的播客', one('.sidebar-navigation[aria-label="我的"]')), 'my podcasts');
    await click(() => one('[data-podcast-recent-tab]'), 'recent tab');
    await wait(() => pages.length === 1 && ready(trigger('501')), 'first recent revision');
    const old = { revision: pages[0].value.value.revision, voiceId: '501' };
    const more = await click(() => trigger('501'), 'More starts held native count');
    await wait(async () => {
      await emit('recent_program_menu_poll');
      return window.__recentProgramMenuStatus?.held === true;
    }, 'sealed API is holding the real count reply');
    assert(!menu() && more.getAttribute('aria-expanded') === 'false', 'menu waits for preparation');
    await click(() => button('加载更多'), 'publish a new native recent revision');
    await wait(() => pages.length === 2 && pages[1].query.kind === 'next', 'continuation reply');
    const current = pages[1].value.value.revision;
    assert(current !== old.revision, 'recent page acquired a distinct revision');
    await emit('recent_program_menu_release');
    await wait(async () => {
      await emit('recent_program_menu_poll');
      return window.__recentProgramMenuStatus?.delivered === true;
    }, 'the held native mock actually returned its response');
    await wait(() => menuReplies.length === 1 && menuReplies[0].error === 'snapshot_changed',
      'old count was rejected by native scope check');
    assert(!menu() && more.getAttribute('aria-expanded') === 'false', 'late reply did not open old menu');
    const retired = await invoke('podcast_recent_program', { epoch, target: old })
      .then(() => null, (error) => error?.code);
    assert(retired === 'snapshot_changed', 'native refuses the retired link/download target');
    await capture('retired_reply', { old, current, retired, delivered: true, menuReplies: menuReplies.slice() });

    const next = row('503');
    next.scrollIntoView({ block: 'center', behavior: 'instant' });
    await frame();
    const rect = next.getBoundingClientRect();
    const x = rect.left + Math.min(130, rect.width / 2), y = rect.top + rect.height / 2;
    const hit = document.elementFromPoint(x, y);
    assert(hit?.closest('[data-recent-voice]') === next, 'context point hits the visible recent row');
    hit.dispatchEvent(new MouseEvent('contextmenu', { button: 2, bubbles: true, cancelable: true,
      clientX: x, clientY: y }));
    await menuReady('503', 53);
    await capture('current_menu', { pointerHitVoice: next.dataset.recentVoice, menuReplies: menuReplies.slice() });
    await click(() => item('share'), 'open recent program share');
    const sharing = await wait(() => dialog('track-share-dialog'), 'recent program share dialog');
    const link = 'https://music.163.com/dj?id=503';
    assert(one('input', sharing)?.value === link && sharing.textContent.includes('合成单集 503'), 'program share identity');
    await click(() => button('复制链接', sharing), 'copy in share');
    await wait(() => copied.length === 1, 'clipboard receipt');
    await capture('share', { link, copied: copied.slice() });
    await click(() => button('关闭分享', sharing), 'close share');
    await wait(() => !dialog('track-share-dialog') && document.activeElement === trigger('503'), 'share restores focus');
    await click(() => trigger('503'), 'More for direct copy');
    await menuReady('503', 53);
    await click(() => item('copy'), 'copy exact program target');
    await wait(() => copied.length === 2, 'direct clipboard receipt');

    await click(() => one('.recent-play', row('501')), 'play baseline from the recent row');
    await wait(async () => {
      const v = await state(); return v.state === 'playing' && v.position > .2 && v.track?.voiceId === '501';
    }, 'native baseline playing');
    await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    await invoke('player_action', { epoch, action: { kind: 'mode', mode: 'single_loop' } });
    parked = await wait(async () => {
      const v = await state(); return v.state === 'paused' && v.position > .2 && v;
    }, 'native baseline paused');
    await click(() => trigger('503'), 'More for next');
    await menuReady('503', 53);
    await click(() => item('next'), 'queue the selected recent program');
    const queued = await wait(async () => {
      const value = await queue();
      return value.tracks.map((entry) => entry.track.voiceId).join(',') === '501,503' && value;
    }, 'native queue contains exactly the retained and inserted program');
    // The actor can publish its queue before WebView receives the command's
    // acknowledgement. Require both independent observations before capture.
    const receipt = await wait(() => queueCalls[0], 'native queue action acknowledgement');
    assert(queueCalls.length === 1 && receipt.owner === epoch &&
      receipt.action.expected_queue_revision === parked.queueRevision &&
      receipt.action.target.revision === current && receipt.action.target.voiceId === '503' &&
      receipt.revision === queued.revision && receipt.playId === parked.playId &&
      receipt.entryId === parked.entryId, 'acknowledged the same queue edit and retained decoder');
    await capture('next', { copied: copied.slice(), queueCalls: queueCalls.slice(),
      previous: { playId: parked.playId, entryId: parked.entryId, position: parked.position,
        revision: parked.queueRevision } });

    await click(() => trigger('503'), 'More for download');
    await menuReady('503', 53);
    await click(() => item('download'), 'open recent program download');
    await wait(() => dialog('download-dialog'), 'program download dialog');
    await capture('download', { dialog: dialog('download-dialog')?.querySelector('h2')?.textContent.trim() });
    await click(() => button('关闭下载对话框', dialog('download-dialog')), 'cancel program download');

    await click(() => trigger('503'), 'More for comments');
    await menuReady('503', 53);
    await click(() => item('comments'), 'open recent program comments');
    await wait(() => {
      const header = one('.comment-resource-header[data-resource-kind="voice"][data-resource-id="503"]');
      return header?.getAttribute('aria-busy') === 'false' &&
        one('.comment-resource-play', header) &&
        one('[data-comment-resource-name]', header)?.textContent.trim() === '合成单集 503';
    }, 'comment route loads the correct program metadata');
    await capture('comments');
    await click(() => button('返回上一页'), 'return to recent programs');

    await click(() => trigger('504'), 'More for play');
    await menuReady('504', 54);
    await click(() => item('play'), 'play only the selected recent program');
    const playing = await wait(async () => {
      const v = await state(); return v.state === 'playing' && v.position > .2 && v.track?.voiceId === '504' && v;
    }, 'new native program playing');
    await capture('play', { selections: selections.slice() });

    await click(() => trigger('504'), 'More for current program');
    await menuReady('504', 54);
    await click(() => item('play'), 'menu Play is not the current-row toggle');
    await wait(() => selections.length === 3, 'current menu action dispatched playback');
    await wait(async () => {
      const v = await state(); return v.state === 'playing' && v.playId === playing.playId &&
        v.entryId === playing.entryId && v.position >= playing.position;
    }, 'same current program remains playing');
    await capture('current_play', { selections: selections.slice(),
      before: { playId: playing.playId, entryId: playing.entryId, position: playing.position } });
    await emit('complete', { ok: true, stageCount, queueCalls: queueCalls.slice(),
      realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', { ok: false, last, error: String(error?.message ?? JSON.stringify(error)),
      pages, menuReplies, selections, queueCalls, menu: menu()?.outerHTML,
      route: one('.main-content h1')?.textContent.trim() });
  }
})();
