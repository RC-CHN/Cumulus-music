// Actual profile card, IPC and native decoder; service calls are synthetic.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (value, label) => { if (!value) throw Error(label); };
  async function wait(check, label) {
    for (let i = 0; i < 180; i++) {
      const value = check();
      if (value) return value;
      await sleep(80);
    }
    throw Error(label);
  }
  const button = (label) => [...document.querySelectorAll('button')].find(
    (item) => item.getAttribute('aria-label') === label || item.textContent.trim() === label,
  );
  const click = (element) => {
    assert(element && !element.disabled, 'button unavailable');
    element.focus({ preventScroll: true });
    element.click();
  };
  const emit = (stage, detail = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...detail },
  });
  const card = () => one('[data-profile-rank-play]');
  const title = () => card()?.getAttribute('aria-label') ?? '';
  const player = () => one('.playerbar').dataset;
  let count = 0;
  let step = 'boot';
  async function capture(name) {
    await emit(`ux_${String(++count).padStart(2, '0')}_${name}`, { title: title(), width: innerWidth, height: innerHeight });
  }
  async function openProfile(playlist, name) {
    click(button('我的歌单'));
    click(await wait(() => button('打开歌单链接'), 'playlist library'));
    const input = await wait(() => one('[aria-label="歌单分享链接"]'), 'playlist link dialog');
    input.value = String(playlist);
    input.dispatchEvent(new Event('input', { bubbles: true }));
    click(one('.playlist-dialog button.primary'));
    click(await wait(() => !one('.playlist-dialog[open]') && one('button.playlist-creator'), 'playlist creator'));
    await wait(() => one('.playlist-creator-page h1')?.textContent.trim() === name &&
      one('.playlist-creator-page').getAttribute('aria-busy') === 'false', 'profile: ' + name);
  }
  async function active(id, label) {
    await wait(() => player().trackId === id && player().state === 'playing' &&
      Number(player().position) > 0.25 && title().startsWith('暂停'), label);
  }
  try {
    await wait(() => one('.track-row'), 'signed in with daily tracks');
    await emit('resize', { width: 960, height: 700 });
    await wait(() => innerWidth === 960 && innerHeight === 700, 'resized');

    step = 'hidden';
    await openProfile(22, '合成听友 5');
    assert(!card() && !one('[data-profile-rank-entry]'), 'hidden guest has no playback or link');
    await capture('hidden_guest');

    step = 'guest-start';
    await openProfile(20, '合成听友 3');
    const guest = await wait(() => card() && !card().disabled && card(), 'guest play button');
    assert(title() === '播放TA的最近一周听歌排行', 'fresh guest source');
    await capture('guest_available');
    click(guest);
    await active('82', 'guest first eligible row starts');
    assert(one('.playlist-creator-page') && !one('.listening-rank'), 'play stays on profile');
    await wait(() => document.activeElement === card(), 'play button focus restored');
    await capture('guest_first_play');

    step = 'guest-repeat';
    click(button('下一首'));
    await active('87', 'next rank song starts');
    const id = player().playId;
    await capture('guest_next');
    click(card());
    await wait(() => player().state === 'paused' && title().startsWith('继续播放'), 'card pauses exact source');
    assert(player().playId === id && player().trackId === '87', 'pause keeps second song');
    const paused = Number(player().position);
    await capture('guest_paused');
    click(card());
    await wait(() => player().state === 'playing' && player().playId === id &&
      Number(player().position) > paused + 0.15, 'card resumes same native clock');
    await capture('guest_resumed');

    step = 'owner-source';
    await openProfile(2, '合成自己');
    assert(title() === '播放我的最近一周听歌排行' && player().playId === id,
      'another UID cannot claim the same virtual card');
    click(card());
    await active('84', 'owner rank plays separate source');
    const ownerPlay = player().playId;
    assert(ownerPlay !== id, 'another profile chooses its own first song');
    await capture('owner_playing');
    click(one('[data-profile-rank-entry]'));
    await wait(() => one('.listening-rank')?.textContent.includes('本人周榜'), 'separate rank title navigation');
    assert(player().playId === ownerPlay, 'title navigation preserves playback');
    await capture('owner_title_opens');

    step = 'guest-revoked';
    await openProfile(20, '合成听友 3');
    await emit('profile-rank-revoke');
    await wait(() => window.__profileRankRevoked === true, 'revocation fixture acknowledged');
    click(card());
    await wait(() => document.body.textContent.includes('此听歌排行不可查看或播放'), 'fresh guest check fails closed');
    assert(player().playId === ownerPlay && player().trackId === '84' && player().state === 'playing',
      'denied guest start leaves owner playback untouched');
    await capture('guest_revoked');
    await emit('complete', { ok: true, stageCount: count });
  } catch (error) {
    await emit('failed', { ok: false, step, error: String(error?.message || error),
      state: one('.playerbar')?.dataset.state, title: title(), stack: String(error?.stack || '') });
  }
})();
