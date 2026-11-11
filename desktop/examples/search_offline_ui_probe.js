// Real production search route over the sealed native PC fixtures.
(async () => {
  const first = '合成断网前搜索', old = '合成迟到搜索', next = '合成恢复搜索', error = '合成读取失败';
  const one = (selector) => document.querySelector(selector);
  const page = () => one('.search-results-page');
  const offline = () => one('[data-search-offline]');
  const category = () => page()?.querySelector('[data-category][aria-selected="true"]')?.dataset.category;
  const heading = () => page()?.querySelector('h1')?.textContent.trim();
  const ready = () => one('#search-result-panel')?.getAttribute('aria-busy') === 'false';
  const input = () => one('[aria-label="搜索音乐"]');
  const focusNotice = () => one('[data-search-offline-focus]');
  const discovery = () => one('#search-discovery-panel');
  const trackScroll = () => one('#search-result-panel .track-scroll');
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const assert = (value, message) => { if (!value) throw Error(message); };
  let last = 'startup', count = 0, browserOnline = false, held = false, delivered = false;
  const requests = [], discoveryReads = [];
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  const fetch = window.fetch;
  window.fetch = async function (url, options) {
    const query = String(url).endsWith('/search_results') ? JSON.parse(options.body).query : null;
    if (query) requests.push([query.category, query.keyword]);
    if (String(url).endsWith('/search_discovery')) {
      const { kind } = JSON.parse(options.body).query;
      if (kind !== 'defaults') discoveryReads.push(kind);
    }
    const response = await fetch.call(window, url, options);
    if (query?.keyword === old) {
      held = true;
      await gate;
      delivered = true;
    }
    return response;
  };
  async function wait(test, label) {
    last = label;
    for (let i = 0; i < 200; ++i) {
      const value = await test();
      if (value) return value;
      await sleep(60);
    }
    throw Error(label);
  }
  function network(online) {
    browserOnline = online;
    window.dispatchEvent(new Event(online ? 'online' : 'offline'));
  }
  async function submit(word) {
    const node = input();
    node.value = word;
    node.dispatchEvent(new Event('input', { bubbles: true }));
    node.form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await wait(() => input()?.value === word, 'search term ' + word);
  }
  async function result(word, key = 'all') {
    await wait(() => heading() === word && category() === key && ready() &&
      !!one('#search-result-panel .search-result-section') && !offline(), 'result ' + word + '/' + key);
  }
  async function readMode(mode) {
    window.__searchFixtureAck = false;
    await emit('search-fixture', { mode });
    await wait(() => window.__searchFixtureAck, 'sealed result mode ' + mode);
  }
  async function capture(label) {
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await sleep(260);
    const toast = focusNotice(), toastRect = toast?.getBoundingClientRect();
    await emit(`ux_${String(++count).padStart(2, '0')}_${label}`, {
      label, requests: [...requests], browserOnline, hasOffline: !!offline(), category: category() ?? null,
      diagnostics: !!one('.settings-network-diagnostics[open]'), trackScroll: trackScroll()?.scrollTop ?? null,
      searchFocus: {
        focused: document.activeElement === input(), selected: input()?.form.classList.contains('searchbox-selected'),
        gradient: getComputedStyle(input().form).backgroundImage !== 'none', keyword: input().value,
        notice: toast?.textContent.trim() ?? null, alert: toast?.getAttribute('role') ?? null,
        centered: !!toastRect && Math.abs(toastRect.x + toastRect.width / 2 - innerWidth / 2) < 1 &&
          Math.abs(toastRect.y + toastRect.height / 2 - innerHeight / 2) < 1,
        noticeHeight: toastRect?.height ?? null, discovery: !!discovery(), discoveryReads: [...discoveryReads],
      },
    });
  }
  try {
    Object.defineProperty(navigator, 'onLine', { configurable: true, get: () => browserOnline });
    await wait(() => input() && !input().disabled && one('.track-row'), 'account and daily route ready');
    network(false);
    await new Promise(requestAnimationFrame);
    const beforeFocus = input().value;
    input().focus({ preventScroll: true });
    await wait(() => focusNotice()?.textContent.trim() === '网络异常，请检查网络连接' &&
      !focusNotice().classList.contains('search-offline-focus-enter-active'), 'offline search focus warning');
    assert(document.activeElement === input() && input().value === beforeFocus && !discovery() &&
      !input().form.classList.contains('searchbox-selected') && !page() && !requests.length &&
      !discoveryReads.length, 'offline focus preserves input, route and request counts');
    await capture('offline_focus');
    await wait(() => !focusNotice(), 'offline focus warning auto-dismisses');
    assert(document.activeElement === input() && input().value === beforeFocus,
      'dismissal leaves the search box focused');
    await capture('offline_focus_expired');

    await submit(first);
    await wait(() => offline()?.textContent.includes('网络似乎有问题') && !page(), 'offline cold route');
    await capture('cold_offline');

    offline().querySelector('button').click();
    await wait(() => one('.settings-network-diagnostics .network-diagnostics-results'),
      'actual local-only diagnostics');
    await capture('local_diagnostics');
    one('.settings-network-diagnostics [aria-label="关闭网络诊断"]').click();
    await wait(() => !one('.settings-network-diagnostics[open]'), 'close diagnostics');

    network(true);
    await result(first);
    await capture('first_online');
    input().blur();
    input().focus({ preventScroll: true });
    await wait(() => document.activeElement === input() &&
      input().form.classList.contains('searchbox-selected') && !focusNotice() && !discovery(),
      'online focus restores selected style without discovery');
    await capture('online_focus');
    one('[data-category="songs"]').click();
    await result(first, 'songs');
    await wait(() => !one('.search-results-enter-active,.search-results-leave-active'),
      'song category transition settled');
    // Let the initial viewport restore finish before mimicking the user's scroll.
    for (let frame = 0; frame < 6; ++frame) await new Promise(requestAnimationFrame);
    const scroller = await wait(() => trackScroll()?.scrollHeight > trackScroll()?.clientHeight &&
      trackScroll(), 'dedicated song results can scroll');
    scroller.scrollTop = 85;
    const row = await wait(() => scroller.querySelector('[data-track-id="104"] button:not(:disabled)'),
      'song focus anchor');
    row.focus({ preventScroll: true });
    const scrollTop = scroller.scrollTop;
    assert(scrollTop > 0, 'save a nonzero results scroll position');
    await capture('classified');
    assert(Math.abs(scroller.scrollTop - scrollTop) <= 1, 'classified scroll remains stable before going offline');

    network(false);
    await wait(() => offline() && !page(), 'existing result unmounted');
    assert(document.activeElement === offline().querySelector('h1'), 'offline status receives lost result focus');
    await capture('known_offline');
    network(true);
    await result(first, 'songs');
    await wait(() => document.activeElement?.closest('[data-track-id]')?.dataset.trackId === '104' &&
      Math.abs(trackScroll()?.scrollTop - scrollTop) <= 1, 'same category, scroll and result focus restored');
    await capture('restored_category');

    await submit(old);
    await wait(() => held && heading() === old && !ready(), 'native response held before renderer delivery');
    network(false);
    await wait(() => offline() && !page(), 'interrupted result unmounted');
    await submit(next);
    release();
    await wait(() => delivered && offline() && !page(), 'old result delivered into retired visit');
    await capture('interrupted_offline');
    network(true);
    await result(next);
    await capture('new_online');

    await readMode('result_error');
    await submit(error);
    await wait(() => heading() === error && ready() && one('#search-result-panel [role="alert"]') &&
      !offline(), 'reachable service failure is not offline');
    await capture('reachable_error');
    await readMode('good');
    one('#search-result-panel .empty-state button').click();
    await result(error);
    await capture('explicit_retry');
    await emit('complete', { ok: true, stageCount: count, requests, realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    release();
    await emit('failed', { ok: false, last, count, message: String(error),
      requests, heading: heading(), category: category(), browserOnline,
      hasOffline: !!offline(), trackScroll: trackScroll()?.scrollTop ?? null, headings: [...document.querySelectorAll('h1')].map((node) => node.textContent.trim()),
      focused: document.activeElement?.outerHTML.slice(0, 280) });
  }
})();
