// Actual Vue + IPC + core coordinator + libmpv/null. Only fake API and media.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const invoke = (...args) =>
    (window.__CUMULUS_PROOF_INVOKE__ ?? window.__TAURI_INTERNALS__.invoke)(...args);
  const emit = (stage, value = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...value } });
  const assert = (condition, message) => {
    if (!condition) throw Error(message);
  };
  const wait = async (predicate, message) => {
    for (let i = 0; i < 220; i++) {
      if (await predicate()) return;
      await sleep(60);
    }
    throw Error(message);
  };
  const button = (label, root = document) =>
    [...root.querySelectorAll('button')].find(
      (node) => node.getAttribute('aria-label') === label || node.textContent.trim() === label,
    );
  const click = (node) => {
    assert(node && !node.disabled, 'enabled button: ' + node?.outerHTML);
    node.focus({ preventScroll: true });
    node.click();
  };
  const rect = (node) => {
    const r = node.getBoundingClientRect();
    return { left: r.left, top: r.top, right: r.right, bottom: r.bottom, height: r.height };
  };
  const bar = () => one('.playerbar');
  const panel = () => one('.queue-panel');
  const toggle = () => one('button[role=switch][data-queue-more-toggle]');
  const divider = () => one('[data-queue-more-divider]');
  const toggleAvailable = () => toggle()?.getAttribute('aria-disabled') === 'false';
  const ids = (q) => q.tracks.map(({ track }) => String(track.id));
  const entryFor = (q, id) => q.tracks.find((row) => String(row.track.id) === id)?.entryId;
  const row = (entry) => all('.queue-row[data-entry-id]').find((el) => el.dataset.entryId === entry);
  let stageCount = 0, fixtureSequence = 0, last = 'startup', client, epoch, initialPlayId;
  const actionCalls = [];
  window.__queueMoreFixtureAck = {};
  async function fixture(mode) {
    const sequence = ++fixtureSequence;
    await emit('queue-more-fixture', { sequence, mode });
    await wait(() => sequence in window.__queueMoreFixtureAck, 'fixture acknowledgment: ' + mode);
    const result = window.__queueMoreFixtureAck[sequence];
    delete window.__queueMoreFixtureAck[sequence];
    return result;
  }
  const queue = () => client.playerQueue(epoch);
  const more = async () => (await queue()).moreRecommendations;
  async function open() {
    click(button('播放队列', bar()));
    await wait(() => panel() && one('.queue-scroll') && !one('.queue-panel-enter-active'), 'drawer ready');
  }
  async function close() {
    click(button('关闭播放队列', panel()));
    await wait(() => !panel(), 'drawer closed');
  }
  async function resize(width, height) {
    await emit('resize', { width, height });
    await sleep(160);
    if (innerWidth !== width || innerHeight !== height) {
      await emit('resize', { width: width * width / innerWidth, height: height * height / innerHeight });
    }
    await wait(() => innerWidth === width && innerHeight === height, 'viewport size');
  }
  async function pause() {
    click(button('暂停', bar()));
    await wait(() => bar().dataset.state === 'paused', 'native paused');
  }
  async function seek(seconds) {
    const input = one('[aria-label="播放进度"]');
    assert(input && !input.disabled, 'native seek control');
    input.value = String(seconds);
    input.dispatchEvent(new Event('change', { bubbles: true }));
    await wait(
      () => Math.abs(Number(bar().dataset.position) - seconds) < .3 && !input.disabled,
      'native seek to ' + seconds,
    );
  }
  async function capture(label, snapshot = {}) {
    last = label;
    await sleep(250);
    const p = panel();
    const r = p ? rect(p) : null;
    const q = await queue();
    const line = divider();
    const anchor = line?.closest('.queue-row');
    const allRows = all('.queue-row');
    const stage = 'ux_' + String(++stageCount).padStart(2, '0') + '_' + label;
    if (p) {
      assert(r.left >= 0 && r.right <= innerWidth && r.bottom <= rect(bar()).top + 1, 'drawer contained');
      assert(allRows.length <= 5, 'bounded rendered rows');
    }
    if (anchor) {
      assert(anchor.dataset.entryId === q.moreRecommendations.separatorBefore, 'DOM anchors native separator');
      assert(Math.abs(rect(anchor).height - 74) <= 1, 'official 74px separator row');
      assert(allRows.filter((n) => n !== anchor).every((n) => Math.abs(rect(n).height - 64) <= 1),
        'other rows stay at 64px');
    }
    window.__queueSelectionRecorded = null;
    await emit(stage, {
      width: innerWidth, height: innerHeight, ratio: devicePixelRatio, screenX, screenY,
      visible: document.visibilityState === 'visible', focused: document.hasFocus(),
      playId: bar().dataset.playId ?? null, trackId: bar().dataset.trackId ?? null,
      position: Number(bar().dataset.position), model: q.moreRecommendations,
      rows: allRows.map((n) => ({ entryId: n.dataset.entryId, height: rect(n).height, badge: !!n.querySelector('[data-queue-more-badge]') })),
      toggle: toggle()?.getAttribute('aria-checked') ?? null, anchor: anchor?.dataset.entryId ?? null,
      actionCalls: actionCalls.slice(), initialPlayId, ...snapshot,
      captureHandshake: true,
      paintSamples: p ? [{ x: r.left + 4, y: r.top + 4, color: getComputedStyle(p).backgroundColor }] : [],
    });
    await wait(() => window.__queueSelectionRecorded === stage, 'native observation: ' + label);
  }
  function count(state, suffix) {
    return state.requests.filter(({ path }) => path.endsWith(suffix)).length;
  }
  async function checkCounts(recommends, privileges, message) {
    const state = await fixture('read');
    assert(count(state, '/rcmd/song') === recommends && count(state, '/enhance/privilege') === privileges,
      message + ': ' + JSON.stringify(state.requests));
  }
  async function waitCounts(recommends, privileges) {
    await wait(async () => {
      const state = await fixture('read');
      return count(state, '/rcmd/song') === recommends && count(state, '/enhance/privilege') === privileges;
    }, 'recommendation/privilege calls: ' + recommends + '/' + privileges);
  }
  try {
    await wait(() => button('列表循环') && !button('列表循环').disabled && one('.track-row'), 'Daily startup');
    const provides = one('#app').__vue_app__._context.provides;
    client = provides[Reflect.ownKeys(provides).find((key) => typeof key === 'symbol' && key.description === 'core-client')];
    epoch = (await client.accountSnapshot()).epoch;
    const nativeAction = client.playerAction.bind(client);
    client.playerAction = (owner, action) => {
      if (action.kind === 'queue_more_opened' || action.kind === 'queue_more_toggle') {
        assert(owner === epoch && /^[0-9]+$/.test(action.expected_revision), 'action is epoch/revision-bound');
        actionCalls.push({ kind: action.kind, expectedRevision: action.expected_revision });
      }
      return nativeAction(owner, action);
    };
    await resize(1120, 760);
    const allButton = button('播放全部');
    assert(allButton, 'official Daily playback button');
    click(allButton);
    await wait(() => bar().dataset.trackId === '1' && bar().dataset.state === 'playing' && Number(bar().dataset.position) > .18,
      'actual Daily seed audio');
    initialPlayId = bar().dataset.playId;
    assert(ids(await queue()).join(',') === '1,2', 'non-tail Daily entry with two seeds');
    await checkCounts(0, 0, 'no recommendation while drawer is unopened');
    if (window.__queueMoreControl) {
      await open();
      assert(!(await more())?.visible && !toggle() && !divider(), 'control assignment suppresses UI');
      await capture('control');
      await checkCounts(0, 0, 'control assignment suppresses network');
    } else {
      await wait(async () => (await more())?.prefetchOnOpen, 'eligible and awaiting the first drawer mount');
      await capture('unopened');
      await fixture('hold');
      await open();
      await wait(() => toggle()?.getAttribute('aria-checked') === 'true', 'eligible switch default');
      assert(document.getElementById(toggle().getAttribute('aria-labelledby'))?.textContent.trim() ===
        '在列表下方推荐你喜欢的相似歌曲', 'official switch label');
      await waitCounts(1, 0);
      await pause();
      await seek(3);
      await resize(760, 540);
      const scroll = one('.queue-scroll');
      scroll.scrollTop = scroll.scrollHeight;
      await queue();
      await queue();
      assert(bar().dataset.playId === initialPlayId && !divider(), 'held recommendation keeps seed and separator hidden');
      await capture('held');
      await checkCounts(1, 0, 'pause, seek, resize, scroll and reading do not repeat requests');
      await fixture('release');
      await wait(async () => ids(await queue()).join(',') === '1,2,51,52,53' && !!divider(), 'three rows and divider appended');
      await waitCounts(1, 1);
      assert(bar().dataset.playId === initialPlayId && bar().dataset.state === 'paused' &&
        Math.abs(Number(bar().dataset.position) - 3) < .2, 'append preserves native playback and clock');
      await capture('appended');

      await close();
      await open();
      await capture('reopened');
      await checkCounts(1, 1, 'reopening with a recommendation section does not fetch');

      const first = row(entryFor(await queue(), '51'));
      assert(first && first.querySelector('[data-queue-more-badge]'), 'first native recommended row mounted');
      first.querySelector('.queue-song').dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
      await wait(() => bar().dataset.trackId === '51' && bar().dataset.state === 'playing' &&
        Number(bar().dataset.position) > .12, 'explicit recommendation decodes actual PCM');
      assert(bar().dataset.playId !== initialPlayId, 'explicit selection starts only the chosen entry');
      await capture('selected');

      const recommendationPlayId = bar().dataset.playId;
      // The 30-second local PCM still reaches a real libmpv EOF; no fake engine event.
      const seekInput = one('[aria-label="播放进度"]');
      assert(Math.abs(Number(seekInput.max) - 30) < .3, 'known finite media duration');
      seekInput.value = '29.2';
      seekInput.dispatchEvent(new Event('change', { bubbles: true }));
      await wait(() => bar().dataset.trackId === '52' && bar().dataset.state === 'playing' &&
        Number(bar().dataset.position) > .12, 'native EOF advances to the next recommended entry');
      assert(bar().dataset.playId !== recommendationPlayId, 'natural EOF loads the next entry');
      await capture('eof');
      await checkCounts(1, 1, 'non-tail EOF does not cause another recommendation');

      await pause();
      await seek(3);
      const retained = bar().dataset.playId;
      await wait(toggleAvailable, 'more recommendation switch available');
      click(toggle());
      await wait(async () => ids(await queue()).join(',') === '1,2,52' &&
        toggle()?.getAttribute('aria-checked') === 'false' && !divider(), 'off retains only the current recommended row');
      assert(bar().dataset.playId === retained && bar().dataset.state === 'paused', 'off preserves decoder');
      await capture('off_retained');
      await checkCounts(1, 1, 'turning off makes no recommendation request');

      await fixture('hold');
      await wait(toggleAvailable, 'more recommendation switch available');
      click(toggle());
      await waitCounts(2, 1);
      await wait(() => toggle()?.getAttribute('aria-checked') === 'true' && toggleAvailable(),
        'turn-off remains available while held');
      await wait(toggleAvailable, 'more recommendation switch available');
      click(toggle());
      await wait(() => toggle()?.getAttribute('aria-checked') === 'false', 'prompt turn-off before service release');
      assert(bar().dataset.playId === retained && Math.abs(Number(bar().dataset.position) - 3) < .2,
        'toggle race keeps the paused entry');
      await capture('off_during_fetch');
      await fixture('release');
      await waitCounts(2, 2);
      await sleep(320);
      assert(ids(await queue()).join(',') === '1,2,52' && !divider() &&
        toggle()?.getAttribute('aria-checked') === 'false' && bar().dataset.playId === retained,
        'late privilege completion does not append or load after turn-off');
      await capture('late_discarded');
      await checkCounts(2, 2, 'late reply does not retry');
    }
    await emit('complete', {
      ok: true, stageCount, control: !!window.__queueMoreControl, realMusicRequests: 0,
      accountWrites: 0, initialPlayId, actionCalls,
    });
  } catch (error) {
    await emit('failed', { ok: false, stageCount, last, error: String(error),
      playId: bar()?.dataset.playId, trackId: bar()?.dataset.trackId, position: bar()?.dataset.position,
      panel: panel()?.textContent.slice(0, 1600), body: document.body.textContent.slice(-1800), actionCalls });
  }
})();
