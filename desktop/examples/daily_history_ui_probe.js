// Real daily-page navigation through the production member IPC.
(async () => {
  const one = (s) => document.querySelector(s);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const emit = (stage, data = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const wait = async (predicate, message) => {
    for (let index = 0; index < 200; index++) {
      const found = predicate();
      if (found) return found;
      await sleep(65);
    }
    throw new Error(message);
  };
  const birthday = window.__cumulusDailyBirthdayProof === true;
  const styleWrite = window.__cumulusStyleWriteProof === true;
  function dailyHeadingMatches() {
    const heading = one('.daily-heading-copy');
    return heading?.classList.contains('daily-birthday') === birthday &&
      heading.querySelector('h1').getAttribute('aria-label') === (birthday ? '生日特别推荐' : '每日推荐') &&
      heading.querySelector('.daily-description').textContent.trim() === (birthday
        ? '亲爱的界面体验验证，祝您生日快乐🎂'
        : '根据你的音乐口味生成，每天 6:00 更新');
  }
  try {
    await wait(() => one('.daily-heading') && one('.track-row'), 'daily page ready');
    await wait(dailyHeadingMatches, 'birthday or normal heading from native daily data');
    await emit('resize', { width: 760, height: 540 });
    await wait(() => innerWidth === 760 && innerHeight === 540, 'compact window');
    const shortcut = await wait(() => one('.daily-history-entry'), 'history shortcut');
    const actions = one('.daily-track-actions');
    const links = one('.daily-links');
    const playAll = actions?.querySelector('.primary');
    if (!actions?.contains(links) || !links.contains(shortcut) || !playAll ||
        links.getBoundingClientRect().height !== 36 ||
        links.getBoundingClientRect().left <= playAll.getBoundingClientRect().right)
      throw new Error('daily shortcuts are not grouped at the right of playback controls');
    const bounds = shortcut.getBoundingClientRect();
    if (!shortcut.contains(document.elementFromPoint(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2))) throw new Error('history shortcut clipped');
    await sleep(200);
    await emit('ux_01_daily_history_shortcut');
    const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
    const account = await invoke('account_snapshot');
    if ('birthday' in account || 'birthdayToday' in account) throw new Error('birthday leaked into AccountView');
    const { epoch } = account;
    await wait(() => !playAll.disabled, 'daily play available');
    playAll.click();
    await wait(() => one('.playerbar')?.dataset.trackId === '1' &&
      one('.playerbar')?.dataset.state === 'playing', 'daily source selected through the toolbar');
    const dailyPlayer = await invoke('player_snapshot', { epoch });
    if (dailyPlayer.track?.id !== '1' || dailyPlayer.queueLen < 2 || dailyPlayer.state !== 'playing')
      throw new Error('daily source did not reach native queue and playback');
    one('.playerbar [aria-label="停止播放"]').click();
    await wait(() => one('.playerbar')?.dataset.state === 'idle', 'daily playback stopped');
    shortcut.click();
    await wait(() => one('.member-page')?.getAttribute('aria-busy') === 'false' &&
      one('.member-page .track-row')?.textContent.includes('合成历史日推单曲'), 'selected history loaded');
    if (one('.member-tabs button[aria-pressed="true"]')?.textContent.trim() !== '历史日推') throw new Error('wrong member section');
    await sleep(250);
    await emit('ux_02_history_from_daily');
    await sleep(500);
    one('[aria-label="返回上一页"]').click();
    await wait(() => one('.daily-heading') && one('.daily-history-entry') && one('.track-row'), 'returned to daily');
    const cached = await wait(() => [...document.querySelectorAll('.notice')].find((node) =>
      node.textContent.includes('本账号上次加载的列表')), 'same-account daily cache visible after outage');
    await wait(dailyHeadingMatches, 'same birthday policy on cached daily');
    await emit('ux_05_daily_cached');
    const retry = cached.querySelector('button');
    retry.focus();
    retry.click();
    await wait(() => cached.getAttribute('aria-busy') === 'true' && one('.track-row') &&
      document.activeElement === retry, 'retry keeps cached rows and keyboard focus while pending');
    await wait(() => !cached.isConnected && one('.track-row') &&
      document.activeElement === one('.daily-heading h1'), 'retry returns fresh recommendations and restores focus');
    const style = one('.daily-style-entry');
    const styleBounds = style.getBoundingClientRect();
    if (!style.contains(document.elementFromPoint(styleBounds.x + styleBounds.width / 2, styleBounds.y + styleBounds.height / 2)))
      throw new Error('style shortcut clipped');
    style.click();
    await wait(() => [...document.querySelectorAll('.page-heading h1')].some((heading) =>
      heading.textContent.trim() === '风格推荐') && one('.track-row')?.textContent.includes('合成风格歌曲 71'), 'style recommendations loaded');
    await sleep(250);
    await emit('ux_03_style_recommendations');
    const sticky = () => one('.recommendation-sticky');
    const queueIds = async () => (await invoke('player_queue', { epoch })).tracks.map((entry) => entry.track.id);
    async function scrollRecommendations(label) {
      if (sticky()) throw new Error('recommendation toolbar visible at the top');
      const scroller = one('.track-table .track-scroll');
      scroller.scrollTop = 175;
      await wait(() => sticky()?.getAttribute('aria-label') === label &&
        !one('.page-heading')?.getClientRects().length && !one('.track-actions')?.getClientRects().length,
      'recommendation toolbar replaces the full controls at list scroll');
      return scroller;
    }
    const inline = await wait(() => one('.style-queue-all:not(:disabled)'), 'style inline append available');
    inline.click();
    await wait(() => one('.batch-operation-feedback')?.textContent.includes('已添加全部'), 'style inline append confirmed');
    const withStyle = await queueIds();
    if (withStyle.length !== 60 || withStyle.filter((id) => Number(id) >= 71).join(',') !== Array.from({length:20}, (_, i) => i + 71).join(','))
      throw new Error('style inline append lost daily queue or style order');
    const styleScroll = await scrollRecommendations('风格推荐快捷操作');
    await emit('ux_10_style_sticky');
    sticky().querySelector('[aria-label="播放全部推荐歌曲"]').click();
    let player;
    for (let i = 0; i < 150; i++) {
      player = await invoke('player_snapshot', { epoch });
      if (player.track?.id === '71' && player.state === 'playing' && player.position > 0.15) break;
      await sleep(65);
    }
    if (player?.track?.id !== '71' || player.queueLen !== 20 || player.state !== 'playing' || player.position <= 0.15)
      throw new Error('style songs did not reach ordinary native playback');
    styleScroll.scrollTop = 0;
    await wait(() => !sticky(), 'style toolbar hides at list start');
    const tagEntry = one('.style-tags-entry');
    tagEntry.click();
    let tags = await wait(() => one('.style-tag-browser[open] .style-tag-current') &&
      one('.style-tag-browser'), 'style tags loaded');
    const applied = () => tags.querySelector('.style-tag-applied')?.textContent ?? '';
    const active = tags.querySelector('nav button[aria-pressed="true"]');
    if (active?.textContent.trim() !== '合成曲风' || !applied().includes('合成轻音乐') ||
      applied().includes('合成节拍') || tags.querySelectorAll('.style-tag-current').length !== 1)
      throw new Error('style browser distinguishes the applied tag from other catalog tags');
    await sleep(250);
    await emit('ux_04_style_tags');
    if (styleWrite) {
      const tag = [...tags.querySelectorAll('ul[aria-label="当前分类标签"] button')].find((node) => node.textContent.trim() === '合成节拍');
      const action = (label) => [...tags.querySelectorAll('button')].find((node) => node.textContent.trim() === label);
      tag.click();
      await wait(() => !action('确认更新风格标签').disabled && tags.querySelector('.style-tag-applied').textContent.includes('合成轻音乐'), 'style draft retains applied state');
      [...tags.querySelectorAll('nav button')].find((node) => node.textContent.trim() === '合成空分类').click();
      await wait(() => tags.textContent.includes('此分类暂无标签。') && tags.querySelector('.style-tag-draft')?.textContent.includes('合成曲风 · 合成节拍'), 'style draft stays visible when browsing another category');
      action('确认更新风格标签').click();
      await wait(() => action('核对服务端标签') && !action('核对服务端标签').disabled &&
        action('确认更新风格标签').disabled, 'unknown style write disables resubmission');
      await emit('ux_12_style_uncertain');
      tags.querySelector('[aria-label="关闭风格标签"]').click();
      await wait(() => !one('.style-tag-browser[open]') && document.activeElement === tagEntry, 'unknown operation dialog closes safely');
      tagEntry.click();
      tags = await wait(() => one('.style-tag-browser[open]'), 'reopen style tags for recovery');
      await wait(() => action('核对服务端标签') && !action('核对服务端标签').disabled &&
        action('确认更新风格标签').disabled && tags.querySelector('.style-tag-unresolved')?.textContent.includes('合成曲风 · 合成节拍'), 'reopened dialog recovers the exact unresolved style write');
      action('核对服务端标签').click();
      await wait(() => tags.querySelector('.style-tag-applied')?.textContent.includes('合成节拍') && tags.textContent.includes('风格标签已更新'), 'read-only reconciliation confirms applied style');
      await emit('ux_13_style_confirmed');
    }
    [...tags.querySelectorAll('nav button')].find((node) => node.textContent.trim() === '合成空分类').click();
    await wait(() => tags.textContent.includes('此分类暂无标签。'), 'style category changes without a write');
    if (!applied().includes(styleWrite ? '合成节拍' : '合成轻音乐') || tags.querySelector('.style-tag-current'))
      throw new Error('browsing a different category changes neither applied tags nor their identity');
    tags.querySelector('[aria-label="关闭风格标签"]').click();
    await wait(() => !one('.style-tag-browser[open]') && document.activeElement === tagEntry, 'style tag focus restored');
    const bar = () => one('.playerbar');
    bar().querySelector('[aria-label="暂停"]').click();
    await wait(() => bar().dataset.state === 'paused', 'pause style song before daily append');
    const paused = await invoke('player_snapshot', { epoch });
    const stylePausedScroll = await scrollRecommendations('风格推荐快捷操作');
    sticky().querySelector('[aria-label="添加全部推荐歌曲至播放列表"]').click();
    await sleep(0);
    await wait(() => one('.batch-operation-feedback')?.textContent.includes('已添加全部'), 'sticky style append confirmed');
    if ((await invoke('player_snapshot', { epoch })).playId !== paused.playId)
      throw new Error('sticky style append replaced paused entry');
    sticky().querySelector('[aria-label="添加全部推荐歌曲至播放列表"]').focus();
    stylePausedScroll.scrollTop = 0;
    await wait(() => !sticky() && document.activeElement === one('.style-queue-all'), 'style toolbar hides and restores inline action focus');
    one('[aria-label="返回上一页"]').click();
    const trigger = () => one('.daily-track-actions [aria-label="每日推荐更多操作"]');
    async function menu() {
      await wait(() => trigger() && !trigger().disabled, 'daily menu available');
      trigger().click();
      const list = await wait(() => trigger()?.getAttribute('aria-expanded') === 'true' &&
        [...document.querySelectorAll('.action-menu[role="menu"]')].find((node) =>
          !node.closest('[inert]') && node.querySelector('[data-menu-key="queue"]') &&
          getComputedStyle(node).visibility === 'visible'), 'daily menu open');
      if ([...list.querySelectorAll('[data-menu-key]')].map((node) => node.textContent.trim()).join('|') !== '批量操作|添加全部至播放列表')
        throw new Error('wrong daily menu items');
      return list;
    }
    let list = await menu();
    await wait(() => Number(getComputedStyle(list).opacity) > .99, 'daily menu settles before capture');
    await emit('ux_06_daily_menu');
    list.querySelector('[data-menu-key="queue"]').click();
    await wait(() => document.body.textContent.includes('已添加全部至播放列表'), 'append all feedback');
    const appended = await invoke('player_snapshot', { epoch });
    const all = (await invoke('player_queue', { epoch })).tracks.map((entry) => entry.track.id);
    if (appended.playId !== paused.playId || appended.state !== 'paused' || Math.abs(appended.position - paused.position) > .05 ||
        all.length !== 60 || all.slice(0, 4).join(',') !== '71,1,2,3' || all.at(-1) !== '90')
      throw new Error('append all did not preserve paused playback and order');
    const dailyScroll = await scrollRecommendations('每日推荐快捷操作');
    if (birthday && !sticky().textContent.includes('生日特别推荐')) throw new Error('sticky title lost birthday variant');
    await emit('ux_11_daily_sticky');
    sticky().querySelector('[aria-label="添加全部推荐歌曲至播放列表"]').click();
    await sleep(0);
    await wait(() => one('.batch-operation-feedback')?.textContent.includes('已添加全部'), 'sticky daily append confirmed');
    if ((await invoke('player_snapshot', { epoch })).playId !== paused.playId) throw new Error('sticky daily append replaced paused entry');
    sticky().querySelector('[aria-label="添加全部推荐歌曲至播放列表"]').focus();
    dailyScroll.scrollTop = 0;
    await wait(() => !sticky() && document.activeElement === trigger(), 'daily toolbar hides and restores menu focus');
    list = await menu();
    list.querySelector('[data-menu-key="batch"]').click();
    const batch = await wait(() => one('.daily-batch-page [data-batch-index="2"]')?.closest('.daily-batch-page'), 'daily batch route');
    const batchButton = (label) => [...batch.querySelectorAll('button')].find((node) => (node.getAttribute('aria-label') || node.textContent.trim()) === label);
    if (!batchButton('添加所选至播放列表').disabled) throw new Error('empty batch is not disabled');
    batch.querySelector('[data-batch-index="2"]').click();
    batch.querySelector('[data-batch-index="0"]').click();
    await wait(() => !batch.closest('.page-enter-active'), 'batch page settles before capture');
    await emit('ux_07_daily_batch');
    batchButton('添加所选至播放列表').click();
    await wait(() => batch.querySelector('.batch-operation-feedback')?.textContent.includes('已添加所选'), 'append selected feedback');
    const selectedQueue = (await invoke('player_queue', { epoch })).tracks.map((entry) => entry.track.id);
    if ((await invoke('player_snapshot', { epoch })).playId !== paused.playId || selectedQueue.slice(0, 4).join(',') !== '71,1,3,2')
      throw new Error('selected append lost source order or current playback');
    for (const [label, dialog] of [['下载', '.download-dialog'], ['添加到歌单', '.playlist-collect-dialog']]) {
      batchButton(label).click();
      const opened = await wait(() => one(dialog + '[open]'), label + ' dialog');
      opened.querySelector('button[aria-label^="关闭"]').click();
      await wait(() => !one(dialog + '[open]'), label + ' dialog closed');
    }
    batchButton('播放所选歌曲').click();
    await wait(() => bar().dataset.trackId === '1' && bar().dataset.state === 'playing', 'selected daily songs started');
    const selectedPlay = (await invoke('player_queue', { epoch })).tracks.map((entry) => entry.track.id);
    if (selectedPlay.join(',') !== '1,3') throw new Error('selected playback queue has unselected tracks');
    await emit('ux_08_daily_batch_play');
    batchButton('完成').focus();
    batchButton('完成').click();
    await wait(() => trigger() && document.activeElement === trigger(), 'return focus to daily menu');
    bar().querySelector('[aria-label="播放队列"]').click();
    const clear = await wait(() => [...document.querySelectorAll('.queue-panel header button')].find((node) => node.textContent.trim() === '清空' && !node.disabled), 'clear queue button');
    clear.click();
    const confirm = await wait(() => one('.queue-clear-dialog[open] .primary:not(:disabled)'), 'clear queue confirmation');
    confirm.click();
    await wait(() => !one('.queue-clear-dialog[open]') && bar().dataset.state === 'idle', 'clear queue completed');
    one('.queue-panel [aria-label="关闭播放队列"]').click();
    list = await menu();
    list.querySelector('[data-menu-key="queue"]').click();
    await wait(() => bar().dataset.trackId === '1' && bar().dataset.state === 'playing', 'append all starts empty queue');
    if ((await invoke('player_queue', { epoch })).tracks.length !== 40) throw new Error('empty queue did not load all daily tracks');
    await emit('ux_09_daily_empty_queue');
    await emit('complete', { ok: true, stageCount: styleWrite ? 13 : 11, dailyPlayback: true, dailyCache: true, dailyBirthday: birthday, stylePlayback: true, dailyBatch: true, stickyRecommendations: true, styleWrite });
  } catch (error) {
    await emit('failed', { message: String(error), body: document.body.innerText.slice(-2200) });
  }
})();
