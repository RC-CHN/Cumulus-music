// Production profile relation routes, native IPC and synthetic read service.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (value, label) => { if (!value) throw Error(label); };
  async function wait(predicate, label) {
    for (let i = 0; i < 180; i++) {
      const result = predicate();
      if (result) return result;
      await sleep(80);
    }
    throw Error(label);
  }
  const button = (label) => [...document.querySelectorAll('button')].find(
    (item) => item.getAttribute('aria-label') === label || item.textContent.trim() === label,
  );
  const click = (item) => {
    assert(item && !item.disabled, 'missing or disabled button');
    item.focus({ preventScroll: true });
    item.click();
  };
  const emit = (stage, detail = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...detail },
  });
  const capture = async (stage) => {
    await sleep(180);
    await emit(`ux_${stage}`, { captureHandshake: true });
    await sleep(360);
  };
  const creator = () => one('.playlist-creator-page');
  const heading = () => one('.profile-relations h1')?.textContent.trim();
  const entry = (kind) => one(`[data-relation-entry="${kind}"]`);
  const card = (id, kind = 'user') => one(`[data-relation-resource="${kind}:${id}"]`);
  const notice = () => one('[data-relation-notice]')?.textContent.trim();
  const back = () => click(button('返回上一页'));
  const list = () => one('.relations-scroll');
  let step = 'source';
  try {
    await wait(() => one('.track-row'), 'daily tracks');
    click(button('我的歌单'));
    click(await wait(() => button('打开歌单链接'), 'library'));
    const input = await wait(() => one('[aria-label="歌单分享链接"]'), 'playlist link dialog');
    input.value = '20';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    click(one('.playlist-dialog button.primary'));
    click(await wait(() => one('button.playlist-creator'), 'playlist creator link'));
    await wait(() => entry('following') && entry('followers'), 'both verified relation counts');
    assert(creator().querySelector('h1').textContent === '合成作者 3', 'profile identity');
    await capture('01_profile');

    step = 'guest-follow';
    click(entry('following'));
    await wait(() => heading() === '合成作者 3的关注' && card(2) && card(101), 'guest following first page');
    assert(card(2).textContent.includes('合成备注') && !card(2).textContent.includes('已互关') &&
      card(101).textContent.includes('已互关'), `public card relation labels: ${card(2).textContent}; ${card(101).textContent}`);
    assert(!one('[data-relation-filter]'), 'guest does not get host filters');
    await capture('02_guest_following');
    click(await wait(() => one('[data-relation-more]'), 'continuation'));
    await wait(() => one('[data-relation-retry]'), 'exact continuation failure');
    assert(card(2), 'previous entries remain during continuation failure');
    await capture('03_retry');
    click(one('[data-relation-retry]'));
    await wait(() => !one('[data-relation-retry]') && !one('[data-relation-more]') && one('.profile-relations').getAttribute('aria-busy') === 'false', 'retry succeeded');
    list().scrollTop = list().scrollHeight;
    await wait(() => card(120), 'second-page card rendered on scroll');
    list().scrollTop = 0;
    await wait(() => card(2), 'first row remounted');

    step = 'own';
    click(card(2));
    await wait(() => creator()?.querySelector('h1').textContent === '合成自己' && entry('following'), 'canonical own user profile');
    click(entry('following'));
    await wait(() => heading() === '我的关注' && card(9, 'artist'), 'own follows all branch');
    assert(card(9, 'artist').textContent.includes('绑定用户签名'), 'artist display uses its associated profile signature');
    click(one('[data-relation-filter="following_artists"]'));
    await wait(() => card(10, 'artist'), 'own artist tab');
    click(one('[data-relation-filter="following_users"]'));
    await wait(() => card(3), 'own user tab');
    await capture('04_own_filters');
    back();
    click(await wait(() => entry('followers'), 'return to own profile'));
    await wait(() => heading() === '合成自己的粉丝' && card(91), 'own fans skip guest privacy');
    await capture('05_own_fans');
    back();
    await wait(() => creator()?.querySelector('h1').textContent === '合成自己', 'own profile return');
    back();
    await wait(() => heading() === '合成作者 3的关注' && document.activeElement === card(2), 'guest list and source card focus restored');
    await capture('06_guest_restored');
    back();
    await wait(() => creator()?.querySelector('h1').textContent === '合成作者 3' && entry('followers'), 'guest profile restored');

    step = 'guest-fans';
    click(entry('followers'));
    await wait(() => notice() === '合成作者限制了查看', 'header privacy denial');
    assert(creator() && !heading(), 'header denial stays on profile');
    await capture('07_denied');
    click(entry('followers'));
    await wait(() => !notice(), 'clears previous denial before retry');
    await wait(() => notice() === '合成作者限制了查看' && creator() && !heading(), 'destination privacy revocation returns to profile');
    click(entry('followers'));
    await wait(() => heading() === '合成作者 3的粉丝' && card(90), 'guest fans allowed after new verification');
    await capture('08_guest_fans');
    await emit('complete', { ok: true });
  } catch (error) {
    await emit('failed', { ok: false, step, error: String(error?.message || error), stack: String(error?.stack || '') });
  }
})();
