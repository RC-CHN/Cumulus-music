// Actual WebView and production settings IPC; only the assignment and OS sink are synthetic.
(async () => {
  const mode = window.__cumulusPowerGuidePhase;
  const marker = 'cumulus.playback.keep-awake-guide-seen.v1';
  const one = (s) => document.querySelector(s);
  const guide = () => one('.header-playback-guide:not([inert])');
  const gear = () => one('.settings-button');
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const check = (condition, label) => {
    if (!condition) throw Error(label);
  };
  const wait = async (predicate, label) => {
    for (let i = 0; i < 250; i++) {
      if (predicate()) return;
      await sleep(50);
    }
    throw Error(label);
  };
  const click = (target) => {
    check(target && !target.disabled, 'expected enabled control');
    target.focus({ preventScroll: true });
    target.click();
  };
  const sidebar = (title) =>
    [...document.querySelectorAll('aside button')].find((button) => button.textContent.trim() === title);
  const emit = (stage, data = {}) =>
    window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
      event: 'ux-proof',
      payload: { stage, ...data },
    });
  let stageCount = 0;
  async function capture(label, data = {}) {
    await sleep(330);
    await emit(`ux_${String(++stageCount).padStart(2, '0')}_${label}`, data);
    await sleep(450);
  }
  function enterHome() {
    click(sidebar('首页'));
    return performance.now();
  }
  try {
    await wait(() => gear() && !gear().disabled && sidebar('首页'), 'main shell is ready');
    const provides = one('#app').__vue_app__._context.provides;
    const core = provides[Reflect.ownKeys(provides).find((key) => key.description === 'core-client')];
    check(core, 'production core adapter');
    const nativeRead = core.playbackPower;
    const nativeSet = core.setPlaybackPower;
    let arrive = false, hold = mode === 'first', release, writes = 0;
    const gate = new Promise((resolve) => { release = resolve; });
    core.playbackPower = async (...args) => {
      const result = await nativeRead(...args);
      if (hold) {
        hold = false;
        arrive = true;
        await gate;
      }
      return result;
    };
    core.setPlaybackPower = async (...args) => {
      writes++;
      return nativeSet(...args);
    };

    if (mode === 'close') {
      await emit('resize', { width: 760, height: 540 });
      await wait(() => innerWidth === 760 && innerHeight === 540, 'narrow window');
      click(one('button[aria-label="外观主题"]'));
      await wait(() => one('.action-menu'), 'appearance menu');
      click([...document.querySelectorAll('.action-menu button')]
        .find((button) => button.textContent.trim() === '浅色'));
      await wait(() => document.documentElement.dataset.theme === 'light', 'light appearance');
    }

    if (mode === 'first') {
      check(!localStorage.getItem(marker), 'fresh profile marker absent');
      const start = enterHome();
      await wait(() => arrive, 'authorized reply is held after the home delay');
      check(performance.now() - start >= 5900 && !guide(), 'no early guide before native reply');
      click(sidebar('每日推荐'));
      await sleep(220);
      release();
      await sleep(220);
      check(!guide() && !localStorage.getItem(marker), 'leaving home cancels late reply without consumption');
      await capture('stale_home_reply_discarded');
    }

    const entered = enterHome();
    await wait(() => one('.discovery-home [data-home-card]'), 'synthetic homepage is visibly populated');
    if (mode === 'first' || mode === 'close') {
      const active = document.activeElement;
      await wait(() => guide(), 'assigned disabled preference offers first-home guide');
      check(performance.now() - entered >= 5900, 'no premature first-home display');
      check(document.activeElement === active, 'the passive guide does not steal focus');
      await wait(() => localStorage.getItem(marker) === '1', 'visible guide remembered on presentation');
      await sleep(330);
      const bounds = guide().getBoundingClientRect(), anchor = gear().getBoundingClientRect();
      const style = getComputedStyle(guide());
      const arrow = parseFloat(style.getPropertyValue('--guide-arrow'));
      check(bounds.left >= 7 && bounds.right <= innerWidth - 7 &&
        Math.abs(bounds.top - anchor.bottom - 8) < 1 &&
        Math.abs(bounds.left + arrow - anchor.left - anchor.width / 2) < 1,
        'popover stays inside the viewport with its arrow aligned under the gear');
      check(guide().textContent.includes('功能上新，音乐播放中保持亮屏不休眠'), 'official wording');
      await capture('guide_presented', { bounds: { left: bounds.left, right: bounds.right } });
      if (mode === 'close') {
        click(guide().querySelector('.header-playback-guide-close'));
        await wait(() => !guide() && document.activeElement === gear(), 'close restores gear focus');
        await capture('guide_closed');
      } else {
        click(guide().querySelector('.header-playback-guide-action'));
        await wait(
          () => document.activeElement?.id === 'settings-playback' &&
            one('input[name="playback-keep-awake"]:not(:disabled)'),
          'CTA opens playback settings and focuses its heading',
        );
        const checkbox = one('input[name="playback-keep-awake"]');
        check(!guide() && !checkbox.checked && writes === 0, 'CTA never enables keep-awake');
        await capture('playback_settings_opened', { focused: document.activeElement.id, checked: checkbox.checked });
      }
    } else {
      await sleep(6800);
      check(!guide(), 'priority, unavailable, enabled or previously seen guide remains hidden');
      check((localStorage.getItem(marker) === '1') === (mode === 'reopen'), 'hidden branch does not write marker');
      await capture('guide_suppressed');
    }
    check(writes === 0, 'no renderer set_power command from the guide');
    await emit('complete', { ok: true, mode, stageCount, writes, marker: localStorage.getItem(marker) });
  } catch (error) {
    await emit('failed', { mode, error: String(error), stageCount });
  }
})();
