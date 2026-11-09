// Real GTK/WebView controls, native startup timer, and null-output playback.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const card = () => one('[data-daily-notification]');
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const assert = (pass, message) => { if (!pass) throw new Error(message); };
  const wait = async (predicate, message) => {
    for (let n = 0; n < 200; n++) {
      if (await predicate()) return;
      await sleep(60);
    }
    throw new Error(message);
  };
  const control = (name, within = document) => [...within.querySelectorAll('button')]
    .find((node) => node.textContent.trim() === name || node.getAttribute('aria-label') === name);
  const click = (node) => {
    assert(node?.isConnected && !node.disabled, 'expected connected, enabled control');
    node.focus({ preventScroll: true });
    node.click();
  };
  const mode = window.__dailyNotificationCase;
  let epoch, count = 0, step = 'startup';
  const player = () => invoke('player_snapshot', { epoch });
  const offer = () => invoke('daily_notification', { epoch });
  const windowFacts = () => Promise.all(
    ['is_focused', 'is_visible', 'is_minimized', 'is_fullscreen',
      'outer_position', 'outer_size', 'current_monitor'].map(async (name) => {
      try { return [name, await invoke('plugin:window|' + name, { label: 'main' })]; }
      catch (error) { return [name, String(error)]; }
    }),
  ).then(Object.fromEntries);
  async function stage(label) {
    step = label;
    await wait(() => !one('.page-enter-active,.page-leave-active'), 'page transition settled');
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await sleep(130);
    const name = 'ux_' + String(++count).padStart(2, '0') + '_' + label;
    window.__dailyNotificationRecorded = '';
    await emit(name, {
      label,
      heading: one('.main-content h1')?.textContent.trim() ?? null,
      noticeTitle: card()?.querySelector('h2')?.textContent.trim() ?? null,
      songs: [...(card()?.querySelectorAll('li span') ?? [])].map((node) => node.textContent.trim()),
      focused: document.activeElement?.id ?? null,
      preference: one('[data-daily-notification-preference]')?.checked ?? null,
      fullscreen: window.innerWidth === screen.width && window.innerHeight === screen.height,
      noOverflow: document.documentElement.scrollWidth <= innerWidth,
    });
    await wait(() => window.__dailyNotificationRecorded === name, 'native saved ' + name);
  }
  try {
    const started = performance.now();
    await wait(async () => {
      const account = await invoke('account_snapshot');
      if (account.status !== 'authenticated' || !one('.track-row')) return false;
      epoch = account.epoch;
      return true;
    }, 'isolated account and initial Daily page');
    const denied = ['privacy', 'fullscreen', 'control'].includes(mode);
    if (denied) {
      await sleep(Math.max(0, 6200 - (performance.now() - started)));
      assert(!card() && await offer() === null, 'no offered or presented notification');
      await stage('suppressed');
    } else {
      const focused = one('[aria-label="搜索音乐"]');
      focused?.focus({ preventScroll: true });
      await wait(() => card()?.getAttribute('aria-busy') === 'false', 'native shown acknowledgement');
      assert(!focused || document.activeElement === focused, 'presentation did not steal focus');
      const notification = await offer();
      assert(notification?.shown && notification.songs.length === 3, 'shown exact receipt');
      assert(card().querySelector('h2')?.textContent === '每日推荐'
        && card().querySelector('p')?.textContent === '合成的每日推荐', 'daily title/verified description');
      assert([...card().querySelectorAll('li span')].map((node) => node.textContent.trim()).join(',')
        === '合成歌曲 1,合成歌曲 2,合成歌曲 3', 'three server-order preview rows');
      assert((await player()).state === 'idle', 'presentation does not start playback');
      await stage('offered');
      if (mode === 'play') {
        click(control('一键播放', card()));
        await wait(async () => {
          const snapshot = await player();
          return !card() && snapshot.state === 'playing' && snapshot.position > .2;
        }, 'real null audio and hidden notification');
        assert(await offer() === null, 'receipt consumed');
        await stage('playing');
      } else if (mode === 'details') {
        click(control('排行榜'));
        await wait(() => one('.main-content h1')?.textContent.trim() === '排行榜' && card(), 'other page before details');
        click(control('查看详情', card()));
        await wait(() => !card() && one('.main-content h1')?.textContent.trim() === '每日推荐',
          'details explicitly opens the Daily page');
        assert(await offer() === null && (await player()).state === 'idle', 'details did not play');
        await stage('details');
      } else if (mode === 'close') {
        click(control('关闭每日推荐通知', card()));
        await wait(() => !card(), 'closed notice');
        assert(await offer() === null, 'receipt consumed');
        await stage('closed');
      } else {
        click(control('更多通知操作', card()));
        const label = {
          settings: '管理桌面通知', snooze: '7天内不再推送', disable: '关闭桌面通知',
        }[mode];
        await wait(() => !!control(label), 'notification menu');
        click(control(label));
        await wait(() => !card(), 'closed notification menu/card');
        if (mode === 'settings') {
          await wait(() => document.activeElement?.id === 'settings-desktop-notifications'
            && one('[data-daily-notification-preference]')?.checked === true,
            'manage enters and focuses desktop notifications');
        }
        assert(await offer() === null && (await player()).state === 'idle', 'dismissal did not play');
        await stage(mode);
      }
    }
    await emit('complete', { ok: true, dailyNotificationMode: mode, count });
  } catch (error) {
    await emit('failed', { ok: false, step, message: error.message, focus: document.activeElement?.outerHTML.slice(0, 300),
      body: document.body.innerText.slice(0, 400), windowFacts: await windowFacts() });
  }
})();
