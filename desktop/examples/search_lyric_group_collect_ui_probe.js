// Real Lyrics group menu, existing native collection picker and status-only readback.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])];
  const text = (node) => node?.textContent.trim() ?? null;
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, facts = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...facts },
  });
  const assert = (condition, message) => { if (!condition) throw Error(message); };
  const live = (node) => node?.isConnected && node.getClientRects().length &&
    !node.closest('[inert]') && !node.matches(':disabled,[aria-disabled="true"]');
  const row = (index) => one(`#search-result-panel [data-lyric-index="${index}"] .track-row`);
  const more = (index) => one('[data-result-action="more"]', row(index));
  const menu = () => one('[role="menu"][aria-label="歌曲操作"]:not([inert])');
  const action = (name) => one(`[data-menu-key="${name}"]`, menu() ?? null);
  const picker = () => one('dialog[data-lyric-group-collect][open]:not(.modal-leave-active)');
  const anyPicker = () => one('dialog[data-lyric-group-collect][open]');
  const settledPicker = () => {
    const dialog = picker();
    if (!live(dialog) || dialog.matches('.modal-enter-active,.modal-enter-from') ||
      one('.collect-step-enter-active,.collect-step-leave-active', dialog)) return false;
    const style = getComputedStyle(dialog);
    return style.opacity === '1' && style.transform === 'none' &&
      style.visibility === 'visible' && dialog.getAnimations().every((animation) =>
        !animation.pending && animation.playState !== 'running');
  };
  const destination = () => one('[data-playlist-id="81"]', picker() ?? null);
  const destinations = () => all('[data-playlist-id]', picker() ?? null).map((node) => node.dataset.playlistId);
  const feedback = () => one('[data-search-lyric-collect-status]');
  const unknown = () => one('.mutation-result[data-phase="uncertain"]', picker() ?? null);
  const reconcile = () => one('button', unknown() ?? null);
  const selected = () => all('#search-result-panel .track-row[aria-selected="true"]')
    .map((node) => Number(node.closest('[data-lyric-index]').dataset.lyricIndex));
  const matches = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const noDownloadPicker = () => !one('dialog[open][data-search-group-download-picker]');
  let step = 'startup';
  let stages = 0;
  async function wait(check, label) {
    step = label;
    const deadline = performance.now() + 16_000;
    do {
      const value = await check();
      if (value) return value;
      await sleep(55);
    } while (performance.now() < deadline);
    throw Error(label);
  }
  function click(node) {
    assert(live(node), 'enabled actual control during ' + step);
    node.focus({ preventScroll: true });
    node.click();
  }
  function mouse(node, type, flags = {}) {
    return node.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, ...flags }));
  }
  function addSelection(index) {
    const target = row(index);
    assert(live(target), 'mounted lyric row ' + index);
    mouse(target, 'mousedown', { button: 0, buttons: 1, ctrlKey: true });
    mouse(target, 'mouseup', { button: 0, ctrlKey: true });
    mouse(target, 'click', { button: 0, ctrlKey: true });
  }

  const anchorsByReceipt = new Map();
  const preparations = [], downloads = [], choices = [];
  const planReceipts = [];
  const cancelledPlans = new Set();
  let operation;
  let statusReads = 0;
  try {
    await wait(() => live(one('.track-row')), 'daily music and signed-in account');
    const { epoch } = await invoke('account_snapshot');
    const provides = one('#app').__vue_app__._context.provides;
    const core = provides[Reflect.ownKeys(provides).find((k) =>
      typeof k === 'symbol' && k.description === 'core-client')];
    assert(typeof core?.lyricGroupCollectPrepare === 'function', 'actual typed collection adapter');
    function tap(name, observer) {
      const original = core[name].bind(core);
      core[name] = (...args) => observer(original, args);
    }
    tap('searchResults', async (original, args) => {
      const reply = await original(...args);
      if (args[1].category === 'lyrics' && args[0] === epoch && reply.epoch === epoch) {
        const items = reply.value.sections.find((section) => section.category === 'lyrics')?.items ?? [];
        items.forEach((item, index) => {
          if (item.lyricGroupCollectionReceipt)
            anchorsByReceipt.set(item.lyricGroupCollectionReceipt, {
              id: item.track.id, ordinal: args[1].offset + index,
            });
        });
      }
      return reply;
    });
    tap('lyricGroupCollectPrepare', async (original, args) => {
      const anchor = anchorsByReceipt.get(args[1]);
      const rows = args[2].map((receipt) => anchorsByReceipt.get(receipt));
      assert(args.length === 3 && args[0] === epoch && anchor && rows.every(Boolean),
        'exact Lyrics source receipts passed to scoped prepare');
      preparations.push({ anchor, rows });
      const reply = await original(...args);
      if (reply.epoch === epoch && reply.value.receipt) planReceipts.push(reply.value.receipt);
      return reply;
    });
    tap('searchGroupDownload', (original, args) => {
      assert(args[0] === epoch && args[1].kind === 'preview_lyrics_anchor' && args[1].anchor.trackId === '104',
        'no download prepare, choice, status or start through collection');
      downloads.push({ kind: args[1].kind, trackId: args[1].anchor.trackId });
      return original(...args);
    });
    tap('downloadCollectCancel', async (original, args) => {
      const reply = await original(...args);
      if (args[0] === epoch && args[1] === planReceipts[0]) cancelledPlans.add(0);
      return reply;
    });
    tap('downloadCollectExisting', async (original, args) => {
      assert(args[0] === epoch && args[1] === planReceipts[1] && args[2] === '81',
        'exact prepared group plan and chosen ordinary destination');
      choices.push(args[2]);
      const reply = await original(...args);
      operation = reply.value?.operationId;
      return reply;
    });
    tap('reconcilePlaylist', (original, args) => {
      assert(args[0] === epoch && typeof operation === 'string' && args[1] === operation,
        'read back the same uncertain operation');
      statusReads++;
      return original(...args);
    });

    const queue = () => invoke('player_queue', { epoch });
    const player = () => invoke('player_snapshot', { epoch });
    const before = await player();
    const initialQueue = await queue();
    assert(before.state === 'idle' && initialQueue.tracks.length === 0, 'empty native playback baseline');
    async function capture(name, facts = {}) {
      const now = await player(), currentQueue = await queue();
      assert(noDownloadPicker() && now.state === 'idle' && now.playId === before.playId &&
        currentQueue.revision === initialQueue.revision && currentQueue.tracks.length === 0,
        'collection does not start playback, download or alter the queue');
      const dialog = picker();
      let visual = {};
      if (dialog) {
        await wait(settledPicker, 'painted collection picker ' + name);
        const rect = dialog.getBoundingClientRect();
        assert(Math.round(rect.width) === Math.min(442, innerWidth - 48) &&
          rect.left >= 0 && rect.top >= 0 && rect.right <= innerWidth && rect.bottom <= innerHeight,
        'recovered dialog width and viewport containment');
        visual = { ratio: devicePixelRatio, screenX, screenY, dialogs: [{
          open: true, opacity: '1', rect: rect.toJSON(), background: getComputedStyle(dialog).backgroundColor,
        }] };
      }
      stages++;
      await emit(`ux_${String(stages).padStart(2, '0')}_${name}`, {
        label: name, selectedOrdinals: selected(), preparations: [...preparations], choices: [...choices],
        downloadIpc: [...downloads], statusReads, firstCancelled: cancelledPlans.has(0),
        queueUnchanged: true, ...facts, ...visual,
      });
    }

    const field = one('[aria-label="搜索音乐"]');
    field.value = '合成连续单曲';
    field.dispatchEvent(new Event('input', { bubbles: true }));
    field.form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await wait(() => one('.search-results-page h1')?.textContent === '合成连续单曲' &&
      one('#search-result-panel')?.getAttribute('aria-busy') === 'false', 'exact search visit');
    click(one('[data-category="lyrics"]'));
    await wait(() => one('[data-category="lyrics"]')?.getAttribute('aria-selected') === 'true' &&
      one('#search-result-panel')?.getAttribute('aria-busy') === 'false' &&
      [0, 2, 3, 4].every((index) => row(index)), 'visible dedicated Lyrics rows');
    addSelection(4);  // second occurrence of 103, then 101, unlike visual order
    addSelection(0);
    await wait(() => matches(selected(), [0, 2, 4]), 'duplicate Lyrics hits share Song selection');

    async function openMenu() {
      const anchor = more(3);
      assert(live(anchor) && row(3)?.dataset.trackId === '104', 'exact unselected lyric More anchor');
      anchor.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'instant' });
      anchor.focus({ preventScroll: true });
      let previous, since;
      await wait(() => {
        if (!live(anchor) || document.activeElement !== anchor || anchor !== more(3)) return false;
        const b = anchor.getBoundingClientRect();
        const bounds = [b.left, b.top, b.width, b.height].join(',');
        if (bounds !== previous) { previous = bounds; since = performance.now(); return false; }
        return performance.now() - since >= 90 &&
          anchor.contains(document.elementFromPoint(b.left + b.width / 2, b.top + b.height / 2));
      }, 'stable hittable clicked Lyric More');
      const target = row(3), b = target.getBoundingClientRect();
      mouse(target, 'mousedown', { button: 2, buttons: 2, ctrlKey: true });
      mouse(target, 'contextmenu', { button: 2, ctrlKey: true, clientX: b.left + 22, clientY: b.top + 12 });
      mouse(target, 'mouseup', { button: 2, ctrlKey: true });
      await wait(() => live(menu()) && menu().style.visibility === 'visible' &&
        anchor.getAttribute('aria-expanded') === 'true' && live(action('collect')),
      'visible group 收藏 for exact unselected anchor');
      assert(action('play')?.disabled && action('next')?.disabled &&
        !action('comments') && !action('share') && matches(selected(), [0, 2, 4]),
      'modified outsider cannot fall back to any single-song action');
      return anchor;
    }
    async function openPicker() {
      click(action('collect'));
      await wait(() => !menu() && settledPicker() && live(destination()) &&
        matches(destinations(), ['81']),
      'existing scoped collection picker loaded');
      const title = text(one('h2', picker())), summary = text(one('.collect-selection-summary', picker()));
      assert(title === '收藏到歌单' && summary === '所选歌词结果共 2 首，将收藏 2 首。',
        'the picker represents only the selected Songs');
      return { title, summary, destinations: destinations() };
    }

    const anchor = await openMenu();
    await capture('lyric_collect_menu', {
      clicked: '104', menuKeys: all('button[data-menu-key]', menu()).map((node) => node.dataset.menuKey),
    });
    await capture('lyric_collect_picker', await openPicker());
    click(one('[aria-label="关闭歌单操作"]', picker()));
    await wait(() => !anyPicker() && document.activeElement === anchor && cancelledPlans.has(0),
      'cancel retires the first plan and restores clicked focus');
    await capture('lyric_collect_cancelled', { focusRestored: true });
    const reopened = await openMenu();
    await capture('lyric_collect_second_picker', await openPicker());

    click(destination());
    await wait(() => live(reconcile()) && text(reconcile()) === '核对结果' &&
      choices.length === 1 && typeof operation === 'string', 'lost write receipt leaves a readback-only choice');
    await capture('lyric_collect_uncertain', { phase: unknown().dataset.phase, statusAction: text(reconcile()) });
    click(reconcile());
    await wait(() => !anyPicker() && document.activeElement === reopened &&
      text(feedback()) === '已收藏到歌单。' && statusReads === 1, 'status-only read confirms and restores focus');
    await capture('lyric_collect_confirmed', { focusRestored: true, feedback: text(feedback()) });
    await emit('complete', { ok: true, stageCount: stages, statusReads, queueUnchanged: true,
      realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', { step, stageCount: stages, message: String(error) });
  }
})();
