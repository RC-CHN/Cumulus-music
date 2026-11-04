// Production listening ranks using only the closed synthetic Tauri service.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (truth, label) => { if (!truth) throw Error(label); };
  async function wait(predicate, label) {
    for (let i = 0; i < 180; i++) {
      const result = predicate();
      if (result) return result;
      await sleep(80);
    }
    throw Error(label);
  }
  const button = (label, root = document) => [...root.querySelectorAll('button')].find(
    (item) => item.getAttribute('aria-label') === label || item.textContent.trim() === label,
  );
  function click(item) {
    assert(item && !item.disabled, 'missing or disabled button');
    item.focus({ preventScroll: true });
    item.click();
  }
  const emit = (stage, detail = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...detail },
  });
  const profile = () => one('.playlist-creator-page');
  const rank = () => one('.listening-rank');
  const entry = () => one('[data-profile-rank-entry]');
  const titles = () => [...rank().querySelectorAll('.rank-song strong')].map((node) => node.textContent.trim());
  const counts = () => [...rank().querySelectorAll('.rank-count')].map(
    (node) => node.textContent.replace(/[^0-9]/g, ''),
  );
  const played = () => one('.playerbar')?.dataset;
  const back = () => click(button('返回上一页'));
  let step = 'boot';
  let stageCount = 0;
  async function capture(name) {
    await wait(() => !one('.page-view.page-enter-active, .page-view.page-leave-active'),
      'page transition has settled for ' + name);
    const stage = `ux_${String(++stageCount).padStart(2, '0')}_${name}`;
    assert(document.documentElement.scrollWidth <= innerWidth, 'ranking causes horizontal overflow');
    await emit(stage, { width: innerWidth, height: innerHeight, theme: document.documentElement.dataset.theme });
  }
  async function openProfile(playlist, name) {
    click(button('我的歌单'));
    click(await wait(() => button('打开歌单链接'), 'playlist library'));
    const input = await wait(() => one('[aria-label="歌单分享链接"]'), 'playlist link dialog');
    input.value = String(playlist);
    input.dispatchEvent(new Event('input', { bubbles: true }));
    click(one('.playlist-dialog button.primary'));
    click(await wait(() => !one('.playlist-dialog[open]') && one('button.playlist-creator'), 'playlist author'));
    await wait(() => profile()?.querySelector('h1')?.textContent.trim() === name &&
      profile().getAttribute('aria-busy') === 'false', 'profile loaded: ' + name);
  }
  async function rankRows(expected) {
    await wait(() => rank()?.getAttribute('aria-busy') === 'false' &&
      titles().join('|') === expected.join('|'), 'rank rows: ' + expected.join(', '));
  }
  function period(label) {
    return button(label, rank());
  }
  try {
    await wait(() => one('.track-row'), 'signed in with daily rows');
    await emit('resize', { width: 960, height: 700 });
    await wait(() => innerWidth === 960 && innerHeight === 700, 'isolated small window');

    step = 'missing-guest';
    await openProfile(21, '合成听友 4');
    assert(!entry(), 'missing visibility hides the profile rank entry');
    await capture('missing_guest');

    step = 'hidden-guest';
    await openProfile(22, '合成听友 5');
    assert(!entry(), 'false visibility hides the profile rank entry');
    await capture('hidden_guest');

    step = 'allowed-guest';
    await openProfile(20, '合成听友 3');
    assert(entry()?.textContent.includes('TA的听歌排行') &&
      entry().textContent.includes('累计听歌987654首'), 'visible guest header and shared aggregate');
    await capture('guest_header');
    click(entry());
    await rankRows(['访客周榜第一', '访客周榜第二']);
    assert(rank().querySelector('h1').textContent.trim() === '合成听友 3的听歌排行',
      'guest ranking title identifies the opened user');
    assert(period('最近一周').getAttribute('aria-pressed') === 'true' &&
      period('所有时间').getAttribute('aria-pressed') === 'false', 'default is week');
    assert(counts().length === 0 && !rank().textContent.includes('654321'), 'guest per-song count is private');
    await capture('guest_week');

    step = 'guest-play';
    click(await wait(() => {
      const control = button('播放 访客周榜第二', rank());
      return control && !control.disabled && control;
    }, 'rank song is playable'));
    await wait(() => played()?.trackId === '82' && played().state === 'playing' &&
      Number(played().position) > 0.25, 'native playback from ranking row');
    await capture('guest_play');

    step = 'guest-all-and-back';
    click(period('所有时间'));
    await rankRows(['访客总榜第一']);
    assert(period('所有时间').getAttribute('aria-pressed') === 'true' && counts().length === 0,
      'all-time hides counts for guest too');
    await capture('guest_all');
    back();
    await rankRows(['访客周榜第一', '访客周榜第二']);
    assert(period('最近一周').getAttribute('aria-pressed') === 'true', 'first Back restores week');
    await capture('guest_week_restored');
    back();
    await wait(() => entry() && document.activeElement === entry(), 'second Back restores guest entry focus');
    assert(!rank() && profile().querySelector('h1').textContent.trim() === '合成听友 3',
      'Back returns to the original profile');
    await capture('guest_return');

    step = 'revocation';
    await emit('profile-rank-revoke');
    await wait(() => window.__profileRankRevoked === true, 'synthetic revocation acknowledged');
    click(entry());
    await wait(() => rank()?.textContent.includes('此听歌排行不可查看'), 'destination rechecks revocation');
    assert(titles().length === 0 && counts().length === 0, 'revocation does not reuse rows');
    await capture('guest_revoked');
    back();
    await wait(() => profile()?.querySelector('h1')?.textContent.trim() === '合成听友 3', 'return after revoke');

    step = 'own-profile';
    await openProfile(2, '合成自己');
    assert(entry()?.textContent.includes('我的听歌排行') &&
      entry().textContent.includes('累计听歌7890首'), 'owner sees own entry even without public flag');
    await capture('owner_header');
    click(entry());
    await rankRows(['本人周榜']);
    assert(rank().querySelector('h1').textContent.trim() === '我的听歌排行' &&
      counts().join('|') === '4321', 'owner week shows owner counts');
    await capture('owner_week');
    back();
    await wait(() => entry() && document.activeElement === entry(), 'owner profile return');

    step = 'account-shortcut';
    click(one('.account-button'));
    const shortcut = await wait(() => {
      const dialog = one('dialog[open]');
      const control = dialog && button('我的听歌排行', dialog);
      return control && !control.disabled && control;
    }, 'account rank shortcut');
    click(shortcut);
    await rankRows(['本人周榜']);
    await wait(() => !one('dialog[open]'), 'account modal closes on navigation');
    click(period('所有时间'));
    await wait(() => rank()?.querySelector('[role=alert]')?.textContent.includes('无法读取听歌排行'),
      'explicit failure for own all-time read');
    await capture('owner_retry');
    click(button('重试', rank()));
    await rankRows(['本人总榜']);
    assert(counts().join('|') === '8765', 'owner all-time count after retry');
    await capture('owner_all');
    await emit('complete', { ok: true, stageCount });
  } catch (error) {
    await emit('failed', { ok: false, step, error: String(error?.message || error), stack: String(error?.stack || '') });
  }
})();
