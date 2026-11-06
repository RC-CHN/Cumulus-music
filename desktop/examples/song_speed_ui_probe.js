// Closed signed experiment changes and real libmpv clock; no service contact.
(async () => {
  const one = (query) => document.querySelector(query);
  const catalogRow = () => one('.track-row[data-track-id="2"]');
  const bar = () => one('.playerbar');
  const trigger = () => one('.song-speed-trigger');
  const popup = () => one('.song-speed-panel');
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const invoke = (...args) =>
    (window.__CUMULUS_PROOF_INVOKE__ ?? window.__TAURI_INTERNALS__.invoke)(...args);
  const emit = (stage, value = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...value } });
  const assert = (value, label) => { if (!value) throw Error(label); };
  const wait = async (test, label) => {
    for (let i = 0; i < 175; i++) {
      if (await test()) return;
      await sleep(60);
    }
    throw Error(label);
  };
  const click = (node) => {
    assert(node && !node.disabled, 'control unavailable');
    node.focus({ preventScroll: true });
    node.click();
  };
  const busy = () => one('[data-song-speed-save-state="pending"]');
  const playButton = () => bar()?.querySelector('[aria-label="播放"]');
  const pauseButton = () => bar()?.querySelector('[aria-label="暂停"]');
  const custom = () => popup()?.querySelector('.song-speed-row');
  const range = () => popup()?.querySelector('input');
  let count = 0, nonce = 0, epoch, native, current, ratios = [];
  async function cohort(mode) {
    const desired = ++nonce;
    await emit('pc-presentation-fixture', { mode, nonce: desired });
    await wait(() => window.__pcPresentationFixtureAck?.nonce === desired, 'signed cohort acknowledgement');
    const ack = window.__pcPresentationFixtureAck;
    assert(ack.result?.Ok && ack.afterFull === ack.beforeFull + 1, 'one signed full-batch read');
  }
  async function observe(label, rate, check = () => true) {
    await wait(async () => {
      current = await native();
      return current.speed === rate && check(current);
    }, label);
    return current;
  }
  async function capture(label) {
    const r = popup()?.getBoundingClientRect();
    const focus = document.activeElement;
    await emit(`ux_${String(++count).padStart(2, '0')}_${label}`, {
      player: await native(),
      panel: r ? { left: r.left, top: r.top, right: r.right, bottom: r.bottom, width: r.width } : null,
      documentContained: !r || (r.left >= 0 && r.top >= 0 && r.right <= innerWidth && r.bottom <= innerHeight),
      focus: focus === trigger() ? 'trigger' : focus === custom() ? 'custom' : focus === range() ? 'range' : 'other',
    });
  }
  async function open() {
    click(trigger());
    await wait(() => popup(), 'open song speed panel');
  }
  async function chooseSlider(value) {
    const input = range();
    assert(input?.min === '0.5' && input?.max === '2' && input?.step === '0.1', 'official slider limits');
    input.focus();
    input.value = String(value);
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await observe('custom rate committed', value, (view) => view.songSpeed?.customSelected && !busy());
    assert(document.activeElement === input && popup(), 'live custom slider stays open and focused');
  }
  async function clock(rate, playId) {
    const start = await observe('active native play', rate, (view) => view.playId === playId && view.state === 'playing');
    const t0 = performance.now();
    await sleep(1450);
    const finish = await native();
    const elapsed = (performance.now() - t0) / 1000;
    const delta = finish.position - start.position;
    assert(finish.playId === playId && finish.state === 'playing' && finish.speed === rate && !finish.error, 'same native play');
    assert(Math.abs(delta - elapsed * rate) < .45 && delta > .8, 'decoded clock matches native rate');
    ratios.push({ rate, elapsed, delta });
  }
  try {
    await wait(() => catalogRow()?.querySelector('.track-play:not(:disabled)'), 'native fixture ready');
    ({ epoch } = await invoke('account_snapshot'));
    native = () => invoke('player_snapshot', { epoch });
    await cohort('t1');
    click(catalogRow().querySelector('.track-play'));
    await wait(() => bar()?.dataset.state === 'playing' && pauseButton() && !pauseButton().disabled &&
      Number(bar().dataset.position) > .2 && catalogRow()?.classList.contains('current'), 'real catalog row playing state');
    const playId = bar().dataset.playId;
    click(pauseButton());
    await wait(() => bar().dataset.state === 'paused' && trigger() && !trigger().disabled, 'real paused state');
    await open();
    const rows = [...popup().querySelectorAll('button')].map((row) => row.textContent.trim());
    assert(rows.slice(1).join('/') === '2.0/1.5/1.0（正常）/0.7/0.5', 'official preset order');
    if (window.__songSpeedReopen) {
      const restored = await observe('persisted custom after process restart', 1.3,
        (view) => view.songSpeed?.customTenths === 13 && view.songSpeed?.customSelected);
      assert(custom().getAttribute('aria-pressed') === 'true' && range().value === '1.3', 'persisted panel state');
      await capture('persisted_custom');
      await emit('complete', { ok: true, stageCount: count, restored: restored.songSpeed });
      return;
    }
    await chooseSlider(1.5);
    assert(custom().getAttribute('aria-pressed') === 'true' &&
      popup().querySelectorAll('[aria-pressed="true"]').length === 1, 'equal-valued preset is not selected');
    await capture('custom_same_as_preset');
    click([...popup().querySelectorAll('button')].find((row) => row.textContent.trim() === '0.7'));
    await observe('preset changed on same paused play', .7, (view) =>
      view.playId === playId && view.state === 'paused' && !view.songSpeed?.customSelected && !busy());
    assert(!popup() && document.activeElement === trigger(), 'preset closes and returns focus');
    await open();
    assert(custom().textContent.trim() === '自定义 (1.5)', 'preset keeps the custom value');
    await capture('remembered_custom');
    click(custom());
    await observe('remembered custom applies', 1.5, (view) => view.songSpeed?.customSelected && !busy());
    await chooseSlider(1.3);
    const prior = await native();
    const stale = { kind: 'song_speed', expected_revision: prior.songSpeed.revision,
      expected_entry: prior.entryId, selection: { kind: 'custom', tenths: 18 } };
    popup().dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await wait(() => !popup() && document.activeElement === trigger(), 'Escape restores focus');
    click(playButton());
    await clock(1.3, playId);
    await cohort('c');
    await observe('cohort off keeps remembered choice on the same play', 1, (view) =>
      view.playId === playId && view.songSpeed?.enabled === false && view.songSpeed?.customTenths === 13);
    await wait(() => !trigger(), 'control cohort hides song panel');
    let rejected = false;
    try { await invoke('player_action', { epoch, action: stale }); } catch (error) {
      rejected = error?.code === 'stale';
    }
    assert(rejected, 'stale cohort action rejected by native player');
    await clock(1, playId);
    await capture('control_cohort');
    await cohort('t1');
    await clock(1.3, playId);
    await wait(() => trigger() && !trigger().disabled, 'restored rate control');
    await open();
    assert(custom().getAttribute('aria-pressed') === 'true' && range().value === '1.3', 'same custom selection restored');
    await capture('restored_cohort');
    const provides = one('#app').__vue_app__._context.provides;
    const core = provides[Reflect.ownKeys(provides).find((key) => typeof key === 'symbol' && key.description === 'core-client')];
    const voice = await core.playSelection(epoch, { kind: 'voice', id: '402' });
    assert(voice.resourceKind === 'voice' && voice.track?.voiceId === '402' && voice.track?.id === '2' &&
      voice.speedScope === 'voice' && voice.speed === 1 && !voice.songSpeed, 'voice uses the same backing song ID');
    await wait(() => catalogRow() && !catalogRow().classList.contains('current'),
      'playing a voice with the same backing ID clears the catalog row highlight');
    await core.playerAction(epoch, { kind: 'speed_for', scope: 'voice', value: 1.5 });
    await cohort('c');
    await observe('voice rate unaffected by c', 1.5, (view) => view.playId === voice.playId && !view.songSpeed);
    await cohort('t1');
    await observe('voice rate unaffected by t1', 1.5, (view) => view.playId === voice.playId && !view.songSpeed);
    await wait(() => !trigger() && one('.speed-control [aria-label="播放速度"]'), 'voice menu remains separate');
    await capture('voice_separate');
    await emit('complete', { ok: true, stageCount: count, ratios, staleRejected: true, voiceRate: 1.5,
      catalogIdentity: { matchedSong: '2', playedVoice: voice.track.voiceId, backingSong: voice.track.id,
        rowStillVisible: !!catalogRow(), rowHighlighted: catalogRow()?.classList.contains('current') } });
  } catch (error) {
    await emit('failed', { ok: false, error: String(error), stageCount: count, observed: current, ratios });
  }
})();
