// Native Recently Played radio menus in the sealed, synthetic catalog host.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])];
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, payload = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...payload } });
  const assert = (ok, message) => { if (!ok) throw Error(message); };
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const ready = node => node?.isConnected && node.getClientRects().length &&
    !node.matches(':disabled,[aria-disabled="true"]') && !node.closest('[inert]');
  const text = node => node?.textContent.trim();
  const action = action => invoke('player_action', { epoch, action });
  const player = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  const history = async kind => (await invoke('history_page', { epoch, kind, refresh: false })).value;
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const voiceIds = state => state.tracks.map(row => row.track.voiceId);
  const decoder = state => ({
    epoch: state.epoch, state: state.state, playId: state.playId, entryId: state.entryId,
    position: state.position, voiceId: state.track?.voiceId, mediaId: state.track?.id,
  });
  const tabs = '[aria-label="播放记录类型"]';
  const more = id => one(`[data-history-radio-more="${id}"]`);
  const collector = id => one(`[data-history-radio-collect="${id}"]`);
  const link = id => `https://music.163.com/djradio?id=${id}`;
  const panel = () => one('[role="menu"][aria-label="播放记录操作"]:not([inert])');
  const item = key => one(`[data-menu-key="${key}"]`, panel() ?? null);
  const share = () => one('dialog.track-share-dialog[open]:not([inert])');
  const confirm = () => one('dialog[aria-labelledby="history-confirm-title"][open]:not([inert])');
  const row = id => more(id)?.closest('[role="row"]');
  const menuItems = () => all('[role="menuitem"]', panel() ?? null).map(node => node.dataset.menuKey);
  const ordinary = ['open', 'play', 'next', 'share', 'copy', 'remove'];
  const eligible = ['open', 'play', 'next', 'collect', 'share', 'copy', 'remove'];
  const stages = new Set();
  let epoch, baseline, beforeNext, step = 'startup', clipboardAttempt = 0;
  let menuTarget, menuEventHandled, menuObserver, menuMutations = [];

  async function wait(observe, label) {
    step = label;
    const deadline = performance.now() + 15_000;
    do {
      const value = await observe();
      if (value) return value;
      await sleep(60);
    } while (performance.now() < deadline);
    throw Error(label);
  }
  function click(node) {
    assert(ready(node), 'enabled control for ' + step);
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    node.click();
  }
  async function enter(kind, tab, id) {
    const target = await wait(() => all(`${tabs} button`).find(node =>
      text(node) === tab && ready(node)), 'history tab ' + tab);
    if (!target.hasAttribute('aria-current')) click(target);
    await wait(() => text(one(`${tabs} [aria-current]`)) === tab &&
      one('.history-page')?.getAttribute('aria-busy') === 'false' &&
      !one('.history-content:is(.catalog-tab-enter-active,.catalog-tab-leave-active)') &&
      ready(more(id)), 'history rows settled for ' + kind);
    const page = await history(kind);
    assert(page.kind === kind && page.remoteIncluded && page.entries.some(entry => entry.id === id),
      'right category with synthetic multi-device record');
    return page;
  }
  async function openMenu(id, expected, trigger = 'button') {
    step = 'open menu ' + id;
    menuTarget = id;
    menuEventHandled = undefined;
    menuObserver?.disconnect();
    menuMutations = [];
    menuObserver = new MutationObserver(records => {
      for (const record of records) {
        if (record.target === more(id) && record.attributeName === 'aria-expanded')
          menuMutations.push([record.oldValue, more(id).getAttribute('aria-expanded')]);
      }
    });
    menuObserver.observe(more(id), { attributes: true, attributeOldValue: true });
    if (trigger === 'contextmenu') {
      const target = row(id);
      assert(ready(target), 'visible row for context menu');
      const rect = target.getBoundingClientRect();
      menuEventHandled = !target.dispatchEvent(new MouseEvent('contextmenu', {
        bubbles: true, cancelable: true, button: 2, clientX: rect.left + 25, clientY: rect.top + 10,
      }));
    } else if (trigger === 'keyboard') {
      const target = more(id);
      assert(ready(target), 'focusable More control for keyboard menu');
      target.focus({ preventScroll: true });
      menuEventHandled = !target.dispatchEvent(new KeyboardEvent('keydown', {
        key: 'F10', shiftKey: true, bubbles: true, cancelable: true,
      }));
    } else click(more(id));
    if (trigger !== 'button') assert(menuEventHandled, 'history handled the ' + trigger + ' trigger');
    await wait(() => ready(panel()) && more(id)?.getAttribute('aria-expanded') === 'true' &&
      same(menuItems(), expected), 'visible menu with expected items for ' + id);
    menuObserver.disconnect();
    const rect = panel().getBoundingClientRect();
    assert(rect.width > 0 && rect.height > 0 && rect.left >= 0 && rect.top >= 0 &&
      rect.right <= innerWidth + 1 && rect.bottom <= innerHeight + 1,
    'menu is contained in the WebView');
    return panel();
  }
  async function closeMenu(id) {
    const focused = document.activeElement;
    assert(panel()?.contains(focused), 'menu has keyboard focus');
    focused.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await wait(() => more(id)?.getAttribute('aria-expanded') === 'false' &&
      !panel() && document.activeElement === more(id), 'escape restores More focus');
  }
  async function radioLayout(id) {
    const target = row(id);
    const controls = one('.history-radio-row-controls', target);
    await wait(() => getComputedStyle(controls).opacity === '1',
      'open radio row controls reach full opacity');
    const cover = getComputedStyle(one('.history-name-cell > :is(img, .history-cover)', target));
    const title = getComputedStyle(one('.history-name', target));
    const actions = getComputedStyle(controls);
    return {
      width: cover.width, height: cover.height, corner: cover.borderTopLeftRadius,
      title: title.fontSize, gap: actions.columnGap, visible: actions.opacity === '1',
    };
  }
  async function readClipboard(id) {
    const attempt = ++clipboardAttempt;
    await emit('history-radio-clipboard', { attempt, expected: link(id) });
    const result = await wait(() => window.__historyRadioClipboard?.attempt === attempt &&
      window.__historyRadioClipboard, 'native clipboard readback for ' + id);
    assert(result.matches, 'native clipboard contains exactly the intended public link');
    return result.matches;
  }
  async function capture(name, data = {}, playback = 'paused') {
    step = name;
    assert(!stages.has(name), 'unique screenshot stage');
    stages.add(name);
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const state = await player();
    const nativeQueue = await queue();
    assert(state.queueRevision === nativeQueue.revision && state.entryId === nativeQueue.current,
      'coherent native playback and queue snapshots');
    if (playback === 'paused') assert(same(decoder(state), baseline), 'paused decoder or clock changed');
    else assert(state.state === 'playing' && state.track?.voiceId === '821' && state.position > .25,
      'selected audiobook first chapter is decoding');
    await emit(`ux_${String(stages.size).padStart(2, '0')}_${name}`, {
      ...data, queue: voiceIds(nativeQueue), checkPausedPlayback: playback === 'paused',
      playing: playback === 'playing',
    });
  }
  async function route(id, title, firstEpisode) {
    click(item('open'));
    await wait(() => text(one('.podcast-cover h2')) === title &&
      one('.podcast-page')?.textContent.includes(firstEpisode), 'open radio detail ' + id);
    await capture(`opened_${id}`, { id, title: text(one('.podcast-cover h2')), firstEpisode });
    click(await wait(() => ready(one('button[aria-label="返回上一页"]')) &&
      one('button[aria-label="返回上一页"]'), 'main history back button'));
  }

  try {
    epoch = await wait(async () => {
      const account = await invoke('account_snapshot');
      return account.status === 'authenticated' && account.epoch;
    }, 'synthetic account authenticated');
    click(await wait(() => all('.sidebar button').find(node =>
      text(node) === '最近播放' && ready(node)), 'recently played sidebar entry'));
    await enter('audiobook', '有声书', '84');
    await action({ kind: 'mode', mode: 'single_loop' });
    step = 'existing audiobook history chapter';
    click(one('.history-chapter button', row('84')));
    await wait(async () => {
      const state = await player();
      return state.state === 'playing' && state.track?.voiceId === '842' && state.position > .25;
    }, 'real playback from an independent audiobook');
    await action({ kind: 'pause', paused: true });
    let sample, stableSince;
    baseline = await wait(async () => {
      const state = await player();
      if (state.state !== 'paused' || state.track?.voiceId !== '842' ||
          !Number.isFinite(state.position) || state.position <= .25) return;
      const current = { ...decoder(state), queueRevision: state.queueRevision };
      if (!same(current, sample)) {
        sample = current;
        stableSince = performance.now();
        return;
      }
      if (performance.now() - stableSince < 450) return;
      beforeNext = state.queueRevision;
      return decoder(state);
    }, 'same paused native decoder and clock for 450 ms');
    await capture('baseline');

    await enter('podcast', '播客', '81');
    await openMenu('83', ordinary, 'contextmenu');
    await capture('own_podcast_menu', { id: '83', items: menuItems(), trigger: 'contextmenu' });
    await closeMenu('83');
    await openMenu('81', eligible);
    await capture('podcast_menu', { id: '81', items: menuItems(), layout: await radioLayout('81') });
    await route('81', '合成历史播客', '播客第一集');

    await enter('podcast', '播客', '81');
    await openMenu('81', eligible);
    click(item('collect'));
    await wait(() => collector('81')?.getAttribute('aria-pressed') === 'true', 'podcast collected');
    await openMenu('81', ordinary);
    await capture('collected_podcast_menu', {
      id: '81', items: menuItems(), collected: collector('81').getAttribute('aria-pressed'),
    });
    click(item('share'));
    await wait(() => ready(share()) && text(one('h2', share())) === '分享播客' &&
      text(one('strong', share())) === '合成历史播客' &&
      one('input[readonly]', share())?.value === link('81'), 'share is a local public-link dialog');
    await capture('share_dialog', {
      id: '81', title: text(one('h2', share())), publicLink: one('input[readonly]', share()).value,
    });
    click(one('button.primary', share()));
    await wait(() => text(one('[role="status"]', share())) === '链接已复制', 'share dialog copy notice');
    await capture('share_copied', { id: '81', clipboardMatches: await readClipboard('81') });
    click(one('button[aria-label="关闭分享"]', share()));
    await wait(() => !share(), 'share dialog closed');

    await openMenu('81', ordinary);
    click(item('next'));
    await wait(async () => {
      const state = await player(), tracks = await queue();
      return state.queueRevision !== beforeNext && state.queueRevision === tracks.revision &&
        same(voiceIds(tracks), ['841', '842', '811', '812']);
    }, 'Next inserts the podcast after the existing decoder');
    await capture('podcast_next', { id: '81' });

    await enter('audiobook', '有声书', '82');
    await openMenu('82', eligible, 'keyboard');
    await capture('book_menu', {
      id: '82', items: menuItems(), trigger: 'shift-f10', layout: await radioLayout('82'),
    });
    click(item('copy'));
    await wait(() => text(one('.history-radio-menu-feedback')) === '链接已复制', 'direct copy notice');
    await capture('book_copied', { id: '82', clipboardMatches: await readClipboard('82') });
    await openMenu('82', eligible);
    await route('82', '合成历史有声书', '有声书第一章');

    await enter('audiobook', '有声书', '82');
    await openMenu('82', eligible);
    click(item('play'));
    await wait(async () => {
      const state = await player(), tracks = await queue();
      return state.state === 'playing' && state.playId !== baseline.playId &&
        state.track?.voiceId === '821' && state.track?.id === '1821' && state.position > .25 &&
        same(voiceIds(tracks), ['821', '822']);
    }, 'Play replaces the queue and decodes the selected audiobook');
    await capture('book_play', { id: '82' }, 'playing');

    // Stop the clock without touching the selected entry. The subsequent
    // menu confirmation must not alter the audiobook decoder or queue.
    await action({ kind: 'pause', paused: true });
    sample = undefined;
    baseline = await wait(async () => {
      const state = await player();
      if (state.state !== 'paused' || state.track?.voiceId !== '821' || state.position <= .25) return;
      const current = decoder(state);
      if (!same(current, sample)) {
        sample = current;
        stableSince = performance.now();
        return;
      }
      return performance.now() - stableSince >= 450 && current;
    }, 'stable paused audiobook after Play');
    const previous = await enter('podcast', '播客', '81');
    await openMenu('81', ordinary);
    click(item('remove'));
    await wait(() => ready(confirm()) && text(one('h2', confirm())) === '删除 1 条播放记录？' &&
      text(document.activeElement) === '取消', 'safe removal confirmation');
    await capture('removal_confirmation', {
      id: '81', title: text(one('h2', confirm())), actions: all('footer button', confirm()).map(text),
      focus: text(document.activeElement), description: text(one('p', confirm())),
    });
    click(one('button.secondary', confirm()));
    await wait(() => !confirm() && ready(more('81')), 'cancel leaves the podcast row');
    assert((await history('podcast')).revision === previous.revision,
      'cancel leaves the exact native history revision');
    await capture('removal_cancelled', { id: '81', rowRetained: true });
    await openMenu('81', ordinary);
    click(item('remove'));
    await wait(() => ready(confirm()), 'explicit removal reopened');
    click(one('button.primary', confirm()));
    await wait(async () => !confirm() && !more('81') && ready(more('83')) &&
      !(await history('podcast')).entries.some(entry => entry.id === '81'),
    'only the selected podcast history row is gone');
    await capture('removal_confirmed', { id: '81', remaining: (await history('podcast')).entries.map(entry => entry.id) });
    await emit('complete', { ok: true, stageCount: stages.size, checkPausedPlayback: true });
  } catch (error) {
    await emit('failed', {
      ok: false, step, error: String(error?.message ?? error),
      menu: {
        target: menuTarget, handled: menuEventHandled, expanded: more(menuTarget)?.getAttribute('aria-expanded'),
        items: menuItems(), expandedChanges: menuMutations, busy: one('.history-page')?.getAttribute('aria-busy'),
        notices: all('.history-page [role="alert"]').map(text),
        focus: document.activeElement?.getAttribute('aria-label') ?? text(document.activeElement)?.slice(0, 50),
      },
    });
  }
})();
