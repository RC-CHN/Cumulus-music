(async () => {
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const all = (selector, root = document) => [...root.querySelectorAll(selector)];
  const one = (selector) => document.querySelector(selector);
  const emit = (stage, rest = {}) =>
    window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
      event: 'ux-proof', payload: { stage, ...rest },
    });
  const assert = (condition, label) => { if (!condition) throw Error(label); };
  async function wait(test, label) {
    for (let attempt = 0; attempt < 200; attempt++) {
      if (test()) return;
      await sleep(80);
    }
    throw Error(label);
  }
  function button(text, root = document) {
    return all('button', root).find((element) => element.textContent.trim() === text);
  }
  function click(element) {
    assert(element && !element.disabled, 'click target unavailable');
    element.scrollIntoView({ block: 'nearest' });
    element.focus({ preventScroll: true });
    element.click();
  }
  let stageCount = 0;
  async function capture(name, playing = false) {
    const stage = `ux_${String(++stageCount).padStart(2, '0')}_${name}`;
    const page = one('.follow-release-page');
    const overflow = document.documentElement.scrollWidth > innerWidth;
    assert(!overflow, 'horizontal overflow');
    await emit(stage, {
      playing, overflow, theme: document.documentElement.dataset.theme,
      width: innerWidth, height: innerHeight, rows: page?.querySelectorAll('article').length ?? 0,
    });
    await wait(() => window.__CUMULUS_ALBUM_CAPTURE_ACK__ === stage, stage + ' capture ack');
  }
  try {
    await wait(() => one('.track-row'), 'account ready');
    await emit('resize', { width: 760, height: 540 });
    await wait(() => innerWidth === 760, 'small window');
    click(one('[aria-label="外观主题"]'));
    await wait(() => button('浅色'), 'theme menu');
    click(button('浅色'));
    await wait(() => document.documentElement.dataset.theme === 'light', 'light theme');
    click(button('专辑与关注'));
    await wait(() => button('关注歌手'), 'collection route');
    click(button('关注歌手'));
    await wait(() => button('新歌发布'), 'followed artist entry');
    click(button('新歌发布'));
    await wait(() => all('.follow-release-page [role=status]').some((status) => status.textContent.includes('正在加载新歌发布')), 'loading');
    await capture('initial_loading');
    await emit('follow-new-song-allow-first');
    await wait(() => all('.follow-release').length === 9, 'first release page');
    assert(one('.follow-release-play-all').disabled, 'failed playall must be disabled');
    assert(one('.follow-release-page').textContent.includes('以下是历史内容'), 'history marker');
    click(button('重试播放列表'));
    await wait(() => one('.follow-release-play-all')?.textContent.includes('2'), 'playall retry');
    assert(document.activeElement === one('.follow-release-play-all'), 'playall retry focus restored');
    await capture('first_page');
    click(all('.follow-release-expand').find((control) => control.textContent.includes('伴奏')));
    await wait(() => button('合成伴奏'), 'song expansion');
    click(all('.follow-release-expand').find((control) => control.textContent.includes('专辑歌曲')));
    const mvTarget = '[aria-label="打开 合成专辑曲目 的 MV"]';
    await wait(() => one(mvTarget), 'album expansion');
    const mv = one(mvTarget);
    assert(mv && !mv.disabled && !button('合成伴奏'), 'album tracks and separate MV');
    await capture('album_expanded');
    click(one('[aria-label="外观主题"]'));
    await wait(() => button('深色'), 'theme menu');
    click(button('深色'));
    await wait(() => document.documentElement.dataset.theme === 'dark', 'dark theme');
    const more = button('加载更多', one('.follow-release-page'));
    more.scrollIntoView({ block: 'end' });
    await wait(() => one('.follow-release-page [role=alert]'), 'scroll continuation failure');
    assert(all('.follow-release').length === 9, 'next page error preserves data');
    await capture('next_error');
    click(button('重试', one('.follow-release-page')));
    await wait(() => button('合成较早新歌'), 'next retry');
    assert(all('.follow-release').length === 10 && !button('加载更多', one('.follow-release-page')), 'pagination finish');
    await capture('next_recovered');
    const play = one('.follow-release-play-all');
    await wait(() => !play.disabled, 'playall enabled');
    click(play);
    await wait(() => Number(one('.playerbar')?.dataset.position) > 0.25, 'latest PCM advances');
    await capture('play_latest', true);
    if (window.__cumulusFollowNoticeProof) {
      async function confirmation(artist) {
        click(one(`[aria-label="合成新歌歌手 ${artist} 更多操作"]`));
        await wait(() => button('不接收此歌手的通知'), 'notification menu');
        click(button('不接收此歌手的通知'));
        await wait(() => one('dialog[open]')?.textContent.includes('后续将屏蔽') ||
          one('dialog[open]')?.textContent.includes('未收到确认'), 'notification confirmation');
        await wait(() => ['取消', '关闭'].includes(document.activeElement?.textContent.trim()),
          'safe initial focus for notification dialog');
      }
      await confirmation(1);
      click(button('取消', one('dialog[open]')));
      await wait(() => !one('dialog[open]') && document.activeElement?.getAttribute('aria-label') ===
        '合成新歌歌手 1 更多操作', 'cancellation restores menu focus');
      assert(all('.follow-release').length === 10, 'cancel keeps feed');
      await capture('notification_cancelled');

      await confirmation(2);
      click(button('确认', one('dialog[open]')));
      await wait(() => one('dialog[open]')?.textContent.includes('不会再次发送'), 'lost reply visible');
      click(button('关闭', one('dialog[open]')));
      await wait(() => !one('dialog[open]'), 'unknown result can close');
      await confirmation(2);
      assert(!button('确认', one('dialog[open]')) && all('.follow-release').length === 10,
        'unknown does not allow another write or hide cards');
      await capture('notification_unknown');
      click(button('关闭', one('dialog[open]')));
      await wait(() => !one('dialog[open]'), 'close recovered journal');

      await confirmation(1);
      click(button('确认', one('dialog[open]')));
      await wait(() => all('.follow-release').length === 1 && !one('dialog[open]'), 'confirmed hides all cards for artist');
      assert(one('.follow-release')?.getAttribute('aria-label') === '合成新歌歌手 2', 'other artist remains');
      assert(document.activeElement === one('#follow-release-title'), 'focus recovers after removing artist');
      await capture('notification_confirmed');
    }
    click(one('[aria-label="返回上一页"]'));
    await wait(() => button('关注歌手')?.getAttribute('aria-current') === 'page', 'return to followed artists');
    await wait(() => document.activeElement === button('新歌发布'), 'return focus to new-song entry');
    await capture('return_to_followed');
    await emit('complete', { ok: true, stageCount, realMusicRequests: 0, latestPlayback: true });
  } catch (error) {
    await emit('failed', { ok: false, stageCount, error: String(error),
      focus: document.activeElement?.getAttribute('data-collection-key') ?? document.activeElement?.tagName,
    });
  }
})();
