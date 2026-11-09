// Isolated real WebView: per-book progress, native chapter playback, and a closed PC cohort.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, payload = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...payload },
  });
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (condition, label) => { if (!condition) throw Error(label); };
  const click = (node) => {
    assert(node && !node.disabled && !node.closest('[inert]'), 'action is enabled');
    node.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    node.focus({ preventScroll: true });
    node.click();
  };
  async function wait(test, label) {
    for (let n = 0; n < 180; n++) {
      const current = await test();
      if (current) return current;
      await sleep(60);
    }
    throw Error(label);
  }
  const settled = () => !one('.page-enter-active,.page-leave-active');
  const book = (index) => one(`[data-listened-book="${index}"]`);
  const chapter = () => book(0)?.querySelector('[data-listened-chapter]');
  const pagination = (label) => all('.listened-books-pagination button').find((node) => node.textContent.trim() === label);
  const setting = () => all('.podcast-page label').find((node) => node.textContent.trim() === '续播')?.querySelector('input');
  const back = () => one('[aria-label="返回上一页"]');
  let epoch, sequence = 0, last = 'startup';
  const states = [];
  async function configure(enabled, group) {
    const id = ++sequence;
    await emit('books-resume-fixture', { id, enabled, group });
    await wait(() => window.__booksResumeAck?.[0] === id, 'native fixture acknowledges group/setting');
    assert(window.__booksResumeAck[1], 'real native PC experiment refresh succeeded');
  }
  async function capture(name, extra = {}) {
    last = name;
    await wait(settled, 'page visually settled');
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const stage = `ux_${String(states.length + 1).padStart(2, '0')}_${name}`;
    await emit(stage, {
      first: book(0)?.textContent.trim(), second: book(1)?.textContent.trim(),
      chapter: chapter()?.textContent.trim(), ...extra,
    });
    await wait(() => window.__CUMULUS_ALBUM_CAPTURE_ACK__ === stage, 'native receipt written');
    states.push(name);
  }
  async function firstPage(visible) {
    await wait(() => settled() && chapter() && !chapter().disabled && book(1), 'first Books page');
    assert(chapter().textContent.trim() === '目录章节 792', 'chapter title and link remain');
    assert(book(0).textContent.includes('已播 10%') === visible, 'exact first book progress visibility');
    assert(book(1).textContent.includes('已播完'), 'unknown other book remains visible');
  }
  async function seeBookSetting(enabled) {
    click(book(0).querySelector('.listened-book-title'));
    await wait(() => settled() && setting()?.checked === enabled, 'existing detail reads authoritative setting');
    click(back());
  }
  async function roundTrip() {
    click(pagination('下一页'));
    await wait(() => book(20) && !book(0), 'local page two');
    click(pagination('上一页'));
  }
  try {
    const choice = await wait(() => all('.sidebar button').find((node) => node.textContent.trim() === '精选' && !node.disabled), 'authenticated sidebar ready');
    epoch = (await invoke('account_snapshot')).epoch;
    click(choice);
    click(await wait(() => settled() && one('[data-essential-focus="books:history:open"]'), 'official Books entry'));
    await firstPage(true);
    await capture('unknown_setting_visible');

    await configure(false, true);
    await seeBookSetting(false);
    await firstPage(false);
    await capture('t1_off_hidden');
    await roundTrip();
    await firstPage(false);
    await capture('same_snapshot_hidden');

    const player = () => invoke('player_snapshot', { epoch });
    click(chapter());
    const playing = await wait(async () => {
      const value = await player();
      return value.state === 'playing' && !value.nativeSeeking && value.position > .2 && value;
    }, 'actual chapter decoder advances');
    assert(playing.track?.voiceId === '792' && playing.track.id === '1792' && playing.queueLen === 2
      && playing.position < 2.5 && !playing.error, `exact episode starts from zero with Off: ${playing.position}`);
    await capture('chapter_plays', { playing: true, firstNativePosition: playing.position });
    await invoke('player_action', { epoch, action: { kind: 'pause_for', play_id: playing.playId,
      expected_queue_revision: playing.queueRevision, paused: true } });
    await wait(async () => (await player()).state === 'paused', 'same native decoder pauses');

    await configure(false, false);
    await roundTrip();
    await firstPage(true);
    const current = await player();
    assert(current.state === 'paused' && current.playId === playing.playId
      && current.entryId === playing.entryId && current.queueRevision === playing.queueRevision,
    'control presentation does not replace native playback');
    await capture('control_visible');

    await configure(true, true);
    await seeBookSetting(true);
    await firstPage(true);
    await capture('t1_on_visible');
    await emit('complete', { ok: true, states, realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', { ok: false, last, error: String(error), text: one('.main-content')?.textContent.slice(0, 500) });
  }
})();
