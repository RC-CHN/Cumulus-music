// Real My Purchases actions through production Tauri IPC and a sealed libmpv null source.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])];
  const button = (name, root = document) => all('button', root).find((node) =>
    node.getAttribute('aria-label') === name || node.textContent.trim() === name);
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, facts = {}) => invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...facts } });
  const settle = () => !one('.page-enter-active,.page-leave-active,.modal-enter-active,.modal-leave-active');
  const ready = (node) => !!node?.getClientRects().length && !node.disabled && !node.closest('[inert]');
  const frame = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (pass, note) => { if (!pass) throw Error(note); };
  const same = (first, second) => JSON.stringify(first) === JSON.stringify(second);
  const list = () => one('[aria-label="已购播单和单集"]');
  const row = (kind, id) => one(`[data-purchased-kind="${kind}"][data-purchased-id="${id}"]`, list());
  const rowOrder = () => all('[data-purchased-kind]', list()).map((node) =>
    `${node.dataset.purchasedKind}:${node.dataset.purchasedId}`);
  const title = (id) => button(`合成已购单集 ${id}`, row('episode', id));
  const bar = () => one('.playerbar')?.dataset.state;
  const selections = [], preparations = [], directSelections = [];
  let checkpoint = 'bootstrap', count = 0, epoch, purchasedPage, captured;
  const player = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  async function wait(predicate, label, budget = 12000) {
    checkpoint = label;
    const until = performance.now() + budget;
    while (performance.now() < until) {
      const value = await predicate();
      if (value) return value;
      await sleep(30);
    }
    throw Error(label);
  }
  async function hit(find, label) {
    const node = await wait(() => ready(find()) && settle() && find(), label);
    node.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    await frame();
    const point = await wait(() => {
      if (!ready(node) || node !== find() || !settle()) return false;
      const r = node.getBoundingClientRect();
      const x = r.left + r.width / 2, y = r.top + r.height / 2;
      return node.contains(document.elementFromPoint(x, y)) && { x, y };
    }, label + ' hit target');
    return { node, ...point };
  }
  async function click(find, label) {
    const { node } = await hit(find, label);
    node.focus({ preventScroll: true });
    node.click();
  }
  async function doubleClick(find, label, episode = true) {
    const { node, x, y } = await hit(find, label);
    if (episode) {
      assert(node.getAttribute('aria-keyshortcuts') === 'Enter', 'available program has keyboard playback');
      node.focus({ preventScroll: true });
      for (const detail of [1, 2]) {
        node.dispatchEvent(new MouseEvent('click', { detail, button: 0, bubbles: true, cancelable: true,
          clientX: x, clientY: y }));
      }
    }
    node.dispatchEvent(new MouseEvent('dblclick', { detail: 2, button: 0, bubbles: true, cancelable: true,
      clientX: x, clientY: y }));
  }
  function identity(view) {
    return { state: view.state, song: view.track?.id ?? null, voice: view.track?.voiceId ?? null,
      playId: view.playId, entryId: view.entryId, queueRevision: view.queueRevision };
  }
  async function observation() {
    const view = await player(), rows = await queue();
    assert(view.queueRevision === rows.revision && rows.current === view.entryId, 'coherent native queue and player');
    return { ...identity(view), kind: view.resourceKind, position: view.position, mode: view.mode,
      current: rows.current, rows: rows.tracks.map(({ entryId, track }) =>
        ({ entryId, song: track.id, voice: track.voiceId ?? null })) };
  }
  function samePlayback(first, second) {
    return ['song', 'voice', 'playId', 'entryId', 'queueRevision', 'current', 'mode', 'kind']
      .every((key) => first[key] === second[key]) && same(first.rows, second.rows);
  }
  async function stage(name, facts = {}) {
    await wait(settle, 'settle ' + name);
    await frame();
    const label = `ux_${String(++count).padStart(2, '0')}_${name}`;
    captured = await observation();
    assert(captured.state === bar(), 'actual player bar reflects native state at ' + name);
    window.__podcastProgressStage = '';
    await emit(label, { ...facts, actual: document.visibilityState === 'visible', visibleRows: rowOrder(),
      native: captured, pageRevision: purchasedPage.revision, selections: selections.slice(),
      preparations: preparations.slice(), directCount: directSelections.length,
      screenX, screenY, ratio: devicePixelRatio });
    await wait(() => window.__podcastProgressStage === label, 'host stage ' + name);
    return captured;
  }
  async function expectSelection(number, id) {
    const call = await wait(() => selections[number - 1], 'actual prepared selection acknowledgement ' + number);
    assert(selections.length === number && preparations.length === number && directSelections.length === 0,
      'one prepared selection per double click, no unscoped selection');
    assert(call.owner === epoch && call.kind === 'podcast_purchased_episode' &&
      same(call.target, { kind: 'episode', id, revision: purchasedPage.revision }),
    'native received the exact clicked program kind, identity, and page');
    return call;
  }
  try {
    const account = await wait(async () => {
      const value = await invoke('account_snapshot');
      return value.status === 'authenticated' && value;
    }, 'sealed account');
    epoch = account.epoch;
    const provides = await wait(() => one('#app')?.__vue_app__?._context.provides, 'mounted production client');
    const core = provides[Reflect.ownKeys(provides).find((key) =>
      typeof key === 'symbol' && key.description === 'core-client')];
    const observe = (name, onCall, onValue = () => {}, onError = () => {}) => {
      const native = core[name];
      core[name] = async (...args) => {
        onCall(args);
        try {
          const value = await native.apply(core, args);
          onValue(args, value);
          return value;
        } catch (error) {
          onError(args, error);
          throw error;
        }
      };
    };
    observe('podcastPage', () => {}, ([owner, query], value) => {
      if (owner === epoch && query.scope.kind === 'purchased' && value.purchases) purchasedPage = value;
    });
    observe('playSelection', ([owner, selection]) => directSelections.push({ owner, kind: selection.kind }));
    observe('preparePlaySelection', ([owner, queueRevision, playId]) =>
      preparations.push({ owner, queueRevision, playId }));
    observe('commitPreparedSelection', () => {}, ([owner, , { kind, target, menu }], value) =>
      selections.push({ owner, kind, target, ...(menu ? { menu } : {}), reply: identity(value) }),
    ([owner, , { kind, target, menu }], error) => selections.push({ owner, kind, target,
      ...(menu ? { menu } : {}), error: error?.code }));

    await click(() => button('我的播客', one('.sidebar-navigation[aria-label="我的"]')), 'open My Podcasts');
    await click(() => button('我的购买', one('.podcast-tabs')), 'open My Purchases');
    await wait(() => purchasedPage && rowOrder().join(',') === 'radio:81,episode:81,episode:82',
      'native bought page and exact mixed rows');
    const preference = await invoke('queue_click_preferences', { epoch });
    assert(preference.epoch === epoch && preference.value.replaceQueue && !preference.value.damaged,
      'ordinary double click would replace by default for this account');
    // Start at the tail of a sequential queue so neither load schedules a
    // separate native prefetch. In SingleLoop the natural next is the current
    // row, including a freshly authorized source outside the shared cache.
    await invoke('player_action', { epoch, action: { kind: 'mode', mode: 'sequential' } });
    await invoke('play_selection', { epoch, selection: { kind: 'songs', ids: ['1', '2', '3'], index: 2,
      quality: 'standard' } });
    await wait(async () => {
      const view = await player();
      return view.state === 'playing' && view.track?.id === '3' && view.position > .2 && bar() === 'playing';
    }, 'real native catalog decoder playing');
    await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    await wait(async () => (await player()).state === 'paused' && bar() === 'paused', 'real baseline paused');
    await click(() => title('81'), 'single-click on episode only selects');
    assert(title('81')?.getAttribute('aria-pressed') === 'true' &&
      preparations.length === 0 && directSelections.length === 0, 'single click starts no playback command');
    const baseline = await stage('baseline', { replaceQueue: preference.value.replaceQueue, singleClickSelected: true });
    assert(baseline.kind === 'catalog' && baseline.song === '3' && baseline.mode === 'sequential' &&
      baseline.rows.map((entry) => entry.song).join(',') === '1,2,3', 'three-entry native baseline');

    await doubleClick(() => one('.purchase-placeholder', row('radio', '81')), 'double-click same-ID radio body', false);
    assert(preparations.length === 0 && directSelections.length === 0 && !one('.podcast-summary'),
      'the same-ID radio body does not trigger a playback action');
    const ticket = await invoke('prepare_play_selection', {
      epoch, expectedQueueRevision: baseline.queueRevision, expectedPlayId: baseline.playId,
    });
    const radioError = await invoke('commit_prepared_selection', {
      epoch, ticket, selection: { kind: 'podcast_purchased_episode',
        target: { kind: 'radio', id: '81', revision: purchasedPage.revision } },
    }).then(() => null, (error) => error?.code);
    assert(radioError === 'invalid_input', 'native rejects the same-ID radio target');
    const radio = await stage('radio_rejected', { radioError });
    assert(radio.state === 'paused' && samePlayback(baseline, radio), 'radio leaves old queue and decoder');

    await doubleClick(() => title('81'), 'double-click denied purchased program');
    const denial = await expectSelection(1, '81');
    assert(denial.error === 'unavailable' && !denial.reply, 'native known rights denial');
    const denied = await stage('denied');
    assert(denied.state === 'paused' && samePlayback(baseline, denied), 'denied program leaves old queue and decoder');

    await doubleClick(() => title('82'), 'double-click playable purchased program');
    assert((await expectSelection(2, '82')).reply, 'allowed native selection was committed');
    await wait(async () => {
      const view = await player();
      return view.state === 'playing' && view.track?.id === '182' && view.track?.voiceId === '82' &&
        view.position > .2 && bar() === 'playing';
    }, 'real native purchased program clock');
    const inserted = await stage('inserted');
    assert(inserted.kind === 'voice' && inserted.playId !== baseline.playId &&
      inserted.queueRevision !== baseline.queueRevision &&
      same(inserted.rows, [...baseline.rows, { entryId: inserted.entryId, song: '182', voice: '82' }]),
    'only the clicked program is inserted after the old current, with all old entries retained');
    await wait(() => title('82')?.getAttribute('aria-current') === 'true' &&
      !title('81')?.hasAttribute('aria-current') &&
      document.querySelectorAll('.purchase-row.current').length === 1,
      'only the current purchased episode receives the playing accent');

    await doubleClick(() => title('82'), 'double-click same program to pause');
    assert((await expectSelection(3, '82')).reply?.playId === inserted.playId, 'pause acknowledges the same play ID');
    await wait(async () => (await player()).state === 'paused' && bar() === 'paused', 'same purchased program paused');
    const paused = await stage('paused');
    assert(samePlayback(inserted, paused) && paused.position >= inserted.position, 'pause retains decoder identity');

    await doubleClick(() => title('82'), 'double-click same program to resume');
    assert((await expectSelection(4, '82')).reply?.playId === inserted.playId, 'resume acknowledges the same play ID');
    await wait(async () => {
      const view = await player();
      return view.state === 'playing' && view.position > paused.position + .1 && bar() === 'playing';
    }, 'same purchased program clock resumes');
    const resumed = await stage('resumed');
    assert(samePlayback(inserted, resumed), 'resume preserves the exact program and queue');

    const menuFor = (id) => one(`[role="menu"][aria-label="单集菜单：合成已购单集 ${id}"]:not([inert]):not(.menu-leave-active)`);
    const item = (id, key) => one(`[data-menu-key="${key}"]`, menuFor(id));
    const more = (id) => one('[data-purchased-menu]', row('episode', id));
    const opened = async (id) => wait(() => {
      const panel = menuFor(id);
      return panel && ready(item(id, 'play')) && settle() &&
        !panel.classList.contains('menu-enter-active') && getComputedStyle(panel).opacity === '1' && panel;
    }, 'painted purchased menu ' + id);
    title('82').focus();
    title('82').dispatchEvent(new KeyboardEvent('keydown', { key: 'F10', shiftKey: true, bubbles: true, cancelable: true }));
    const firstMenu = await opened('82');
    const menuKeys = all('[data-menu-key]', firstMenu).map((node) => node.dataset.menuKey);
    assert(menuKeys.join(',') === 'play,next,comments,download,share,copy' && !item('82','download').disabled,
      'official single-program menu order and download state');
    await stage('menu', { menuKeys });
    await click(() => item('82','play'), 'play in menu while current voice is already playing');
    assert((await expectSelection(5, '82')).reply?.playId === inserted.playId, 'menu Play retained source');
    const menuPlaying = await stage('menu_playing');
    assert(menuPlaying.state === 'playing' && samePlayback(resumed, menuPlaying), 'menu Play cannot pause');

    await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    await wait(async () => (await player()).state === 'paused' && bar() === 'paused', 'same episode paused before menu Play');
    await click(() => more('82'), 'open purchased menu by button');
    await opened('82');
    await click(() => item('82','play'), 'resume the paused episode from the menu');
    assert((await expectSelection(6, '82')).reply?.playId === inserted.playId, 'menu resume retains source');
    await wait(async () => (await player()).state === 'playing' && bar() === 'playing', 'menu resumes same episode');
    assert(samePlayback(resumed, await stage('menu_resumed')), 'menu resume leaves entries intact');

    await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    const nextBefore = await wait(async () => {
      const view = await player();
      return view.state === 'paused' && view.mode === 'sequential' && view;
    }, 'native episode paused before Next');
    await click(() => more('81'), 'open the other episode without selecting its same-ID radio');
    await opened('81');
    await click(() => item('81','next'), 'queue only the exact other purchased episode');
    await wait(async () => (await queue()).tracks.some((entry) => entry.track.voiceId === '81'), 'native next row 81');
    const next = await stage('menu_next', { previous: identity(nextBefore) });
    assert(next.state === 'paused' && next.playId === nextBefore.playId && next.entryId === nextBefore.entryId &&
      next.mode === 'sequential' && next.rows.map((entry) => entry.song).join(',') === '1,2,3,182,181',
      'Next adds one program without starting or replacing the paused decoder');

    await click(() => more('82'), 'open sharing for the current episode');
    await opened('82');
    await click(() => item('82','share'), 'share the program public link');
    const share = await wait(() => all('dialog[open] input').find((node) =>
      node.value === 'https://music.163.com/dj?id=82'), 'public episode link without credentials');
    await stage('menu_share', { link: share.value });
    await emit('complete', { ok: true, stageCount: count, realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', { checkpoint, error: error?.message || String(error), visibleRows: rowOrder(),
      preparations, selections, directSelections, captured, body: document.body.innerText.slice(-1500) });
  }
})();
