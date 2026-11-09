(async () => {
  const one = (selector, root = document) => root.querySelector(selector);
  const all = (selector, root = document) => [...root.querySelectorAll(selector)];
  const button = (text, root = document) => all('button', root).find(
    (element) => element.textContent.trim() === text || element.getAttribute('aria-label') === text,
  );
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, args = {}) => invoke('plugin:event|emit', {
    event: 'local-favorite-proof', payload: { stage, ...args },
  });
  const dialog = () => one('dialog[data-local-batch-favorite-dialog][open]');
  const favorite = () => one('[data-local-batch-favorite]');
  const add = () => one('[data-local-batch-add]');
  const status = () => one('[data-local-batch-favorite-status]');
  const row = (id) => dialog() && one(`[data-playlist-id="${id}"]`, dialog());
  const state = (epoch) => invoke('player_snapshot', { epoch });
  const queue = (epoch) => invoke('player_queue', { epoch });
  const assertion = (value, label) => { if (!value) throw Error(label); };
  async function wait(check, label) {
    const end = performance.now() + 18000;
    while (performance.now() < end) {
      const value = await check();
      if (value) return value;
      await new Promise((done) => setTimeout(done, 75));
    }
    throw Error(label);
  }
  function click(element) {
    assertion(element?.isConnected && !element.disabled, 'missing live control');
    element.focus({ preventScroll: true });
    element.click();
  }
  async function stage(name, element, data = {}) {
    if (window.__favoriteCapture) {
      await wait(() => {
        const item = element();
        if (!item || one('.menu-enter-active,.menu-leave-active')) return false;
        const r = item.getBoundingClientRect();
        return r.width > 0 && r.height > 0 &&
          item.contains(document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2));
      }, 'stage element visible: ' + name);
      await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));
    }
    await emit(name, data);
    if (window.__favoriteCapture)
      await wait(() => window.__favoriteCaptureAck === name, 'capture acknowledgement: ' + name);
  }
  try {
    const { epoch } = await wait(async () => {
      const view = await invoke('account_snapshot');
      return view.status === 'authenticated' && view;
    }, 'sealed authenticated account');
    click(await wait(() => button('本地音乐'), 'local library navigation'));
    await wait(() => one('.local-page')?.dataset.localLoading === 'false', 'native library ready');
    const filterLocal = async (value, count) => {
      const input = await wait(() => one('[aria-label="搜索本地音乐"]'), 'local search');
      input.value = value;
      input.dispatchEvent(new Event('input', { bubbles: true }));
      input.form.requestSubmit();
      await wait(() => one('.local-page')?.dataset.localLoading === 'false' &&
        all('.local-tracks tbody tr').length === count, 'local filter ' + value);
    };
    await filterLocal('Unmatched', 1);
    click(await wait(() => one('[aria-label="播放本地歌曲 Unmatched"]:not(:disabled)'), 'unrelated local playback'));
    await wait(async () => {
      const view = await state(epoch);
      return view.state === 'playing' && view.track?.name === 'Unmatched' && view.position > .2;
    }, 'actual native clock');
    const stopping = await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    assertion(stopping.state === 'paused' && stopping.playId, 'native decoder paused');
    await invoke('player_action', { epoch, action: { kind: 'seek', seconds: 3 } });
    const paused = await wait(async () => {
      const view = await state(epoch);
      return view.state === 'paused' && view.playId === stopping.playId &&
        Math.abs(view.position - 3) < .02 && view;
    }, 'native paused clock is held at the exact seek');
    const singleton = await queue(epoch);
    assertion(singleton.tracks.length === 1 && singleton.current === paused.entryId,
      'only the unrelated paused track is in the original native queue');
    await filterLocal('', 3);

    click(await wait(() => button('更多本地音乐操作'), 'library batch menu'));
    click(await wait(() => button('批量操作', one('.action-menu') ?? document), 'batch entry'));
    const check = (title) => one(`[data-local-batch] input[aria-label="选择 ${title}"]`);
    click(await wait(() => check('Batch A'), 'the later displayed row is selectable first'));
    click(await wait(() => check('Batch B'), 'select the first displayed row second'));
    const toolbar = all('.local-batch-actions button').map((element) => element.textContent.trim());
    assertion(JSON.stringify(toolbar) === JSON.stringify(['播放', '添加到播放列表', '收藏', '从列表中删除']),
      'official toolbar order and separate actions are visible');
    click(add());
    await wait(() => one('[data-local-batch-notice]')?.textContent.includes('已添加到播放列表'),
      'Add returned an accepted native transport update');
    const before = await queue(epoch);
    const afterAdd = await state(epoch);
    const nativeNames = before.tracks.map((item) => item.track.name);
    assertion(nativeNames.join(',') === 'Unmatched,Batch B,Batch A' &&
      before.current === singleton.current && before.revision !== singleton.revision &&
      afterAdd.state === 'paused' && afterAdd.playId === paused.playId &&
      afterAdd.entryId === paused.entryId && afterAdd.position === paused.position && !dialog(),
      'signed-in Add only updates the native queue, preserving decoder, position and focus scope');
    await stage('ux_00_local_favorite_add', add, { nativeNames, playId: afterAdd.playId });
    click(favorite());
    await wait(() => row('81'), 'owned ordinary playlist in actual picker');
    assertion(!row('77') && !row('98') && !button('创建新歌单', dialog()),
      'foreign, system liked and create actions excluded');
    assertion(add().disabled && favorite().disabled && one('[data-local-batch-play]').disabled,
      'the live picker locks other toolbar actions');
    await stage('ux_01_local_favorite_picker', () => row('81'));
    click(button('关闭歌单操作', dialog()));
    await wait(() => !dialog() && document.activeElement === favorite(), 'cancel returns focus');

    click(favorite());
    click(await wait(() => row('81'), 'explicit selection of the owned playlist'));
    const recover = () => status() && button('核对结果', status());
    await wait(() => recover() && document.activeElement === recover(),
      'lost response focuses the explicit read-only recovery');
    assertion(!one('.collect-working', dialog()), 'pending status is not still loading');
    assertion(row('81').getAttribute('aria-disabled') === 'true', 'the same row cannot send again');
    await stage('ux_02_local_favorite_pending', recover);
    await emit('reveal_readback');
    await wait(() => window.__favoriteRevealReady, 'native readback is observable');
    click(recover());
    const notice = () => one('[data-local-batch-favorite-notice]');
    await wait(() => !dialog() && notice()?.textContent.includes('已收藏到歌单'), 'readback confirms');
    await stage('ux_03_local_favorite_confirmed', favorite, { verified: true });

    click(favorite());
    click(await wait(() => row('81'), 'separate user action chooses the same playlist'));
    await wait(() => status()?.textContent.includes('歌单内歌曲重复'), 'official duplicate status');
    await wait(() => document.activeElement === button('关闭歌单操作', dialog()),
      'definite failure returns keyboard focus to close');
    assertion(!recover() && !one('.collect-working', dialog()), 'definite rejection is not ambiguous or loading');
    assertion(one('.collect-count', row('81'))?.textContent === '2首', 'reopened picker reads the observed song count');
    await stage('ux_04_local_favorite_duplicate', status);

    const after = await state(epoch);
    const final = await queue(epoch);
    assertion(after.state === 'paused' && after.playId === paused.playId &&
      after.entryId === paused.entryId && after.position === paused.position &&
      JSON.stringify(before) === JSON.stringify(final), 'no local transport or queue effect');
    await emit('complete', { ok: true, playId: after.playId, paused: after.state, position: after.position });
  } catch (error) {
    await emit('failed', { message: String(error?.message ?? error), stack: error?.stack, body: document.body.innerText.slice(0, 1200) });
  }
})();
