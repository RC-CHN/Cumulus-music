(async () => {
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const el = (selector) => document.querySelector(selector);
  const legacy = window.__interceptionLegacy === true;
  const ordinary = window.__interceptionOrdinary === true;
  const guide = () => el('[data-classic-remove-guide]');
  const expiry = () => el('[data-live-jsk-expiry][role="alert"]');
  const localPage = () => el('.local-page[data-local-ready="true"]');
  const localFocusControl = () => localPage()?.querySelector('.local-heading button');
  const nav = (title) => [...document.querySelectorAll('.sidebar-navigation .nav-item')]
    .find((node) => node.textContent.trim() === title);
  const welcome = () => el('.welcome');
  const relogin = () => [...(welcome()?.querySelectorAll('button') ?? [])]
    .find((node) => node.textContent.trim() === '重新登录' && !node.disabled);
  const report = (stage, data = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'auth-proof', payload: { stage, ...data },
  });
  const wait = async (check, name) => {
    const end = Date.now() + 30000;
    while (Date.now() < end) {
      const result = await check();
      if (result) return result;
      await sleep(50);
    }
    throw Error(name);
  };
  const assert = (check, name) => { if (!check) throw Error(name); };
  const account = () => window.__TAURI_INTERNALS__.invoke('account_snapshot');
  async function stage(name, settle = 900) {
    await sleep(name === 'interception_expired' && legacy ? 0 : 350);
    window.__authAck = '';
    await report(name, {
      text: el('.welcome')?.textContent || '',
      signedIn: el('.account-button')?.textContent.includes('邮箱验证账号'),
      guide: guide()?.open ?? false,
      expiryNotice: expiry()?.textContent.trim() ?? null,
      localPage: !!localPage(),
      localFocus: document.activeElement === localFocusControl(),
      fallbackTitle: welcome()?.querySelector('h1')?.textContent.trim() ?? null,
      fallbackLogin: !!relogin(),
    });
    await wait(() => window.__authAck === name, 'native_ack');
    if (settle) await sleep(settle);
  }
  function search(keyword) {
    const input = el('.searchbox input');
    input.value = keyword;
    input.dispatchEvent(new Event('input', { bubbles: true }));
    el('.searchbox').dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
  }
  try {
    await wait(() => el('.account-button')?.textContent.includes('邮箱验证账号') && !el('.loading-state'), 'restored');
    await stage('interception_restored');
    search('fixture-first');
    await wait(() => document.body.textContent.includes('请求未通过登录校验'), 'retained');
    assert(el('.account-button')?.textContent.includes('邮箱验证账号'), 'account_was_lost');
    assert(!guide(), 'action one must not open the recovery guide');
    assert(!expiry(), 'action one must not display an expiry notice');
    await stage('interception_retained');
    await sleep(5200);
    if (ordinary) {
      const local = nav('本地音乐');
      assert(local && !local.disabled, 'local page remains reachable');
      local.scrollIntoView({ block: 'nearest' });
      local.click();
      await wait(() => localFocusControl() && local.getAttribute('aria-current') === 'page' &&
        !el('.page-leave-active,.page-enter-active'), 'settled local page');
      const previous = await account();
      const initialFocus = localFocusControl();
      initialFocus.focus({ preventScroll: true });
      assert(document.activeElement === initialFocus, 'settled focus before background request');
      let shownAt, removedAt;
      const noticeClock = new MutationObserver(() => {
        if (expiry()) shownAt ??= performance.now();
        else if (shownAt !== undefined) removedAt ??= performance.now();
      });
      // Start before either the account event or the failing IPC response can
      // arrive. The screenshots and polling do not define the toast duration.
      noticeClock.observe(document.body, { childList: true, subtree: true });
      try {
        // Exercise production IPC as a protected background read while the
        // user is on the local page.
        const result = await window.__TAURI_INTERNALS__.invoke('search_results', {
          epoch: previous.epoch,
          query: { keyword: 'fixture-second', category: 'all', offset: 0, limit: 10, correct: true, channel: 'typing' },
          defaultReceipt: null,
          correctionReceipt: null,
        }).then(() => null, (error) => error);
        assert(result?.code === 'session_expired', 'original protected request fails without replay');
        await wait(() => shownAt !== undefined && expiry(), 'live expiry reached the shell over local music');
        const after = await account();
        assert(after.status === 'expired' && after.epoch !== previous.epoch && after.liveJskExpiry === true,
          'native account published the live ordinary invalidation');
        assert(expiry().textContent.trim() === '合成会话已经失效 <img>' && !expiry().querySelector('img'),
          'bounded native reason is shown as literal text');
        assert(localPage() && document.activeElement === initialFocus && !guide() && !el('.login-methods'),
          'notice preserves the route and focus, without opening a login surface');
        await stage('interception_ordinary_notice', 0);
        const daily = await wait(() => {
          const target = nav('每日推荐');
          return target && !target.disabled && target;
        }, 'authenticated-only navigation ready');
        daily.click();
        await wait(() => welcome() && relogin() && !el('.page-leave-active,.page-enter-active'),
          'manual re-login fallback on the authenticated-only page');
        assert(expiry() && welcome().querySelector('h1')?.textContent.trim() === '登录已失效' &&
          !welcome().textContent.includes('合成会话已经失效') &&
          !welcome().textContent.includes('合成失效标题') && !el('.login-methods'),
        'server reason appears only in the concurrent global notice');
        await stage('interception_ordinary_welcome', 220);
        nav('本地音乐').click();
        await wait(() => localFocusControl() && !el('.page-leave-active,.page-enter-active'), 'return to local page');
        const returnFocus = localFocusControl();
        returnFocus.focus({ preventScroll: true });
        await wait(() => removedAt !== undefined && !expiry(), 'transient expiry notice removed');
        const duration = removedAt - shownAt;
        assert(duration >= 6500 && duration <= 10000 && localPage() &&
          document.activeElement === returnFocus && !el('.login-methods'),
        'seven-second notice retires without moving focus, route, or opening login');
        await stage('interception_ordinary_retired');
        await report('complete', { ok: true, duration });
      } finally {
        noticeClock.disconnect();
      }
      return;
    }
    search('fixture-second');
    await wait(() => el('.welcome')?.textContent.includes('请重新确认登录'), 'reauth_notice');
    assert(!expiry(), 'explicit reauthentication keeps its existing presentation');
    if (legacy) assert(!guide(), 'classic guide must not appear with the initial invalidation');
    await stage('interception_expired', legacy ? 220 : 900);
    if (!legacy) {
      assert(!guide(), 'new benefit cohort must not open the classic guide');
      const button = [...document.querySelectorAll('.welcome button')]
        .find((node) => node.textContent.trim() === '重新登录');
      button.click();
      await wait(() => el('.login-methods'), 'login_entry');
      await stage('interception_login');
      await report('complete', { ok: true });
      return;
    }

    await wait(() => guide()?.open && guide().querySelector('.qr-frame svg'), 'automatic classic QR');
    assert(guide().querySelector('h2')?.textContent.trim() === '请重新确认登录', 'official title missing');
    assert(guide().querySelector('.classic-remove-guide-reason')?.textContent.trim() ===
      '登录状态已失效，请重新登录', 'official reason missing');
    assert(!guide().querySelector('.login-methods,.login-accounts'), 'classic guide has no unrelated methods');
    const epoch = (await account()).epoch;
    await stage('interception_login', 220);
    const retry = await wait(() => [...guide().querySelectorAll('button')].find(
      (button) => button.textContent.trim() === '点击重试' && !button.disabled,
    ), 'explicit retry after expired classic ticket');
    retry.click();
    await wait(() => guide()?.open && guide().querySelector('.qr-frame svg') &&
      !guide().textContent.includes('二维码已失效'), 'single new synthetic QR after retry');
    await stage('interception_guide_retry', 220);
    const beforeClose = await window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
      event: 'auth-proof', payload: { stage: 'interception_before_close' },
    });
    void beforeClose;
    await wait(() => window.__authAck === 'interception_before_close', 'close checkpoint');
    guide().querySelector('[aria-label="关闭登录"]').click();
    await wait(() => !guide(), 'classic guide closed');
    const snapshot = await account();
    assert(snapshot.epoch === epoch && snapshot.status === 'expired' && !snapshot.classicRemoveGuide,
      'close neither changes account nor leaves the automatic offer reusable');
    await sleep(1250);
    await stage('interception_guide_closed', 220);
    await report('complete', { ok: true, pollsBeforeClose: window.__classicPollsBeforeClose });
  } catch (error) {
    await report('failed', { ok: false, error: error instanceof Error ? error.message : 'unknown' });
  }
})();
