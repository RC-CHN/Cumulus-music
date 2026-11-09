// Actual main WebView actions against the fixed, signed synthetic copyright reads.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const card = () => one('[data-daily-notification]');
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const assert = (value, message) => { if (!value) throw Error(message); };
  const wait = async (test, message) => {
    for (let n = 0; n < 250; n++) {
      if (await test()) return;
      await sleep(60);
    }
    throw Error(message);
  };
  const control = (name, within = document) => [...within.querySelectorAll('button')]
    .find((node) => node.textContent.trim() === name || node.getAttribute('aria-label') === name);
  const click = (node) => {
    assert(node?.isConnected && !node.disabled, 'expected connected, enabled control');
    node.focus({ preventScroll: true });
    node.click();
  };
  const mode = window.__copyrightNotificationCase;
  const noId = mode.startsWith('no_id_');
  const action = noId ? mode.slice('no_id_'.length) : mode;
  let epoch, count = 0, step = 'startup', unsupportedDetailsRejected = false;
  const player = () => invoke('player_snapshot', { epoch });
  const offer = () => invoke('daily_notification', { epoch });
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
      noticeKind: card()?.dataset.notificationKind ?? null,
      songs: [...(card()?.querySelectorAll('li span') ?? [])].map((node) => node.textContent.trim()),
      safeArt: card()?.querySelector('img')?.getAttribute('src') ?? null,
      hasCopyrightSetting: one('[data-daily-notification-preference="copyright"]')?.checked ?? null,
      hasDailySetting: one('[data-daily-notification-preference="daily"]')?.checked ?? null,
      unsupportedDetailsRejected,
      hasPlaylistAction: !!card() && !!control('查看回归歌单', card()),
      hasCreateAction: !!card() && [...card().querySelectorAll('button')].some((button) => button.textContent.includes('创建回归歌单')),
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
    }, 'isolated signed account and original daily view');
    if (action === 'fullscreen') {
      await sleep(Math.max(0, 6700 - (performance.now() - started)));
      assert(!card() && await offer() === null, 'no displayed or offered notification');
      await stage('suppressed');
    } else {
      await wait(() => card()?.getAttribute('aria-busy') === 'false', 'acknowledged notification');
      const notice = await offer();
      const daily = ['empty','error'].includes(action);
      assert(notice?.shown && notice.kind === (daily ? 'daily' : 'copyright'), 'correct arbitration winner');
      if (daily) {
        assert(notice.playlistId === null && notice.songs.length === 3, 'ordinary Daily fallback');
        await stage('daily_fallback');
      } else {
        assert(notice.playlistId === (noId ? null : '81')
          && notice.songs.map((song) => song.name).join(',') === '合成歌曲 3,合成歌曲 5',
          'native copyright receipt and normalized server rows');
        if (noId) {
          assert(!control('查看回归歌单', card()) && !control('创建回归歌单', card()),
            'no disconnected playlist action without an ID');
          try {
            await invoke('daily_notification_command', { epoch, receipt: notice.receipt, action: 'details' });
          } catch {
            unsupportedDetailsRejected = true;
          }
          const afterRejection = await offer();
          assert(unsupportedDetailsRejected && afterRejection?.receipt === notice.receipt && afterRejection.shown,
            'unsupported native Details does not consume the legitimate Play receipt');
        }
        assert(notice.songs[0].coverUrl === 'https://p1.music.126.net/fixture-copyright.jpg?param=320y320'
          && card().querySelector('p').textContent === '有2首你喜欢的歌曲重新上架了！', 'safe photo and real server count');
        await stage('offered');
        if (action === 'play') {
          const old = await player();
          assert(old.state === 'playing' && old.track.id === '2' && old.position > .2,
            'earlier native playback does not disqualify copyright');
          click(control('一键播放', card()));
          await wait(async () => {
            const result = await player();
            return !card() && result.state === 'playing' && result.track?.id === '3' &&
              result.playId !== old.playId && result.position > .25;
          }, 'inserted returning song starts actual null audio');
          await stage('playing');
        } else if (action === 'existing') {
          click(control('查看回归歌单', card()));
          await wait(() => !card() && one('.main-content h1')?.textContent.trim() === '合成回归歌单',
            'primary action reaches exactly the existing playlist');
          await stage('existing');
        } else if (action === 'close') {
          click(control('关闭版权回归通知', card()));
          await wait(() => !card(), 'plain close dismisses the no-ID notice');
          await stage('close');
        } else if (action === 'settings' || action === 'snooze') {
          click(control('更多通知操作', card()));
          const label = action === 'settings' ? '管理桌面通知' : '7天内不再推送';
          await wait(() => !!control(label), 'native notification menu ready');
          click(control(label));
          await wait(() => !card(), 'notice dismissed');
          if (action === 'settings') {
            await wait(() => document.activeElement?.id === 'settings-desktop-notifications'
              && one('[data-daily-notification-preference="copyright"]')?.checked === true
              && one('[data-daily-notification-preference="daily"]')?.checked === true, 'both local switches with settings focus');
            click(one('[data-daily-notification-preference="copyright"]'));
            await wait(async () => !one('[data-daily-notification-preference="copyright"]')?.checked
              && await invoke('daily_notification_enabled', { epoch, kind: 'copyright' }) === false,
              'copyright-only local setting persisted');
          }
          await stage(action);
        }
        assert(await offer() === null, 'explicit action consumed the native receipt');
      }
    }
    await emit('complete', { ok: true, copyrightMode: mode, count });
  } catch (error) {
    await emit('failed', { ok: false, step, message: error.message,
      focus: document.activeElement?.outerHTML.slice(0, 220), body: document.body.innerText.slice(0, 520) });
  }
})();
