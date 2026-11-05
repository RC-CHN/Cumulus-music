// Sealed lyric credits through the real Tauri WebView, SQLite and null libmpv.
(async () => {
  if (window.__lyricCreditProofStarted) return;
  window.__lyricCreditProofStarted = true;
  window.__lyricAck = {};
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = payload => invoke('plugin:event|emit', { event: 'desktop-lyric-proof', payload });
  const one = selector => document.querySelector(selector);
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const check = (ok, label) => { if (!ok) throw Error(label); };
  async function wait(test, label) {
    for (let i = 0; i < 300; i++) {
      const result = await test();
      if (result) return result;
      await sleep(50);
    }
    throw Error(label);
  }
  let serial = 0, epoch, phase = 0, last = 'startup';
  const rows = () => [...document.querySelectorAll('.lyrics-panel [data-lyric-credit]')];
  const player = () => invoke('player_snapshot', { epoch });
  const action = action => invoke('player_action', { epoch, action });
  const lyric = playId => invoke('player_lyrics', { epoch, playId, cachedOnly: true });
  async function fixture(kind, payload = {}) {
    const id = ++serial;
    await emit({ kind, id, ...payload });
    const ack = await wait(() => window.__lyricAck[id], 'fixture acknowledgement: ' + kind);
    delete window.__lyricAck[id];
    if (ack.error) throw Error(ack.error);
    return ack.value ?? ack;
  }
  async function click(selector) {
    const node = await wait(() => {
      const button = one(selector);
      return button && !button.disabled && !button.closest('[inert]') &&
        button.getAttribute('aria-disabled') !== 'true' && button.getClientRects().length && button;
    }, 'available UI control: ' + selector);
    node.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    node.focus({ preventScroll: true });
    node.click();
  }
  async function capture(name, facts = {}) {
    last = name;
    const root = one('.lyrics-panel') ? '.lyrics-panel' : '.page-view:not([inert])';
    await wait(() => {
      const node = one(root);
      return node && getComputedStyle(node).opacity === '1' &&
        !node.getAnimations().some(animation => animation.pending || animation.playState === 'running');
    }, 'render settles: ' + name);
    const credits = rows().map(row => ({
      kind: row.dataset.lyricCredit,
      text: row.textContent.trim(),
      button: !!row.querySelector('button'),
      timed: row.hasAttribute('data-line'),
    }));
    if (credits.length) {
      const shell = one('.lyric-scroll');
      shell.scrollTop += rows()[0].getBoundingClientRect().top - shell.getBoundingClientRect().top -
        Math.max(20, shell.clientHeight * .18);
      if (rows().some(row => row.getBoundingClientRect().bottom > shell.getBoundingClientRect().bottom))
        rows().at(-1).scrollIntoView({ block: 'center', behavior: 'instant' });
      const frame = shell.getBoundingClientRect();
      check(rows().every(row => {
        const bounds = row.getBoundingClientRect();
        return bounds.top >= frame.top && bounds.bottom <= frame.bottom && bounds.height > 0;
      }), 'credit rows within actual scroll viewport');
    }
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    check(document.visibilityState === 'visible', 'visible real WebView');
    await emit({
      kind: 'snapshot', stage: `ux_${String(++phase).padStart(2, '0')}_${name}`,
      surface: 'main', credits, overflow: document.documentElement.scrollWidth > innerWidth,
      ...facts,
    });
    await sleep(1100);
  }
  const open = () => click('[aria-label="打开播放详情"]');
  async function pauseOn(id) {
    const playing = await wait(async () => {
      const state = await player();
      return state.state === 'playing' && state.track?.id === id && state.position > .08 && state;
    }, 'native playing track ' + id);
    const paused = await action({ kind: 'pause', paused: true });
    check(paused.state === 'paused' && paused.playId === playing.playId, 'pause retains the loaded decoder');
    return paused;
  }

  try {
    epoch = await wait(async () => {
      const account = await invoke('account_snapshot');
      return account.status === 'authenticated' && one('.track-row') && account.epoch;
    }, 'synthetic signed-in shell');
    await invoke('play_selection', {
      epoch, selection: { kind: 'songs', ids: ['1', '2'], index: 0, quality: 'standard' },
    });
    const first = await pauseOn('1');
    await open();
    await wait(() => rows().length === 2, 'both contribution rows after lyrics');
    const creditOrder = rows().map(row => row.dataset.lyricCredit).join(',');
    check(creditOrder === 'lyricContributor,translationContributor' &&
      rows()[0].previousElementSibling?.classList.contains('lyric-line'),
    'official ordering after timed lines, not inside them');
    const original = await lyric(first.playId);
    check(original.lyricContributor.userId === '72' && original.translationContributor.userId === '73' &&
      original.lyricContributor.nickname === '合成 <歌词作者>' &&
      !JSON.stringify(original).includes('never-project') &&
      Object.keys(original.lyricContributor).sort().join(',') === 'nickname,userId',
    'native projects nickname and userid only, not contribution id, URL or raw metadata');
    const translated = '[aria-label="翻译歌词"]';
    await wait(() => one(translated)?.getAttribute('aria-pressed') === 'true', 'translation initially shown');
    await capture('credits', { playId: first.playId, translation: true });
    await click(translated);
    await wait(() => one(translated)?.getAttribute('aria-pressed') === 'false', 'translation disabled');
    check(rows().length === 2, 'both credits remain independent from translation visibility');
    await capture('translation_off', { playId: first.playId, translation: false });

    const beforeProfile = await player();
    await click('[data-lyric-credit="translationContributor"] button');
    await wait(() => !one('.lyrics-panel') &&
      one('.playlist-creator-page h1')?.textContent.trim() === '合成署名主页 73',
    'translation contributor opens native user profile 73');
    const profile = await player();
    check(profile.playId === first.playId && profile.entryId === first.entryId &&
      profile.queueRevision === first.queueRevision && profile.state === 'paused' &&
      profile.position === beforeProfile.position, 'profile navigation leaves the paused decoder and queue unchanged');
    await capture('profile', { playId: first.playId, profile: '73', samePlayback: true });
    await click('[aria-label="返回上一页"]');
    await wait(() => !one('.playlist-creator-page'), 'return to originating route');
    await open();
    await wait(() => rows().length === 2, 'same-play credits after return');

    const queue = await invoke('player_queue', { epoch });
    const otherEntry = queue.tracks.find(row => row.track.id === '2').entryId;
    const firstEntry = queue.tracks.find(row => row.track.id === '1').entryId;
    await action({ kind: 'select', entry_id: otherEntry });
    const other = await pauseOn('2');
    await wait(() => rows().length === 1 && rows()[0].textContent.includes('合成无链接作者'),
      'replacement lyric has only its own malformed-id credit');
    const invalid = await lyric(other.playId);
    const oldRejected = await lyric(first.playId).then(() => false, () => true);
    check(oldRejected && invalid.lyricContributor.userId === null &&
      !rows()[0].querySelector('button, a'), 'stale play and contribution-ID fallback rejected');
    await capture('unlinked_credit', { playId: other.playId, oldRejected });

    await fixture('offline', { enabled: true });
    await action({ kind: 'select', entry_id: firstEntry });
    const restored = await pauseOn('1');
    await wait(() => rows().length === 2 && one('.lyric-cache-note')?.textContent.includes('缓存歌词'),
      'transport failure retains same-song cached lyrics and both credits');
    const cached = await lyric(restored.playId);
    check(cached.networkFailed && cached.fromCache && cached.lyricContributor.userId === '72',
      'native only reuses the right same-song cache');
    await capture('cached_credits', { playId: restored.playId, fromCache: true });

    await fixture('omit_credits', { enabled: true });
    await fixture('offline', { enabled: false });
    await click('.lyric-cache-note button');
    await wait(() => !one('.lyric-cache-note') && !rows().length && one('.lyric-line'),
      'successful versioned refresh removes omitted credits but retains lyrics');
    const cleared = await lyric(restored.playId);
    check(!cleared.networkFailed && !cleared.lyricContributor && !cleared.translationContributor &&
      JSON.stringify(cleared.lines) === JSON.stringify(original.lines), 'timed text unchanged');
    await capture('credits_cleared', { playId: restored.playId, lyricsPreserved: true });
    const calls = await fixture('credit_calls');
    check(calls.filter(row => row.profile).map(row => row.profile).join(',') === '73' &&
      calls.filter(row => row.lyric).map(row => [row.lyric.id, row.lyric.lv].join(':')).join(',') ===
        '1:-1,2:-1,1:7,1:7', 'no extra lyric request on navigation and exact versioned refresh');
    await emit({ kind: 'complete', stage: 'complete', ok: true, stageCount: phase, realMusicRequests: 0, calls });
  } catch (error) {
    await emit({ kind: 'failed', stage: 'failed', last, error: String(error?.message ?? error) });
  }
})();
