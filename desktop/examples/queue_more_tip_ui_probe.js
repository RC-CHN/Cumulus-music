// Production recommendation cue and IPC; synthetic account/media only.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const invoke = (...args) =>
    (window.__CUMULUS_PROOF_INVOKE__ ?? window.__TAURI_INTERNALS__.invoke)(...args);
  const emit = (stage, value = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...value } });
  const assert = (condition, message) => { if (!condition) throw Error(message); };
  const wait = async (condition, message) => {
    for (let i = 0; i < 250; i++) {
      if (await condition()) return;
      await sleep(55);
    }
    throw Error(message);
  };
  const button = (label, root = document) => [...root.querySelectorAll('button')].find(
    (node) => node.getAttribute('aria-label') === label || node.textContent.trim() === label,
  );
  const click = (node) => { assert(node && !node.disabled, 'enabled control'); node.click(); };
  const bar = () => one('.playerbar');
  const panel = () => one('.queue-panel');
  const tip = () => one('[data-queue-more-tip]');
  const position = () => Number(bar()?.dataset.position);
  const playId = () => bar()?.dataset.playId;
  const state = () => bar()?.dataset.state;
  const track = () => bar()?.dataset.trackId;
  let client, epoch, sequence = 0, count = 0, last = 'startup', firstShown;
  const calls = [];
  async function setFocus(focus) {
    const request = ++sequence;
    await emit('queue-more-tip-focus', { focus, sequence: request });
    await wait(() => window.__queueTipFocusAck === request && document.hasFocus() === focus,
      'native focus switched to ' + focus);
    assert(document.visibilityState === 'visible', 'main window remains visible during focus test');
  }
  async function setPaused(paused) {
    await client.playerAction(epoch, { kind: 'pause', paused });
    await wait(() => state() === (paused ? 'paused' : 'playing'), 'native pause/resume');
  }
  async function seek(seconds) {
    await client.playerAction(epoch, { kind: 'seek', seconds });
    await wait(() => Math.abs(position() - seconds) < .08, 'native seek to ' + seconds);
  }
  async function open() {
    click(button('播放队列', bar()));
    await wait(() => panel() && one('.queue-scroll') && !one('.queue-panel-enter-active'), 'drawer open');
  }
  async function close() {
    click(button('关闭播放队列', panel()));
    await wait(() => !panel(), 'drawer closed');
  }
  async function selectRecommendation(id) {
    const q = await client.playerQueue(epoch);
    const item = q.tracks.find((row) => row.track.id === id && row.moreRecommendation);
    assert(item, 'native active recommended row');
    const previous = playId();
    await client.playerAction(epoch, { kind: 'select', entry_id: item.entryId });
    await wait(() => track() === id && playId() !== previous && state() === 'playing' && position() > .15,
      'actual decoded recommendation ' + id);
  }
  async function capture(label, facts = {}) {
    last = label;
    const view = await client.playerSnapshot(epoch);
    const stage = `ux_${String(++count).padStart(2, '0')}_${label}`;
    window.__queueSelectionRecorded = null;
    await emit(stage, {
      width: innerWidth, height: innerHeight, ratio: devicePixelRatio, screenX, screenY,
      focused: document.hasFocus(), visible: document.visibilityState === 'visible',
      cue: tip()?.textContent.trim() ?? null, cueCount: document.querySelectorAll('[data-queue-more-tip]').length,
      drawerOpen: !!panel(), position: position(), playId: playId(), trackId: track(), state: state(),
      eligible: view.queueMoreTipEligible === true, calls: calls.slice(), ...facts, captureHandshake: true,
    });
    await wait(() => window.__queueSelectionRecorded === stage, 'native observation: ' + label);
  }
  try {
    await wait(() => button('列表循环') && !button('列表循环').disabled && one('.track-row'), 'Daily startup');
    const provides = one('#app').__vue_app__._context.provides;
    client = provides[Reflect.ownKeys(provides).find((key) => typeof key === 'symbol' && key.description === 'core-client')];
    epoch = (await client.accountSnapshot()).epoch;
    const direct = client.queueMoreTip.bind(client);
    client.queueMoreTip = async (owner, action) => {
      assert(owner === epoch, 'cue is account bound');
      const started = performance.now();
      const result = await direct(owner, action);
      if (action.kind === 'begin') {
        assert(action.play_id === playId(), 'renderer cue claims the current native play ID');
        assert(result === null || /^[0-9a-f]{32}$/.test(result), 'bounded native cue receipt');
      } else {
        assert(action.kind === 'seen' && /^[0-9a-f]{32}$/.test(action.ticket) && result === null,
          'native cue completion contract');
      }
      calls.push({ kind: action.kind, granted: result !== null, at: started });
      return result;
    };
    one('.track-row').dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    await wait(() => track() === '1' && state() === 'playing' && position() > .15, 'actual Daily seed playback');
    await setPaused(true);
    await open();
    await wait(async () => (await client.playerQueue(epoch)).tracks.some((row) => row.moreRecommendation),
      'two-read recommendation fixture');
    await close();

    if (window.__queueTipReopen) {
      await setFocus(true);
      await selectRecommendation('51');
      await wait(() => calls.some(({ kind, granted }) => kind === 'begin' && !granted), 'native SQLite rejects restored UID offer');
      await sleep(1050);
      assert(!tip() && calls.length === 1 && !panel(), 'same UID restart suppresses eligible early-window cue');
      assert(await direct(epoch, { kind: 'begin', play_id: playId() }) === null, 'native restart guard denies cue');
      await capture('restart_suppressed');
    } else {
      await setFocus(false);
      await selectRecommendation('51');
      await sleep(1250);
      assert(!tip() && calls.length === 0, 'unfocused renderer never requests the cue');
      assert(await direct(epoch, { kind: 'begin', play_id: playId() }) === null,
        'unfocused native main window rejects direct renderer claim');
      await capture('unfocused');

      await setPaused(true);
      await seek(0);
      await setFocus(true);
      await sleep(1150);
      assert(!tip() && calls.length === 0 && await direct(epoch, { kind: 'begin', play_id: playId() }) === null,
        'zero clock does not show or consume');
      await capture('at_zero');

      await setFocus(false);
      await seek(32);
      await setPaused(false);
      await setFocus(true);
      await sleep(1150);
      assert(!tip() && calls.length === 0 && await direct(epoch, { kind: 'begin', play_id: playId() }) === null,
        'clock past 30 does not show or consume');
      await capture('past_window');

      await setFocus(false);
      await setPaused(true);
      await seek(3);
      await setPaused(false);
      await setFocus(true);
      await wait(() => tip() && calls.some(({ kind, granted }) => kind === 'begin' && granted), 'foreground cue displayed');
      firstShown = performance.now();
      assert(tip().textContent.trim() === '列表播完已自动推荐，↓点击去设置' && !panel() &&
        tip().getAttribute('role') === 'status' && getComputedStyle(tip()).pointerEvents === 'none',
        'exact passive cue copy and semantics');
      const icon = button('播放队列', bar()).getBoundingClientRect();
      const bubble = tip().getBoundingClientRect();
      assert(bubble.bottom <= icon.bottom + 1 && bubble.right >= icon.left && bubble.left <= icon.right,
        'cue anchored near queue icon');
      await capture('shown');
      await sleep(2000);
      assert(!!tip() && !calls.some(({ kind }) => kind === 'seen'), 'cue remains and no early completion');
      await wait(() => !tip() && calls.some(({ kind }) => kind === 'seen'), 'cue times out and completes');
      const durationMs = performance.now() - firstShown;
      assert(durationMs >= 4600 && durationMs < 8500, 'approximately five-second cue lifetime: ' + durationMs);
      await capture('seen', { durationMs });

      await open();
      assert(!tip(), 'queue control still opens normal drawer');
      await close();
      await selectRecommendation('52');
      await sleep(1300);
      assert(!tip() && calls.filter(({ kind }) => kind === 'begin').length === 1,
        'another recommendation in the same UID does not request or show cue');
      await capture('another_rec');
    }
    await emit('complete', { ok: true, count, reopened: !!window.__queueTipReopen, calls,
      realMusicRequests: 0, accountWrites: 0 });
  } catch (error) {
    await emit('failed', { ok: false, count, last, error: String(error), position: position(), state: state(),
      trackId: track(), focused: document.hasFocus(), cue: tip()?.outerHTML.slice(0, 500), calls });
  }
})();
