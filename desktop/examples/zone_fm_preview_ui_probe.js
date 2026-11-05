// Actual mounted zone cards and native player, with sealed metadata and PCM.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const assert = (condition, label) => { if (!condition) throw new Error(label); };
  const wait = async (predicate, label) => {
    for (let i = 0; i < 180; i++) {
      if (await predicate()) return;
      await sleep(65);
    }
    throw new Error(label);
  };
  const click = (button, label) => {
    assert(button && !button.disabled, label);
    button.focus({ preventScroll: true });
    button.click();
  };
  const button = (title) => one(`.vertical-zone button[aria-label="${title}"]`);
  const picture = (title) => button(title)?.querySelector('img')?.getAttribute('src');
  const cdn = (name) => `https://p1.music.126.net/synthetic-zone-${name}.png?param=320y320`;
  async function capture(stage, data) {
    await wait(() => {
      const page = one('.page-view');
      return page && !page.matches('.page-enter-active,.page-leave-active') &&
        getComputedStyle(page).opacity === '1';
    }, `painted page for ${stage}`);
    await sleep(100);
    await emit(stage, { ...data, captureHandshake: true });
  }
  try {
    const curated = () => all('.sidebar button:not(:disabled)').find((node) => node.textContent.trim() === '精选');
    await wait(curated, 'enabled curated entry');
    await emit('resize', { width: 1000, height: 680 });
    await sleep(150);
    if (innerWidth !== 1000 || innerHeight !== 680)
      await emit('resize', { width: 1000 * 1000 / innerWidth, height: 680 * 680 / innerHeight });
    await wait(() => innerWidth === 1000 && innerHeight === 680, 'bounded viewport');
    const { epoch } = await invoke('account_snapshot');
    const player = () => invoke('player_snapshot', { epoch });
    await wait(async () => (await player()).state === 'idle', 'initial native transport is idle');
    const before = await player();
    await capture('ux_01_before_mount', { playId: before.playId });
    click(curated(), 'curated entry');
    await wait(() => one('[data-choice-zone="ROCK"]'), 'registered Rock zone');
    click(one('[data-choice-zone="ROCK"]'), 'Rock zone');
    await wait(() => picture('合成漫游首曲') === cdn('card-11'), 'existing cover before passive response');
    await wait(() => picture('合成漫游首曲') === cdn('album-81') &&
      picture('同规则漫游') === cdn('album-81') &&
      picture('另一首曲封面') === cdn('album-92') &&
      picture('最后一组首曲') === cdn('album-93'), 'bounded passive previews and shared rule artwork');
    assert(picture('保留旧封面') === cdn('card-12'), 'unsafe album cover keeps the existing card cover');
    const idle = await player();
    assert(idle.state === 'idle' && idle.fm === false && idle.playId === before.playId &&
      idle.origin === before.origin && !idle.track, 'passive preview left transport untouched');
    await capture('ux_02_passive_art', {
      album: picture('合成漫游首曲'), duplicate: picture('同规则漫游'),
      fallback: picture('保留旧封面'), playId: idle.playId,
    });
    await wait(() => button('播放合成漫游首曲') && !button('播放合成漫游首曲').disabled, 'explicit play control');
    click(button('播放合成漫游首曲'), 'explicit play from zone');
    await wait(async () => {
      const state = await player();
      return state.fm && state.state === 'playing' && state.track?.id === '81' &&
        state.position > 0.15 && !one('.vertical-zone');
    }, 'real libmpv/null FM begins on the retained first song');
    const state = await player();
    const tracks = (await invoke('player_queue', { epoch })).tracks.map(({ track }) => track.id);
    assert(tracks.join() === '81,84', 'explicit play reuses exact first batch order');
    await capture('ux_03_explicit_play', { tracks, playId: state.playId });
    await emit('complete', { ok: true, stageCount: 3 });
  } catch (error) {
    await emit('failed', {
      message: String(error), stack: error?.stack,
      cardCover: picture('合成漫游首曲'), focus: document.activeElement?.getAttribute('aria-label'),
    });
  }
})();
