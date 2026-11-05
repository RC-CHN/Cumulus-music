// Real WebView, authoritative membership IPC and an isolated server response.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (condition, message) => { if (!condition) throw Error(message); };
  async function wait(read, message) {
    for (let attempt = 0; attempt < 175; attempt++) {
      const result = await read();
      if (result) return result;
      await sleep(60);
    }
    throw Error(message);
  }
  const invoke = (...args) =>
    (window.__CUMULUS_PROOF_INVOKE__ ?? window.__TAURI_INTERNALS__.invoke)(...args);
  const emit = (stage, info = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...info } });
  const query = '合成会员刷新';
  const panel = () => one('#search-result-panel');
  const card = () => one('[data-result-key="song:601"]');
  const mv = () => card()?.querySelector('[data-result-action="mv"]')?.getAttribute('aria-label');
  const more = () => card()?.querySelector('[aria-label="更多"], [aria-label^="更多 "]');
  const labels = () => ({
    mv: mv() ?? null,
    vip: card()?.querySelector('.vip-tag')?.textContent ?? null,
    free: [...(card()?.querySelectorAll('.search-original-tag') ?? [])]
      .some((tag) => tag.textContent === '限免'),
  });
  const bar = () => one('.playerbar');
  const footer = (name) => bar()?.querySelector(`button[aria-label="${name}"]`);
  const played = (epoch) => invoke('player_snapshot', { epoch });
  const membership = (epoch) => invoke('member_rights', { epoch, refresh: true });
  let step = 'startup', count = 0, epoch, baseline, focused, original, scrollTop, targetPanel;
  const refreshes = [];

  async function capture(label) {
    step = label;
    const native = await played(epoch);
    const sameDom = original === card() && focused === document.activeElement &&
      targetPanel === panel() && Math.abs(scrollTop - panel().scrollTop) < 1;
    assert(sameDom && one('.search-results-page h1')?.textContent === query &&
      !one('.search-results-enter-active,.search-results-leave-active'),
      'native refresh preserves live search card, focus, scroll and route');
    assert(native.state === 'paused' && native.playId === baseline.playId &&
      native.entryId === baseline.entryId && native.position === baseline.position &&
      native.queueRevision === baseline.queueRevision,
      'search refresh must not change paused native playback');
    await emit(`ux_${String(++count).padStart(2, '0')}_${label}`, {
      labels: labels(), focused: document.activeElement.getAttribute('aria-label'),
      native, sameDom, scrollTop: panel().scrollTop,
      rowKeys: [...panel().querySelectorAll('.search-track-card[data-result-key]')]
        .map((row) => row.dataset.resultKey),
      refreshes,
    });
  }

  try {
    await wait(() => one('.track-row') && footer('列表循环')?.disabled === false, 'ready signed-in desktop');
    await emit('resize', { width: 1040, height: 610 });
    await wait(() => innerWidth === 1040 && innerHeight === 610, 'test window resized');
    ({ epoch } = await invoke('account_snapshot'));
    await invoke('play_selection', {
      epoch, selection: { kind: 'songs', ids: ['701', '702'], index: 0, quality: 'standard' },
    });
    await wait(async () => {
      const value = await played(epoch);
      return value.state === 'playing' && value.position > .2;
    }, 'actual native playback is advancing');
    footer('暂停').click();
    baseline = await wait(async () => {
      const value = await played(epoch);
      return value.state === 'paused' && value.position > .2 && value;
    }, 'native playback is paused');

    // Initial authoritative observation occurs before the search listener is attached.
    await membership(epoch);
    const provides = one('#app').__vue_app__._context.provides;
    const core = provides[Reflect.ownKeys(provides)
      .find((key) => typeof key === 'symbol' && key.description === 'core-client')];
    const refresh = core.searchRefreshPrivileges.bind(core);
    core.searchRefreshPrivileges = async (owner, receipt, observation) => {
      try {
        const result = await refresh(owner, receipt, observation);
        refreshes.push({
          ok: true, fromNativeObservation: !!observation,
          ids: result.value.rows.map((row) => row.id), mvId: result.value.rows[0]?.mvId,
        });
        return result;
      } catch (error) {
        refreshes.push({ ok: false, fromNativeObservation: !!observation });
        throw error;
      }
    };
    const field = one('[aria-label="搜索音乐"]');
    field.value = query;
    field.dispatchEvent(new Event('input', { bubbles: true }));
    field.form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await wait(() => one('.search-results-page h1')?.textContent === query &&
      panel()?.getAttribute('aria-busy') === 'false' && more() &&
      !one('.page-enter-active,.page-leave-active,.search-results-enter-active'),
      'cold comprehensive search and stable card');
    assert(mv()?.startsWith('查看替代 MV') && refreshes.length === 0,
      'restricted initial card comes from search, without extra privilege request');
    targetPanel = panel();
    original = card();
    focused = more();
    focused.focus({ preventScroll: true });
    targetPanel.scrollTop = 52;
    await wait(() => targetPanel.scrollTop > 0 && document.activeElement === focused,
      'nonzero scroll and stable More focus');
    scrollTop = targetPanel.scrollTop;
    await capture('cold_restricted');

    await membership(epoch);
    await wait(() => refreshes.length === 1 && mv()?.startsWith('播放 MV') &&
      labels().vip === 'VIP' && labels().free, 'native rights event updates the ordinary preview');
    assert(refreshes[0].fromNativeObservation && refreshes[0].ok &&
      refreshes[0].mvId === '951' && refreshes[0].ids.join(',') === '601,602,603,604,605,606',
      'precisely six ordinary rows and original MV were projected by Rust');
    await capture('changed_tuple');

    await membership(epoch);
    await sleep(480);
    assert(refreshes.length === 1 && mv()?.startsWith('播放 MV'), 'same tuple does not refresh');
    await capture('same_tuple');

    await membership(epoch);
    await wait(() => refreshes.length === 2, 'rights event read failure observed');
    await sleep(480);
    assert(refreshes.length === 2 && refreshes[1].fromNativeObservation && !refreshes[1].ok &&
      mv()?.startsWith('播放 MV') && labels().free, 'failed privilege read retains prior labels and does not retry');
    await capture('failed_privilege_read');

    await emit('complete', {
      ok: true, stageCount: count, refreshes, realMusicRequests: 0, realAccountWrites: 0,
    });
  } catch (error) {
    await emit('failed', { step, count, error: String(error), refreshes, labels: labels() });
  }
})();
