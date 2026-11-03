// Actual desktop lyric waterfall, synthetic API plus one explicit append error.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, facts = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...facts } });
  const pane = () => one('.search-result-panel');
  const row = (index) => one(`[data-lyric-index="${index}"]`);
  const loaded = () => (Number(one('.search-lyric-list')?.getAttribute('aria-rowcount')) - 1) / 2;
  const button = (text) => [...document.querySelectorAll('button')]
    .find((node) => node.textContent.trim() === text);
  const assert = (value, label) => { if (!value) throw Error(label); };
  async function wait(predicate, label) {
    for (let i = 0; i < 210; i++) {
      const value = await predicate();
      if (value) return value;
      await sleep(45);
    }
    throw Error(label);
  }
  async function fixture(mode) {
    window.__searchFixtureAck = false;
    await emit('search-fixture', { mode });
    await wait(() => window.__searchFixtureAck, 'fixture mode');
  }
  async function choose(key) {
    one(`[data-category="${key}"]`).click();
    await wait(() => one(`[data-category="${key}"]`)?.getAttribute('aria-selected') === 'true' &&
      pane()?.getAttribute('aria-busy') === 'false' &&
      !one('.search-results-enter-active,.search-results-leave-active'), 'active ' + key);
    await sleep(230);
  }
  async function search(keyword) {
    const field = one('[aria-label="搜索音乐"]');
    field.value = keyword;
    field.dispatchEvent(new Event('input', { bubbles: true }));
    field.form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await wait(() => one('.search-results-page h1')?.textContent.trim() === keyword &&
      pane()?.getAttribute('aria-busy') === 'false', 'new search');
    await choose('lyrics');
    await wait(() => loaded() === 10 && row(0), 'first ten lyrics');
  }
  const bottom = () => { pane().scrollTop = pane().scrollHeight; };
  const windowSize = () => document.querySelectorAll('.search-lyric-result').length;
  let stages = 0;
  async function capture(name) {
    await sleep(400);
    const rows = [...document.querySelectorAll('[data-lyric-index]')];
    for (let i = 1; i < rows.length; i++) {
      if (Number(rows[i].dataset.lyricIndex) !== Number(rows[i - 1].dataset.lyricIndex) + 1) continue;
      assert(rows[i].getBoundingClientRect().top >= rows[i - 1].getBoundingClientRect().bottom - 1,
        'measured lyric rows do not overlap');
    }
    await emit(`ux_${String(++stages).padStart(2, '0')}_${name}`, { loaded: loaded(), mounted: windowSize() });
    await sleep(400);
  }
  const offsets = [];
  try {
    await wait(() => one('.track-row'), 'daily ready');
    await fixture('lyrics');
    const provides = one('#app').__vue_app__._context.provides;
    const core = provides[Reflect.ownKeys(provides).find((key) =>
      typeof key === 'symbol' && key.description === 'core-client')];
    const read = core.searchResults;
    let fail = true, held = false, release;
    const gate = new Promise((resolve) => { release = resolve; });
    core.searchResults = async (...args) => {
      const q = args[1];
      if (q.category === 'lyrics') {
        assert(q.limit === 10, 'lyric page size is ten');
        offsets.push(q.offset);
        if (q.offset === 10 && fail) {
          fail = false;
          throw { code: 'unavailable', message: 'Synthetic append refusal' };
        }
        if (q.offset === 20 && !held) {
          held = true;
          await gate;
        }
      }
      return read(...args);
    };
    await search('合成歌词滚动');
    assert(!one('.search-result-pagination'), 'no numbered paging');
    bottom();
    const retry = await wait(() => button('重试加载更多'), 'explicit retry');
    assert(loaded() === 10, 'failed append preserves current rows');
    await sleep(300);
    assert(offsets.join(',') === '0,10', 'append failure must not loop');
    retry.focus();
    retry.click();
    await wait(() => loaded() === 20, 'retry success');
    assert(document.activeElement === retry, 'retry keeps focus on its control');
    await sleep(120);
    bottom();
    await wait(() => held && one('.search-load-more').getAttribute('aria-busy') === 'true', 'held append');
    await capture('held_append');
    release();
    await wait(() => loaded() === 30, 'held page appends');
    for (const count of [40, 50, 60]) {
      await sleep(120);
      bottom();
      await wait(() => loaded() === count, 'append ' + count);
    }
    assert(windowSize() < 28, 'lyrics DOM remains bounded after many pages');
    pane().scrollTop = 0;
    await wait(() => row(4), 'return to long first-page result');
    const target = row(4);
    target.scrollIntoView({ block: 'start' });
    const expand = target.querySelector('[data-result-action="expand"]');
    expand.focus({ preventScroll: true });
    expand.click();
    await wait(() => expand.getAttribute('aria-expanded') === 'true' &&
      target.getBoundingClientRect().height > 5000, 'expanded row gets its actual height');
    pane().scrollTop += 250;
    await sleep(210);
    const relative = target.getBoundingClientRect().top - pane().getBoundingClientRect().top;
    await choose('songs');
    await choose('lyrics');
    await wait(() => row(4)?.querySelector('[data-result-action="expand"]')?.getAttribute('aria-expanded') === 'true',
      'expanded state restored');
    assert(Math.abs(row(4).getBoundingClientRect().top - pane().getBoundingClientRect().top - relative) < 3,
      'restore anchors inside the expanded lyric');
    assert(document.activeElement === row(4).querySelector('[data-result-action="expand"]'), 'restore exact row control');
    await emit('resize', { width: 760, height: 540 });
    await wait(() => innerWidth === 760 && innerHeight === 540, 'narrow resize');
    await sleep(250);
    const resizedRelative = row(4).getBoundingClientRect().top - pane().getBoundingClientRect().top;
    assert(Math.abs(resizedRelative - relative) < 3,
      `resize retains expanded row anchor: previous=${relative}, new=${resizedRelative}, scroll=${pane().scrollTop}`);
    await capture('expanded_return');
    pane().scrollTop = 0;
    await wait(() => row(0), 'top rows reappear');
    // After collapsing, scrolling near the end must mount the actual deep rows.
    row(4).querySelector('[data-result-action="expand"]').click();
    bottom();
    await wait(() => row(52), 'deep visible lyric rows');
    const chosen = row(52).querySelector('.track-play');
    chosen.click();
    const { epoch } = await invoke('account_snapshot');
    await wait(async () => {
      const native = await invoke('player_snapshot', { epoch });
      return native.state === 'playing' && native.track?.id === '53' && native.queueLen === 60;
    }, 'native playback uses the clicked absolute row');
    await fixture('scroll');
    await search('合成重复歌词命中');
    for (let count = 20; count <= 120; count += 10) {
      await wait(() => {
        if (loaded() === count) return true;
        // Keep scrolling to the measured end as new visible rows replace estimates.
        bottom();
        return false;
      }, 'repeat-ID lyric append ' + count);
    }
    assert(windowSize() < 28, 'deep repeat-ID result DOM stays bounded');
    await capture('bounded_repeat_hits');
    await emit('complete', { ok: true, lyricWaterfall: true, stageCount: stages, offsets,
      nativePlaybackId: '53', duplicateHitsPreserved: true });
  } catch (error) {
    await emit('failed', { message: String(error), offsets, loaded: loaded(), mounted: windowSize(),
      scroll: pane() ? [pane().scrollTop, pane().scrollHeight, pane().clientHeight] : null });
  }
})();
