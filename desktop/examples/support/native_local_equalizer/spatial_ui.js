// The sealed host prepares only fixed synthetic audio; all user controls and
// observations below use the rendered main UI and production Tauri commands.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (payload) => invoke('plugin:event|emit', { event: 'local-spatial-proof', payload });
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (value, message) => { if (!value) throw new Error(message); };
  const bar = () => one('.playerbar');
  const dialog = () => one('.equalizer-dialog');
  const control = (label) => one(`[aria-label="${label}"]`);
  const spatial = () => one('.equalizer-dialog [data-spatial-status]');
  const launcher = () => one('.playerbar .equalizer-toggle');
  const master = () => control('启用均衡器和增强音效');
  const common = () => one('dialog.audio-effects-panel[open]');
  const commonTab = (key) => common()?.querySelector(`[data-audio-effect-tab="${key}"]`);
  const commonMaster = () => common()?.querySelector('.audio-effects-master[data-spatial-master-key]');
  const commonProof = {};
  const transport = () => one('.playerbar .play-toggle');
  const tone = ['低音增益', '高音增益'];
  const sliders = ['环绕深度', '环绕强度'];
  let last = 'account';
  let epoch;
  let localId;
  let indexedNavigation;
  const indexedWave = window.__spatialIndexedWave === true;
  async function wait(read, label) {
    last = label;
    const deadline = performance.now() + 11000;
    while (performance.now() < deadline) {
      const value = await read();
      if (value) return value;
      await sleep(35);
    }
    throw new Error(label);
  }
  function visible(node) {
    if (!node?.isConnected || node.disabled || node.closest('[inert]')) return false;
    const r = node.getBoundingClientRect();
    return r.width > 0 && r.height > 0 &&
      node.contains(document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2));
  }
  function reveal(node) {
    if (!visible(node)) node?.scrollIntoView({ block: 'nearest', behavior: 'instant' });
  }
  function click(node) {
    reveal(node);
    assert(visible(node), 'expected visible control: ' + node?.outerHTML.slice(0, 120));
    node.focus({ preventScroll: true });
    node.click();
  }
  function set(label, value, event = 'change') {
    return setRange(control(label), label, value, event);
  }
  function setRange(node, label, value, event = 'change') {
    reveal(node);
    assert(visible(node), 'visible slider ' + label);
    node.value = String(value);
    const accepted = Number(node.value);
    node.dispatchEvent(new Event(event, { bubbles: true }));
    return accepted;
  }
  function sliderValues(labels) {
    return labels.map((label) => Number(control(label)?.value));
  }
  function matchingControl(node, keyField, view, transportState) {
    if (!visible(node)) return false;
    const key = JSON.parse(node.dataset[keyField] ?? 'null');
    return Array.isArray(key) && key[0] === epoch && key[1] === view.playId &&
      key[2] === view.queueRevision && (transportState === undefined || key[4] === transportState);
  }
  function samePlay(a, b) {
    return a.playId === b.playId && a.entryId === b.entryId && a.queueRevision === b.queueRevision;
  }
  function snapshot() { return invoke('player_snapshot', { epoch }); }
  function localNav() {
    const buttons = [...document.querySelectorAll(
      '.sidebar-scroll .sidebar-navigation[aria-label="我的"] button.nav-item'
    )].filter((node) => node.querySelector('span')?.textContent.trim() === '本地音乐');
    assert(buttons.length <= 1, 'exactly one Local Music sidebar route');
    return buttons[0];
  }
  function navGeometry(node) {
    const scroller = node.closest('.sidebar-scroll');
    const bounds = node.getBoundingClientRect();
    const viewport = scroller.getBoundingClientRect();
    const x = bounds.left + bounds.width / 2;
    const y = bounds.top + bounds.height / 2;
    const hit = document.elementFromPoint(x, y);
    return {
      scrollTop: scroller.scrollTop,
      scrollMaximum: scroller.scrollHeight - scroller.clientHeight,
      documentScroll: [scrollX, scrollY],
      nodeTop: bounds.top, nodeBottom: bounds.bottom,
      viewportTop: viewport.top, viewportBottom: viewport.bottom,
      centerHit: visible(node) && x >= viewport.left && x < viewport.right &&
        y >= viewport.top && y < viewport.bottom,
      hit: hit?.tagName ?? null,
    };
  }
  async function enterIndexedLibrary() {
    const route = await wait(localNav, 'mounted Local Music sidebar route');
    indexedNavigation = { label: '本地音乐', before: navGeometry(route) };
    if (!indexedNavigation.before.centerHit) {
      const scroller = route.closest('.sidebar-scroll');
      const node = route.getBoundingClientRect(), viewport = scroller.getBoundingClientRect();
      scroller.scrollTo({
        top: scroller.scrollTop + (node.top + node.bottom - viewport.top - viewport.bottom) / 2,
        behavior: 'instant',
      });
    }
    await wait(() => {
      if (localNav() !== route) return;
      indexedNavigation.after = navGeometry(route);
      return indexedNavigation.after.centerHit;
    }, 'visible Local Music navigation after sidebar reveal');
    const { before, after } = indexedNavigation;
    assert(before.documentScroll.every((position, i) => position === after.documentScroll[i]),
      'sidebar reveal did not scroll the document');
    assert(before.centerHit || before.scrollTop !== after.scrollTop, 'sidebar route really revealed');
    click(route);
    await wait(() => route.getAttribute('aria-current') === 'page' &&
      one('.local-page h1')?.textContent.trim() === '本地音乐', 'actual Local Music route selected');
    indexedNavigation.current = route.getAttribute('aria-current');
  }
  async function openFile(ordinal) {
    await emit({ open: ordinal });
    const ack = await wait(() => window.__spatialOpenAck?.ordinal === ordinal &&
      window.__spatialOpenAck, 'local audio fixture ready ' + ordinal);
    assert(ack.ok, ack.error);
    if (indexedWave) {
      if (ordinal === 1) await enterIndexedLibrary();
      assert(localNav()?.getAttribute('aria-current') === 'page', 'current Local Music route');
      const row = await wait(() => {
        const rows = [...document.querySelectorAll('.local-page .local-tracks tbody tr[data-local-id]')];
        return rows.length === 1 && rows[0];
      }, 'mounted actual indexed WAV row');
      const play = row.querySelector('.local-title');
      reveal(play);
      await wait(() => visible(play), 'visible indexed WAV play button');
      assert(/^[0-9a-f]{64}$/.test(row.dataset.localId), 'native local file identity');
      assert(!localId || localId === row.dataset.localId, 'same indexed row selected again');
      localId = row.dataset.localId;
      click(play);
    }
  }
  async function openEq() {
    await wait(() => visible(launcher()), 'enabled EQ launcher');
    click(launcher());
    await wait(() => dialog()?.open && control(sliders[1]), 'opened EQ dialog');
    reveal(control(sliders[1]));
    await wait(() => visible(control(sliders[0])) && visible(control(sliders[1])),
      'visible spatial controls');
    assert(sliders.every((label, i) => {
      const input = control(label);
      return input.min === String(i === 0 ? -10 : 0) && input.max === '10' && input.step === '1';
    }), 'official integer slider geometry');
  }
  async function closeEq() {
    click(control('关闭均衡器'));
    await wait(() => !dialog(), 'closed EQ dialog');
  }
  async function openCommon() {
    const trigger = await wait(() => {
      const node = one('.playerbar .audio-effects-trigger');
      return visible(node) && node;
    }, 'visible ordinary audio effect entry');
    click(trigger);
    await wait(() => commonTab('catalogue')?.getAttribute('aria-selected') === 'true',
      'common effect modal opens on catalogue');
  }
  async function closeCommon() {
    click(common()?.querySelector('[aria-label="关闭音效"]'));
    await wait(() => !one('dialog.audio-effects-panel'), 'closed common effect modal');
  }
  async function requireSpatialRejection(view, overrides = {}) {
    const rejected = await invoke('player_action', { epoch, action: {
      kind: 'spatial_equalizer_master', play_id: view.playId,
      expected_queue_revision: view.queueRevision, expected_revision: view.equalizer.revision,
      bypassed: !view.equalizer.settings.master_bypassed, ...overrides,
    }}).then(() => false, () => true);
    assert(rejected, 'unsupported or stale common switch is rejected natively');
    const next = await snapshot();
    assert(next.equalizer.revision === view.equalizer.revision &&
      !!next.equalizer.settings.master_bypassed === !!view.equalizer.settings.master_bypassed &&
      samePlay(next, view), 'rejected common switch did not save or alter playback');
  }
  async function toggleCommon(before, bypassed) {
    await closeEq();
    await openCommon();
    const button = await wait(() => {
      const node = commonMaster();
      return visible(node) && node.getAttribute('aria-checked') === String(bypassed) && node;
    }, 'enabled shared spatial switch reflects the confirmed old state');
    const other = bypassed ? 'devices' : 'equalizer';
    click(commonTab(other));
    await wait(() => commonTab(other)?.getAttribute('aria-selected') === 'true' &&
      commonMaster() === button, 'the same spatial switch follows the selected tab');
    if (other === 'equalizer') assert(!master(), 'embedded EQ does not duplicate the shared switch');
    click(button);
    assert(button.getAttribute('aria-checked') === String(bypassed),
      'shared spatial state waits for native acknowledgement');
    const applied = await wait(async () => {
      const view = await snapshot();
      return samePlay(view, before) && view.equalizer.revision !== before.equalizer.revision &&
        view.equalizer.spatial_master_available &&
        view.equalizer.spatial_status === (bypassed ? 'bypassed' : 'configured') &&
        !!view.equalizer.settings.master_bypassed === bypassed &&
        commonMaster() === button && button.getAttribute('aria-checked') === String(!bypassed) &&
        button.getAttribute('aria-disabled') !== 'true' && view;
    }, 'one acknowledged shared spatial change preserves playback identity');
    commonProof[bypassed ? 'off' : 'on'] = { from: 'catalogue', to: other,
      priorRevision: before.equalizer.revision, revision: applied.equalizer.revision };
    await closeCommon();
    await openEq();
    return equalizer(before.equalizer.revision, [6, 8], bypassed ? 'bypassed' : 'configured');
  }
  async function playback(read, label) {
    return wait(async () => {
      const view = await snapshot();
      assert(!view.error, 'native player error: ' + view.error);
      return view.state === bar()?.dataset.state && read(view) && view;
    }, label);
  }
  async function pause(view) {
    assert(transport()?.getAttribute('aria-label') === '暂停', 'visible Pause');
    click(transport());
    return playback((now) => samePlay(now, view) && now.state === 'paused' && !now.nativeSeeking,
      'accepted real Pause');
  }
  async function equalizer(revision, values, status) {
    return wait(async () => {
      const view = await snapshot();
      return view.equalizer.revision !== revision &&
        view.equalizer.spatial_status === status &&
        dialog()?.dataset.draftState === 'settled' &&
        JSON.stringify(sliderValues(sliders)) === JSON.stringify(values) &&
        (status === 'off' || spatial()?.dataset.spatialStatus === status) && view;
    }, 'committed spatial controls: ' + status);
  }
  function paintSample(selector) {
    const node = one(selector);
    assert(node?.isConnected && !node.closest('[inert]'), 'connected capture padding');
    const bounds = node.getBoundingClientRect();
    const pad = Number.parseFloat(getComputedStyle(node).paddingLeft);
    const x = bounds.left + Math.min(5, pad / 2), y = bounds.top + bounds.height / 2;
    assert(pad >= 4 && document.elementFromPoint(x, y) === node, 'uncovered capture padding');
    let color;
    for (let current = node; current; current = current.parentElement) {
      const style = getComputedStyle(current);
      assert(style.opacity === '1' && style.visibility === 'visible', 'visible paint ancestry');
      if (color) continue;
      assert(style.backgroundImage === 'none', 'solid capture padding');
      const rgb = /^rgb\((\d+),\s*(\d+),\s*(\d+)\)$/.exec(style.backgroundColor);
      if (rgb) color = 'rgb(' + rgb.slice(1).join(', ') + ')';
      else assert(style.backgroundColor === 'transparent' ||
        /^rgba\(\d+,\s*\d+,\s*\d+,\s*0\)$/.test(style.backgroundColor), 'nonopaque padding');
    }
    assert(color && color !== 'rgb(0, 0, 0)', 'nonblack opaque background');
    return { x, y, color, source: selector };
  }
  async function stage(name, view, extra = {}) {
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    if (dialog()) {
      const r = dialog().getBoundingClientRect();
      assert(dialog().scrollWidth <= dialog().clientWidth + 1 &&
        r.top >= 0 && r.bottom <= innerHeight + 1, 'EQ dialog stays in viewport');
    }
    const capture = window.__spatialCapture ? {
      screenX, screenY, ratio: devicePixelRatio,
      paintSamples: (dialog() ? ['.equalizer-dialog'] : ['.sidebar-caption', '.playerbar'])
        .map(paintSample),
    } : {};
    await emit({ stage: name, epoch, player: view, ...capture, ...extra,
      ...(indexedWave ? { indexedLocalId: localId, indexedNavigation } : {}), dom: {
      state: bar()?.dataset.state, status: spatial()?.dataset.spatialStatus ?? null,
      sliders: dialog() ? sliderValues(sliders) : null,
      tone: dialog() ? sliderValues(tone) : null,
      master: master()?.getAttribute('aria-checked') ?? null,
    }});
    await wait(() => window.__spatialStageAck === name, 'durable stage ' + name);
  }
  try {
    const account = await wait(async () => {
      const value = await invoke('account_snapshot');
      return value.epoch != null && value.status !== 'authenticated' && value;
    }, 'isolated guest epoch');
    epoch = account.epoch;
    await wait(() => visible(launcher()), 'ready local player');

    await openFile(1);
    const stock = await playback((view) => view.state === 'playing' && view.position > .08,
      'first stock source advances');
    const stockPaused = await pause(stock);
    await openEq();
    const before = await snapshot();
    set(sliders[0], 3, 'input');
    await sleep(180);
    const preview = await snapshot();
    assert(preview.equalizer.revision === before.equalizer.revision &&
      !preview.equalizer.settings.presence, 'uncommitted drag does not write settings');
    set(sliders[0], 3);
    set(sliders[1], 5);
    const saved = await equalizer(before.equalizer.revision, [3, 5], 'unavailable');
    assert(samePlay(saved, stockPaused) && saved.state === 'paused' &&
      !saved.equalizer.settings.enabled && saved.equalizer.settings.gains.every((gain) => gain === 0),
    'spatial-only stock save does not restart or enable GEQ');
    await stage('ux_01_stock_saved', saved);
    await closeEq();
    assert(!saved.equalizer.spatial_master_available, 'stock PCM does not grant spatial master control');
    await openCommon();
    assert(!commonMaster(), 'no spatial master is rendered for the stock decoder');
    await requireSpatialRejection(saved);
    commonProof.stockRejected = true;
    await closeCommon();

    await openFile(2);
    const playing = await playback((view) => view.state === 'playing' &&
      view.position > .1 && view.playId !== stock.playId &&
      view.equalizer.spatial_status === 'processing' && launcher()?.dataset.equalizerState === 'processing',
    'native clock shows real spatial-only PCM processing for the selected file');
    const paused = await pause(playing);
    await openEq();
    set(sliders[0], 6);
    set(sliders[1], 8);
    const hot = await equalizer(paused.equalizer.revision, [6, 8], 'configured');
    assert(samePlay(hot, paused) && hot.state === 'paused', 'hot apply preserves paused native play');
    await stage('ux_02_hot_paused', hot);

    assert(hot.equalizer.spatial_master_available, 'native owned worker admits spatial-only master');
    await requireSpatialRejection(hot, { play_id: stock.playId });
    await requireSpatialRejection(hot, { expected_queue_revision: (BigInt(hot.queueRevision) + 1n).toString() });
    await requireSpatialRejection(hot, { expected_revision: paused.equalizer.revision });
    commonProof.staleRejected = true;
    const off = await toggleCommon(hot, true);
    assert(samePlay(off, hot) && off.equalizer.settings.master_bypassed, 'master retains saved sliders');
    await stage('ux_03_master_off', off);
    const on = await toggleCommon(off, false);
    assert(samePlay(on, off) && !on.equalizer.settings.master_bypassed, 'master re-applies same graph');
    await stage('ux_04_master_on', on);
    await closeEq();
    if (indexedWave) {
      await emit({ replaceIndexedPath: true });
      await wait(() => window.__spatialIndexedPathReplaced, 'held index fixture pathname replaced');
    }

    const requested = .5;
    const { step, accepted } = await wait(() => {
      const rail = control('播放进度');
      if (!matchingControl(rail, 'scrubKey', on) || bar()?.dataset.state !== 'paused') return;
      const step = Number(rail.step);
      assert(step > 0 && Number.isFinite(step), 'valid live seek geometry');
      return { step, accepted: setRange(rail, '播放进度', requested) };
    }, 'enabled seek rail bound to the paused source');
    assert(Math.abs(accepted - requested) <= step / 2 + 1e-9,
      'browser range rounds the requested seek within half its actual step');
    // A pre-seek paused clock can already fall within the target tolerance.
    // Require a receipt for the actual replacement and a fresh native poll.
    await emit({ observePausedSeek: true });
    const seek = await wait(() => window.__spatialNativeSeekAck, 'observed native paused seek');
    assert(seek.ok && seek.seconds === accepted, 'exact native paused seek and replacement');
    const sought = await playback((view) => samePlay(view, on) && view.state === 'paused' &&
      !view.nativeSeeking && Math.abs(view.position - accepted) < .06, 'paused native replacement clock');
    await stage('ux_05_paused_seek', sought, { seekTarget: { requested, accepted, step } });
    await wait(() => {
      const resume = transport();
      if (resume?.getAttribute('aria-label') !== '播放' ||
          !matchingControl(resume, 'primaryTransportKey', sought, 'paused')) return;
      click(resume);
      return true;
    }, 'enabled Resume bound to the replaced paused source');
    await playback((view) => samePlay(view, sought) && view.state === 'playing' &&
      view.position > .65 && view.equalizer.spatial_status === 'processing', 'resumed native spatial PCM');
    const ended = await playback((view) => samePlay(view, sought) && view.state === 'ended' &&
      Math.abs(view.position - 88207 / 44100) < .02, 'real native EOF at original displayed source duration');
    await stage('ux_06_ended', ended);

    await openEq();
    set(tone[0], 4);
    set(tone[1], -2);
    const all = await wait(async () => {
      const view = await snapshot();
      return view.equalizer.settings.bass_db === 4 && view.equalizer.settings.treble_db === -2 &&
        dialog()?.dataset.draftState === 'settled' && view;
    }, 'native preference has all four enhancements before reset');
    click([...dialog().querySelectorAll('button')].find((node) => node.textContent.trim() === '重置增强音效'));
    const reset = await equalizer(all.equalizer.revision, [0, 0], 'off');
    assert(samePlay(reset, ended) && tone.every((_, i) => sliderValues(tone)[i] === 0) &&
      !reset.equalizer.settings.bass_db && !reset.equalizer.settings.treble_db,
    'one enhanced reset clears tone and spatial, keeps completed source');
    await stage('ux_07_reset', reset);
    await emit({ stage: 'complete', ok: true, stageCount: 7, epoch,
      startedSpatial: playing.equalizer.spatial_status === 'processing', commonProof,
      ...(indexedWave ? { indexedLocalId: localId, indexedNavigation } : {}) });
  } catch (error) {
    await emit({ stage: 'failed', ok: false, last, error: String(error), epoch,
      ...(indexedWave ? { indexedNavigation, indexedLocalId: localId } : {}) });
  }
})();
