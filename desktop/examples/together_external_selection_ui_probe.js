// Actual Tauri production Vue and IPC, sealed native API/IM/audio fixtures.
(async () => {
  if (window.__externalRoomProbeStarted) return;
  window.__externalRoomProbeStarted = true;
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const report = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'shared-queue-proof', payload: { stage, ...data },
  });
  const one = selector => document.querySelector(selector);
  const button = (name, parent = document) => [...parent.querySelectorAll('button')]
    .find(element => element.textContent.trim() === name && !element.disabled);
  const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
  async function wait(get, why) {
    const until = performance.now() + 16000;
    while (performance.now() < until) {
      const value = await get();
      if (value) return value;
      await delay(60);
    }
    throw Error(why);
  }
  function check(value, label) {
    if (!value) throw Error(label);
  }
  const same = (one, two) => JSON.stringify(one) === JSON.stringify(two);
  const dialog = () => one('dialog[open][data-together-external-selection]');
  let step = 'account';
  try {
    const { epoch } = await wait(async () => {
      const account = await invoke('account_snapshot');
      return account.status === 'authenticated' && account;
    }, 'fixture account');
    const player = () => invoke('player_snapshot', { epoch });
    const room = async () => (await invoke('together_view', { epoch, refresh: false })).value;
    const queue = async () => (await invoke('player_queue', { epoch })).tracks.map(row => row.track.id);
    const select = (ids, index = 0) => invoke('play_selection', {
      epoch, selection: { kind: 'songs', ids, index, quality: 'standard' },
    });
    await select(['91', '92', '93']);
    (await wait(() => button('一起听'), 'room navigation')).click();
    (await wait(() => button('创建房间'), 'room create control')).click();
    await wait(async () => {
      const view = await room();
      return view.attached && view.targetId === '91' && view.pending.length === 0;
    }, 'published incumbent');
    await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    const paused = await wait(async () => {
      const view = await player();
      return view.state === 'paused' && view;
    }, 'native incumbent paused');
    await wait(async () => !(await room()).playing, 'room pause published');

    step = 'partial visible queue, unchanged transport';
    const partial = await select(['710', '714', '713', '715']);
    check(partial.playId === paused.playId && partial.position === paused.position &&
      partial.state === 'paused' && same(await queue(), ['714', '715']), 'ordered visible result');
    await wait(() => one('.together-filtered-notice')?.textContent.includes('部分歌曲不支持一起听'),
      'official partial feedback rendered');
    await report('external_partial', { kept: await queue(), playId: partial.playId, position: partial.position });

    step = 'all unsupported, dismiss and confirm';
    for (const confirm of [false, true]) {
      let error;
      try { await select(['710', '712', '713', '711']); } catch (value) { error = value; }
      check(error?.code === 'together_selection_unsupported', 'typed native blocked selection');
      const opened = await wait(dialog, 'global leave confirmation appears');
      check(opened.textContent.includes('该资源暂不支持一起听') &&
        (await player()).queueRevision === partial.queueRevision, 'nothing changed before consent');
      const action = button(confirm ? '结束一起听' : '取消', opened);
      check(action && (confirm || document.activeElement === action), 'dismiss is initially focused');
      action.click();
      await wait(() => !dialog(), 'dialog closed');
      const currentRoom = await room();
      check(!!currentRoom.room !== confirm && !currentRoom.externalSelection, 'room outcome matches choice');
      const incumbent = await player();
      check(incumbent.playId === paused.playId && incumbent.position === paused.position &&
        same(await queue(), ['714', '715']), 'blocked input never replays');
      await report(confirm ? 'external_ended' : 'external_dismissed', { roomOpen: !!currentRoom.room, state: incumbent.state });
    }
    await report('complete', { ok: true, fixture: 'external selection' });
  } catch (error) {
    await report('failed', { step, error: error?.message ?? String(error) });
  }
})();
