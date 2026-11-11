// Actual Together page, native leave state and sealed synthetic API/IM/audio.
(async () => {
  if (window.__togetherLeaveRecordStarted) return;
  window.__togetherLeaveRecordStarted = true;
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const one = (selector) => document.querySelector(selector);
  const dialog = () => one('dialog[data-together-leave-record][open]:not([inert])');
  const button = (text, root = document) => [...root.querySelectorAll('button')]
    .find((node) => node.textContent.trim() === text && !node.disabled);
  const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  function check(ok, label) {
    if (!ok) throw Error(label);
  }
  async function wait(read, label) {
    const until = performance.now() + 14000;
    while (performance.now() < until) {
      const value = await read();
      if (value) return value;
      await delay(60);
    }
    throw Error(label);
  }
  const report = (stage, extra = {}) => invoke('plugin:event|emit', {
    event: 'shared-queue-proof', payload: { stage, ...extra },
  });
  async function capture(stage, data) {
    await report(stage, { screenX, screenY, ratio: devicePixelRatio, ...data });
    if (window.__togetherExternalCaptureEnabled)
      await wait(() => window.__togetherExternalCaptured === stage, 'captured ' + stage);
  }
  const click = (node) => {
    check(node && !node.disabled, 'enabled visible control');
    node.focus({ preventScroll: true });
    node.click();
  };
  function modules(svg) {
    check(svg.getAttribute('viewBox') === '0 0 35 35', 'public QR geometry');
    const size = 33;
    const rows = Array.from({ length: size }, () => Array(size).fill('0'));
    const data = svg.querySelector('path')?.getAttribute('d') ?? '';
    const units = [...data.matchAll(/M(\d+) (\d+)h1v1h-1z/g)];
    check(units.length && units.map((unit) => unit[0]).join(' ') === data, 'fixed QR module paths');
    for (const [, column, row] of units) {
      const x = Number(column) - 1, y = Number(row) - 1;
      check(rows[y]?.[x] === '0', 'one clear quiet module and unique QR data');
      rows[y][x] = '1';
    }
    return rows.map((row) => row.join(''));
  }
  let step = 'account';
  let lastDirect;
  const directPlayback = {};
  function observeDirect(native, room, incumbent) {
    const error = room.syncError;
    const blocked = !error ? null : error.startsWith('共享队列') ? 'shared'
      : error.startsWith('远端播放') ? 'incoming' : error.startsWith('好友实时') ? 'connection' : 'other';
    lastDirect = {
      nativeState: native.state, nativePosition: native.position ?? null,
      roomRole: room.room?.role ?? null, sharedPhase: room.sharedPhase,
      connection: room.connection.state, attached: room.attached,
      sameRoom: room.room?.id === 'test-room', roomPlaying: room.playing,
      roomTarget: room.targetId === '91' ? '91' : null, roomProgressMs: room.progressMs ?? null,
      samePlay: incumbent ? native.playId === incumbent.playId && native.entryId === incumbent.entryId &&
        native.queueRevision === incumbent.queueRevision : null,
      pending: room.pending.map(({ action, phase, acknowledged }) => ({ action, phase, acknowledged })),
      blocked,
    };
    return lastDirect;
  }
  try {
    await wait(() => one('.account-button')?.textContent.includes('本地验证账号') &&
      !one('.account-button').disabled, 'ready synthetic account');
    const { epoch } = await invoke('account_snapshot');
    const view = async () => (await invoke('together_view', { epoch, refresh: false })).value;
    const playback = () => invoke('player_snapshot', { epoch });
    const queue = async () => (await invoke('player_queue', { epoch })).tracks.map((row) => row.track.id);
    const roomPage = () => one('.together-page');
    const leaveConfirm = () => one('.together-leave[role="alertdialog"]');
    const absent = async () => {
      const value = await view();
      return !value.room && !value.attached && !value.ending && !value.localRecovery &&
        !value.pending.some((operation) => operation.action === 'end') && value;
    };
    const noRecord = (snapshot) => check(!dialog() && !snapshot.leaveRecord, 'no record rendered or projected');
    await invoke('play_selection', {
      epoch, selection: { kind: 'songs', ids: ['91', '92', '93'], index: 0, quality: 'standard' },
    });
    await invoke('player_action', { epoch, action: { kind: 'speed', value: 1.25 } });
    await invoke('player_action', { epoch, action: { kind: 'mode', mode: 'sequential' } });
    click(await wait(() => button('一起听'), 'Together navigation'));

    async function create(caseName, peer) {
      step = caseName + '_create';
      await report('leave-config', { case: caseName });
      click(await wait(() => roomPage() && button('创建房间', roomPage()), 'visible room creation'));
      await wait(async () => {
        const value = await view();
        return value.room?.role === 'waiting' && value.attached &&
          value.connection.state === 'connected' && value.tracks.length === 3 && value.targetId;
      }, 'waiting room published');
      if (!peer) return;
      await report('inject-peer');
      await wait(async () => {
        const value = await view();
        return value.room?.role === 'together_owner' && value.sharedPhase === 'applied';
      }, 'synthetic peer and shared playback');
    }
    async function openLeave() {
      click(await wait(() => roomPage() && button('退出房间', roomPage()), 'visible leave control'));
      return wait(() => leaveConfirm() && button('确认退出', leaveConfirm()), 'explicit leave confirmation');
    }
    async function endAndWait() {
      click(await openLeave());
      return wait(absent, 'confirmed room absence and completed local cleanup');
    }
    async function verifyRecord(stage, extra = {}) {
      const opened = await wait(dialog, 'current confirmed leave record rendered');
      const receipt = (await view()).leaveRecord;
      check(receipt && receipt.thisListenCount === 3 && receipt.thisDuration === 35 &&
        receipt.totalListenCount === 27 && receipt.totalDuration === 3665 &&
        !Object.hasOwn(receipt, 'shareUrl') && !Object.hasOwn(receipt, 'distance'),
      'exact numeric-only native receipt');
      const close = opened.querySelector('[aria-label="关闭一起听记录"]');
      const svg = opened.querySelector('svg[data-together-record-qr]');
      const qr = modules(svg);
      const text = (selectors) => [...opened.querySelectorAll(selectors)]
        .map((node) => node.textContent.trim().replace(/\s+/g, ''));
      const avatars = [...opened.querySelectorAll('.record-avatars [role="img"]')]
        .map((node) => node.getAttribute('aria-label'));
      const box = await wait(() => {
        if (opened.matches('.modal-enter-active,.modal-enter-from,.modal-leave-active')) return;
        const rect = opened.getBoundingClientRect();
        if (opened.getAnimations().some((animation) => animation.pending || animation.playState === 'running'))
          return;
        const style = getComputedStyle(opened);
        if (style.opacity !== '1' || style.transform !== 'none') return;
        const color = style.backgroundColor;
        return /^rgb\(\d+, \d+, \d+\)$/.test(color) && { rect, color };
      }, 'settled native modal layout');
      check(document.activeElement === close && opened.querySelectorAll('button').length === 1,
        'only close is focused');
      check(!opened.querySelector('a[href]') && !opened.textContent.includes('private-record-link'),
        'no service-supplied share link is exposed');
      await capture(stage, {
        ...extra, receipt, labels: text('.record-stats dt'), values: text('.record-stats dd'),
        total: text('.record-total')[0], avatars, focused: close.getAttribute('aria-label'),
        rect: [box.rect.width, box.rect.height], layout: [opened.offsetWidth, opened.offsetHeight],
        overflow: [opened.scrollHeight - opened.clientHeight, opened.scrollWidth - opened.clientWidth],
        qrWidth: svg.getBoundingClientRect().width, qr, community: text('.record-footer strong')[0],
        paintSamples: [{ x: box.rect.left + 20, y: box.rect.top + 24, color: box.color }],
      });
      return { opened, close, receipt };
    }
    async function closedReceipt(stage, previous, gesture) {
      await wait(async () => !one('dialog[data-together-leave-record]') &&
        !(await view()).leaveRecord, 'record consumed once in the native view');
      const expected = stage === 'leave_dismissed'
        ? '.together-heading h1' : '.queue-panel [aria-label="关闭播放队列"]';
      await wait(() => document.activeElement === one(expected), 'record close restores focus');
      await capture(stage, {
        operationId: previous.operationId, gesture, modalOpen: false,
        activeOutsideModal: !document.activeElement?.closest('dialog[data-together-leave-record]'),
        activeTag: document.activeElement?.tagName ?? null,
        activeLabel: document.activeElement?.getAttribute('aria-label') ??
          document.activeElement?.textContent.trim().slice(0, 60) ?? null,
      });
    }

    await create('waiting', false);
    step = 'cancel_waiting_leave';
    await openLeave();
    click(button('继续收听', leaveConfirm()));
    await wait(() => !leaveConfirm(), 'cancel keeps the room open');
    const waiting = await view();
    check(waiting.room?.role === 'waiting', 'still waiting');
    noRecord(waiting);
    await capture('leave_cancelled', { roomOpen: true, modalOpen: false });

    step = 'waiting_leave';
    noRecord(await endAndWait());
    await capture('leave_waiting', { roomOpen: false, modalOpen: false });

    await create('malformed', true);
    step = 'malformed_leave';
    noRecord(await endAndWait());
    await capture('leave_malformed', { roomOpen: false, modalOpen: false });

    await create('lost', true);
    step = 'lost_leave';
    click(await openLeave());
    const pending = await wait(async () => {
      const value = await view();
      const operation = value.pending.find((entry) => entry.action === 'end');
      // Manual End records its own pending journal. The separate ending flag
      // describes an unsolicited room-end notice, not a lost manual reply.
      return operation?.phase === 'uncertain' && !operation.acknowledged && { value, operation };
    }, 'lost reply retained as unresolved end');
    noRecord(pending.value);
    await capture('leave_uncertain', { pending: true, pendingPhase: pending.operation.phase,
      acknowledged: pending.operation.acknowledged, modalOpen: false });
    click(await wait(() => button('核对状态', roomPage()), 'manual read-only reconciliation'));
    noRecord(await wait(absent, 'read-only reconciliation confirms absence'));
    await capture('leave_recovered', { roomOpen: false, modalOpen: false });

    await create('direct', true);
    step = 'direct_incumbent';
    const incumbent = await wait(async () => {
      const [native, room] = await Promise.all([playback(), view()]);
      const current = observeDirect(native, room);
      if (!current.blocked && current.attached && current.sameRoom && current.roomRole === 'together_owner' &&
          current.sharedPhase === 'applied' && current.connection === 'connected' &&
          !current.pending.length && current.roomPlaying && current.roomTarget === '91' &&
          native.playId && native.entryId && native.state === 'playing') {
        directPlayback.incumbent = current;
        return native;
      }
    }, 'playing incumbent in the current shared room');

    step = 'direct_pause';
    await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
    const awaitPausedRoom = (position) => wait(async () => {
      const [native, room] = await Promise.all([playback(), view()]);
      const current = observeDirect(native, room, incumbent);
      if (!current.blocked && current.attached && current.sameRoom && current.roomRole === 'together_owner' &&
          current.sharedPhase === 'applied' && current.connection === 'connected' &&
          !current.pending.length && !current.roomPlaying && current.roomTarget === '91' &&
          current.samePlay && native.state === 'paused' && !native.nativeSeeking &&
          (position == null || (native.position === position && current.roomProgressMs === position * 1000)))
        return native;
    }, position == null ? 'room pause published before seek' : 'paused room progress published at 12 seconds');
    await awaitPausedRoom(null);
    directPlayback.paused = lastDirect;

    step = 'direct_seek';
    await invoke('player_action', { epoch, action: { kind: 'seek', seconds: 12 } });
    const before = await awaitPausedRoom(12);
    directPlayback.sought = lastDirect;
    step = 'direct_record';
    await endAndWait();
    const direct = await verifyRecord('leave_direct', { directPlayback });
    const after = await playback();
    check(after.playId === before.playId && after.state === 'paused' && after.position === 12 &&
      (await queue()).join(',') === '91,92,93', 'record never changes incumbent native playback or queue');
    check(!direct.opened.dispatchEvent(new Event('cancel', { cancelable: true })),
      'native dialog cancel is consumed');
    await closedReceipt('leave_dismissed', direct.receipt, 'dialog_cancel');

    await create('clear', true);
    step = 'queue_clear_record';
    const queueButton = await wait(() => one('[aria-label="播放队列"]:not(:disabled)'), 'player queue entry');
    click(queueButton);
    const panel = await wait(() => one('.queue-panel') && button('清空', one('.queue-panel')) &&
      one('.queue-panel'), 'real queue panel');
    click(button('清空', panel));
    const confirmation = await wait(() => one('dialog.queue-clear-dialog[open] .queue-clear-together') &&
      one('dialog.queue-clear-dialog[open]'), 'queue clear warns of room end');
    click(await wait(() => button('确认清空', confirmation), 'confirmed clear control'));
    await wait(absent, 'queue clear confirmed and cleaned up');
    const cleared = await verifyRecord('leave_queue_clear');
    check(cleared.receipt.operationId !== direct.receipt.operationId && !(await queue()).length,
      'fresh polling receipt for actual queue clear');
    click(cleared.close);
    await closedReceipt('leave_queue_dismissed', cleared.receipt, 'close_button');
    await report('complete', { ok: true, epoch, stageCount: 9 });
  } catch (error) {
    await report('failed', { step, error: error?.message ?? String(error), lastDirect, directPlayback });
  }
})();
