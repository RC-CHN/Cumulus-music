(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const all = (selector, root = document) => [...root.querySelectorAll(selector)];
  const byText = (text, root = document) => all('button', root).find((node) => node.textContent.trim() === text);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (value, label) => { if (!value) throw Error(label); };
  async function wait(test, label) {
    for (let i = 0; i < 180; i++) {
      if (test()) return;
      await sleep(70);
    }
    throw Error(label);
  }
  function click(node) {
    assert(node && !node.disabled, 'control available');
    node.focus({ preventScroll: true });
    node.click();
  }
  const emit = (stage, data = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const pane = () => one('.podcast-page');
  const recent = () => one('[data-podcast-recent]');
  const row = (id) => one(`[data-recent-voice="${id}"]`);
  const list = () => one('.recent-rows');
  const ids = () => all('[data-recent-voice]').map((node) => node.dataset.recentVoice);
  const size = () => Number(list()?.firstElementChild?.getAttribute('aria-setsize') ?? 0);
  const player = () => one('.playerbar');
  const queue = (epoch) => window.__TAURI_INTERNALS__.invoke('player_queue', { epoch });
  const double = (id) => row(id).dispatchEvent(new MouseEvent('dblclick', { bubbles: true, detail: 2 }));
  let index = 0;
  const capture = (name) => emit(`ux_${String(++index).padStart(2, '0')}_${name}`, {
    ids: ids(), size: size(), actual: document.visibilityState === 'visible',
    scroll: pane()?.scrollTop, state: player()?.dataset.state,
  });
  try {
    await wait(() => byText('我的播客') && !byText('我的播客').disabled, 'library navigation');
    click(byText('我的播客'));
    await wait(() => one('[data-podcast-recent-tab]') && !one('[data-podcast-recent-tab]').disabled, 'authenticated library');
    click(one('[data-podcast-recent-tab]'));
    await wait(() => one('[role="alert"]', recent()), 'initial request error');
    await capture('first_error');
    click(byText('重试', recent()));
    await wait(() => row(501) && size() === 18, 'first page');
    assert(ids().length < 18 && row(501).textContent.includes('00:12') &&
      row(502).textContent.includes('已听完') && one('button.recent-play', row(505)).disabled, 'bounded metadata');
    await capture('first_page');

    click(one('.recent-radio', row(501)));
    await wait(() => pane()?.querySelector('h1').textContent === '播客详情' &&
      one('[data-voice-id="501"]', pane()), 'source radio');
    await capture('radio_detail');
    click(one('[aria-label="返回上一页"]'));
    await wait(() => row(501) && size() === 18 && document.activeElement === one('.recent-radio', row(501)),
      'restore recent and its opener without request');
    await capture('back');

    const account = await window.__TAURI_INTERNALS__.invoke('account_snapshot');
    click(one('button.recent-play', row(501)));
    await wait(() => row(501)?.classList.contains('current') &&
      player()?.dataset.state === 'playing' && Number(player().dataset.position) > 0.1, 'native first audio');
    const oneItem = await queue(account.epoch);
    assert(oneItem.tracks.map((item) => item.track.voiceId).join() === '501', 'row click is a single voice');
    await capture('playing');
    double(501);
    await wait(() => player()?.dataset.state === 'paused', 'same row pauses');
    const paused = await queue(account.epoch);
    assert(paused.revision === oneItem.revision && paused.current === oneItem.current, 'pause keeps queue identity');
    await capture('paused');

    pane().scrollTop = pane().scrollHeight;
    await wait(() => one('[role="alert"]', recent()) && size() === 18, 'continuation failure retains current rows');
    await capture('next_error');
    click(byText('重试', recent()));
    await wait(() => size() === 20 && row(520), 'cursor continuation deduplicated');
    assert(ids().filter((id) => id === '518').length === 1, 'overlap not duplicated');
    pane().scrollTop = pane().scrollHeight;
    await wait(() => row(520)?.getBoundingClientRect().bottom <= pane().getBoundingClientRect().bottom + 1,
      'last row is visible, not only mounted');
    await capture('next_page');

    const rememberedScroll = pane().scrollTop;
    click(one('.recent-radio', row(520)));
    await wait(() => pane()?.querySelector('h1').textContent === '播客详情' &&
      one('[data-voice-id="501"]', pane()), 'radio from scrolled page');
    click(byText('返回播客列表', pane()));
    await wait(() => row(520) && document.activeElement === one('.recent-radio', row(520)) &&
      Math.abs(pane().scrollTop - rememberedScroll) < 2, 'back restores the virtual window and scroll');
    await capture('scrolled_back');

    pane().scrollTop = 0;
    await wait(() => row(503), 'first rows restored after scroll');
    double(503);
    await wait(() => row(503)?.classList.contains('current') &&
      player()?.dataset.state === 'playing' && Number(player().dataset.position) > 0.1, 'missing track hydrated and native playback');
    const replacement = await queue(account.epoch);
    const voiceIds = replacement.tracks.map((item) => item.track.voiceId);
    assert(voiceIds.join() === Array.from({ length: 20 }, (_, i) => String(501 + i)).filter((id) => id !== '505').join(), 'double click replaces queue with eligible loaded rows in server order');
    await capture('queue_replaced');

    const preference = await window.__TAURI_INTERNALS__.invoke('queue_click_preferences', { epoch: account.epoch });
    await window.__TAURI_INTERNALS__.invoke('queue_click_preferences_command', {
      epoch: account.epoch, command: { kind: 'change', expected_revision: preference.value.revision, replace_queue: false },
    });
    double(504);
    await wait(() => row(504)?.classList.contains('current') &&
      player()?.dataset.state === 'playing', 'keep queue setting preserves entries');
    const retained = await queue(account.epoch);
    assert(retained.tracks.map((item) => item.entryId).join() ===
      replacement.tracks.map((item) => item.entryId).join() && retained.current !== replacement.current,
      'double click in keep mode retains native entry identities');
    await capture('queue_retained');

    click(byText('刷新', pane()));
    await wait(() => recent()?.textContent.includes('暂无最近更新的单集'), 'terminal empty refresh');
    assert((await queue(account.epoch)).revision === retained.revision, 'refresh does not change playback');
    await capture('empty');
    await emit('complete', { ok: true, stageCount: index, remoteWrites: 0, realMusicRequests: 0, queue: voiceIds });
  } catch (error) {
    await emit('failed', { ok: false, error: String(error), index, scroll: pane()?.scrollTop,
      focused: document.activeElement?.closest('[data-recent-voice]')?.dataset.recentVoice,
      html: pane()?.innerText.slice(0, 1200) });
  }
})();
