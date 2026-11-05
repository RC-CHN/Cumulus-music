// Real Vue/WebView, registered Tauri commands and a sealed native catalogue/PCM fixture.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const assert = (condition, label) => { if (!condition) throw new Error(label); };
  const wait = async (predicate, label, interval = 65) => {
    const deadline = performance.now() + 9750;
    while (performance.now() < deadline) {
      if (await predicate()) return;
      await sleep(interval);
    }
    throw new Error(label);
  };
  const click = (node, label) => {
    assert(node && !node.disabled, `unavailable: ${label}`);
    node.focus({ preventScroll: true });
    node.click();
  };
  const zone = () => one('.vertical-zone');
  const more = () => one('.choice-zone-more');
  const menu = (code) => one(`[data-choice-zone="${code}"]`);
  const button = (label) => one(`.vertical-zone button[aria-label="${label}"]`);
  const tab = (key) => one(`[data-zone-focus="tab:${key}"]`);
  const rail = () => one('[data-zone-module="horizontal"] [data-zone-rail]');
  const back = () => click(one('[aria-label="返回上一页"]'), 'Back');
  const hit = (entry) => {
    if (!entry || entry.disabled) return false;
    const rect = entry.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0 &&
      entry.contains(document.elementFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2));
  };
  const settled = () => {
    const page = one('.page-view');
    return page && !page.matches('.page-enter-active,.page-leave-active') &&
      getComputedStyle(page).opacity === '1';
  };
  let count = 0;
  async function capture(name, values = {}) {
    await wait(settled, `settled page before ${name}`);
    // Allow the settled layout/focus to reach the compositor before the host
    // captures the actual X11 pixels. The DOM can be ready during enter.
    await sleep(100);
    const stage = `ux_${String(++count).padStart(2, '0')}_${name}`;
    if (window.__verticalZoneCase === 'playlist-detail') window.__zonePlaylistStageRecorded = null;
    await emit(stage, {
      ...values, captureHandshake: true,
      width: innerWidth, height: innerHeight,
      zoneTitle: zone()?.getAttribute('aria-label'),
      modules: all('[data-zone-module]').map((node) => node.dataset.zoneModule),
    });
    if (window.__verticalZoneCase === 'playlist-detail')
      await wait(() => window.__zonePlaylistStageRecorded === stage, `native evidence saved for ${stage}`);
  }
  function viewport() {
    return { top: zone().scrollTop, left: rail().scrollLeft };
  }
  async function remember(opener) {
    zone().scrollTop = 85;
    rail().scrollLeft = 125;
    await sleep(200);
    const before = viewport();
    assert(before.top >= 60 && before.left >= 75, 'two nonzero scroll axes');
    opener.focus({ preventScroll: true });
    return before;
  }
  async function restored(label, before) {
    await wait(() => button(label) && document.activeElement === button(label) &&
      Math.abs(viewport().top - before.top) <= 2 &&
      Math.abs(viewport().left - before.left) <= 2, `Back restores ${label} and both axes`);
    return { expected: before, actual: viewport(), focus: document.activeElement.getAttribute('aria-label') };
  }
  async function playlistDetailCase() {
    const normal = '专区普通同号歌单';
    const personalized = '专区合成歌单 204';
    const title = () => one('.playlist-title h1');
    const heading = () => one('.playlist-actions button.primary');
    const row = (id) => one(`.track-row[data-track-id="${id}"]`);
    const detailMarker = () => one('[data-zone-playlist-detail]');
    const description = () => one('.playlist-description-preview p')?.textContent.trim();
    const { epoch } = await invoke('account_snapshot');
    const player = () => invoke('player_snapshot', { epoch });
    let priorPlay;

    async function open(label, isPersonalized, state) {
      const card = button(label);
      assert(card, `zone card ${label}`);
      card.scrollIntoView({ block: 'nearest', inline: 'nearest' });
      await sleep(200);
      const before = viewport();
      click(card, label);
      await wait(() => title()?.textContent.trim() === '专区打开后的合成歌单' && row('87') &&
        heading() && !heading().disabled, 'both entries open the real playlist and complete its tracks');
      assert(!!detailMarker() === isPersonalized, 'the native detail source is scoped to the clicked occurrence');
      assert(description() === (isPersonalized ? '合成个性化歌单详情' : '合成普通歌单详情'),
        'playlist description came from the expected detail reader');
      await capture(state, { personalized: !!detailMarker(), description: description() });
      return before;
    }
    async function started(id, state, target) {
      let snapshot;
      await wait(async () => {
        const next = await player();
        if (next.state !== 'playing' || next.track?.id !== id || next.playId === priorPlay ||
          !(next.position > 0.15) || next.origin?.kind !== 'playlist' || next.origin.id !== '204') return false;
        snapshot = next;
        return true;
      }, `native ${state} play request and decode clock`);
      priorPlay = snapshot.playId;
      const queue = (await invoke('player_queue', { epoch })).tracks.map(({ track }) => track.id);
      assert(queue.join() === '81,84,87', 'playlist order retained at each chosen start');
      await capture(state, { playId: snapshot.playId, trackId: id, position: snapshot.position, queue,
        personalized: !!detailMarker(), focusRetained: target() === document.activeElement });
      await invoke('player_action', { epoch, action: { kind: 'stop' } });
      await wait(async () => (await player()).state === 'idle', 'native player stopped between gestures');
    }

    const ordinaryScroll = await open(normal, false, 'normal_detail');
    click(heading(), 'normal playlist Play All');
    await started('81', 'normal_header', heading);
    back();
    await capture('normal_back', await restored(normal, ordinaryScroll));

    const personalizedScroll = await open(personalized, true, 'mgc_detail');
    click(heading(), 'personalized playlist Play All');
    await started('81', 'mgc_header', heading);
    const rowPlay = () => row('87')?.querySelector('.track-play');
    rowPlay().scrollIntoView({ block: 'nearest' });
    await wait(() => hit(rowPlay()), 'row play button can be clicked');
    click(rowPlay(), 'personalized playlist row play');
    await started('87', 'mgc_row', rowPlay);
    const double = () => row('84');
    double().scrollIntoView({ block: 'nearest' });
    double().focus({ preventScroll: true });
    double().querySelector('.track-title').dispatchEvent(new MouseEvent('dblclick', {
      bubbles: true, cancelable: true, detail: 2,
    }));
    await started('84', 'mgc_double', double);
    back();
    await capture('mgc_back', await restored(personalized, personalizedScroll));

    await open(normal, false, 'normal_reopen');
    click(heading(), 'normal playlist Play All after personalized entry');
    await started('81', 'normal_replay', heading);
  }
  try {
    const curated = () => all('.sidebar button:not(:disabled)').find((node) => node.textContent.trim() === '精选');
    await wait(curated, 'enabled sidebar');
    await emit('resize', { width: 920, height: 620 });
    await sleep(150);
    if (innerWidth !== 920 || innerHeight !== 620)
      await emit('resize', { width: 920 * 920 / innerWidth, height: 620 * 620 / innerHeight });
    await wait(() => innerWidth === 920 && innerHeight === 620, 'bounded viewport');
    if (window.__verticalZoneCase === 'hotspot') {
      const input = one('[aria-label="搜索音乐"]');
      input.value = '合成专区热点';
      input.dispatchEvent(new Event('input', { bubbles: true }));
      input.form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
      const action = () => one('.hot-explanation .hot-action');
      const openingOpacity = () => Number(getComputedStyle(action().closest('.page-view')).opacity);
      await wait(() => action()?.textContent.trim() === '查看专区' && hit(action()) &&
        openingOpacity() > 0.05, 'first visible HotExplain action frame', 8);
      const hotOpenOpacity = openingOpacity();
      const hotOpenDuringEnter = action().closest('.page-view').matches('.page-enter-active');
      assert(!one('.hot-explanation').outerHTML.includes('private-zone-source'), 'raw source not disclosed');
      click(action(), 'HotExplain zone');
      await wait(() => zone()?.getAttribute('aria-label') === '摇滚' &&
        tab('1')?.getAttribute('aria-current') === 'page' && button('专区热门歌单') &&
        !button('专区常规歌单'), 'HotExplain resolves and opens named tab');
      await capture('hotspot_zone', { hotOpenOpacity, hotOpenDuringEnter });
      back();
      await wait(() => action() && document.activeElement === action() && !zone(), 'Back restores HotExplain CTA');
      await capture('hotspot_back', { focus: document.activeElement.textContent.trim() });
      await emit('complete', { ok: true, stageCount: count });
      return;
    }
    click(curated(), '精选');
    await wait(() => menu('ROCK') && more(), 'assigned zone menu');
    assert(all('.choice-tabs-scroll [data-choice-zone]').map((node) => node.dataset.choiceZone).join() ===
      'ROCK,FOLK,ELECTRONIC,HIPHOP,CLASSICAL', 'first five dynamic tags only');
    assert(!menu('STATIC') && !menu('JAZZ') && !one('.choice-tabs').outerHTML.includes('private=synthetic'), 'static duplicate, URL and More hidden');

    if (window.__verticalZoneCase === 'full') {
      click(more(), '更多');
      await wait(() => menu('JAZZ') && document.activeElement === menu('JAZZ'), 'More entry focus');
      await wait(() => {
        const entry = menu('JAZZ');
        const panel = entry?.closest('.choice-zone-panel');
        if (!panel || panel.matches('.menu-enter-active,.menu-leave-active')) return false;
        return hit(entry);
      }, 'More entry can be seen and clicked');
      const background = getComputedStyle(menu('JAZZ').closest('.choice-zone-panel')).backgroundColor;
      assert(/^rgb\(/.test(background), 'More menu has an opaque themed background');
      await capture('menu_open', { focus: document.activeElement.dataset.choiceZone, background });
      menu('JAZZ').dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
      await wait(() => more().getAttribute('aria-expanded') === 'false' && document.activeElement === more(), 'More escape restores focus');
      await capture('menu', { focus: document.activeElement.textContent.trim() });
    }
    click(menu('ROCK'), '摇滚');
    await wait(() => zone()?.getAttribute('aria-label') === '摇滚' && tab('default')?.getAttribute('aria-current') === 'page' &&
      all('[data-zone-module]').length === 4, 'Rock default and four module kinds');
    assert(zone().textContent.includes('专区榜单歌曲') && !zone().textContent.includes('禁止出现的远程动作'), 'rank preview and unknown target filtering');
    if (window.__verticalZoneCase === 'playlist-detail') {
      await playlistDetailCase();
      await emit('complete', { ok: true, stageCount: count });
      return;
    }
    if (window.__verticalZoneCase === 'privacy') {
      assert(zone().textContent.includes('个性化推荐被关闭了噢！') &&
        !button('专区合成歌单 204') && !button('仅个性化网格卡片') && button('专区常规歌单'), 'privacy hides personalized data only');
      await capture('privacy_gate');
      click(one('.vertical-zone-privacy button'), 'privacy CTA');
      await wait(() => one('#settings-privacy') && document.activeElement === one('#settings-privacy'), 'privacy settings focus');
      await capture('privacy_settings', { focus: document.activeElement.id });
      await emit('complete', { ok: true, stageCount: count });
      return;
    }
    await capture('rock_modules');
    const album = '专区合成专辑';
    const playlist = '专区合成歌单 204';
    const albumScroll = await remember(button(album));
    click(button(album), 'album detail');
    await wait(() => one('.catalog-heading h1')?.textContent.trim() === '测试专辑', 'actual album detail');
    back();
    await capture('album_back', await restored(album, albumScroll));
    const playlistScroll = await remember(button(playlist));
    click(button(playlist), 'playlist detail');
    await wait(() => one('.playlist-title h1')?.textContent.trim() === '专区打开后的合成歌单' &&
      one('.track-row[data-track-id="81"]'), 'actual playlist detail');
    back();
    await capture('playlist_back', await restored(playlist, playlistScroll));
    const { epoch } = await invoke('account_snapshot');
    const player = () => invoke('player_snapshot', { epoch });
    click(button(`播放${playlist}`), 'scoped playlist play');
    await wait(async () => {
      const state = await player();
      return state.state === 'playing' && state.track?.id === '81' && state.position > 0.15 &&
        state.origin?.kind === 'playlist' && state.origin.id === '204';
    }, 'authorized playlist on actual libmpv/null');
    const tracks = (await invoke('player_queue', { epoch })).tracks.map(({ track }) => track.id);
    assert(tracks.join() === '81,84', 'playlist queue exact order');
    await capture('playlist_play', { tracks, playId: (await player()).playId });

    click(tab('1'), '热门');
    await wait(() => tab('1')?.getAttribute('aria-current') === 'page' &&
      button('专区热门歌单') && !button(playlist), 'Rock hot tab');
    await capture('rock_hot');
    click(more(), '更多');
    await wait(() => menu('JAZZ'), 'More menu again');
    click(menu('JAZZ'), '爵士');
    await wait(() => zone()?.getAttribute('aria-label') === '爵士' && button('专区爵士歌单') &&
      !one('.vertical-zone-subtabs'), 'classic Jazz fallback');
    await capture('jazz_fallback');
    await emit('complete', { ok: true, stageCount: count });
  } catch (error) {
    await emit('failed', {
      message: String(error), stack: error?.stack, states: count,
      zoneTitle: zone()?.getAttribute('aria-label'), zoneScroll: zone()?.scrollTop,
      railScroll: rail()?.scrollLeft,
      focus: document.activeElement?.getAttribute('aria-label') ?? document.activeElement?.id,
    });
  }
})();
