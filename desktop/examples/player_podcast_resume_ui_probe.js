// Real WebView and IPC with a network-sealed native PC transport.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const visible = (node) => !!node?.getClientRects().length && !node.closest('[inert]');
  const label = (text, root = document) =>
    [...(root?.querySelectorAll('button') ?? [])].find(
      (button) => (button.getAttribute('aria-label') ?? button.textContent.trim()) === text,
    );
  const trigger = () => label('当前声音更多操作', one('.playerbar'));
  const item = (enabled) =>
    label(`当前播客断点续播 (${enabled ? '开' : '关'})`, one('[role="menu"]'));
  const currentItem = () => item(true) ?? item(false);
  const dialog = () => one('dialog[data-player-podcast-resume][open]');
  const radios = () => [...(dialog()?.querySelectorAll('input[type="radio"]') ?? [])];
  const assert = (ok, why) => { if (!ok) throw Error(why); };
  const sleep = (ms) => new Promise((done) => setTimeout(done, ms));
  const frame = () => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));
  const same = (left, right) => JSON.stringify(left) === JSON.stringify(right);
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  let epoch, baseline, cohort = null, staleRejected = false, nonce = 0, count = 0, last = 'startup';

  async function wait(check, description) {
    last = description;
    const until = performance.now() + 15000;
    while (performance.now() < until) {
      const value = await check();
      if (value) return value;
      await sleep(50);
    }
    throw Error(description);
  }
  async function click(find, description) {
    const node = await wait(() => {
      const node = find();
      return visible(node) && !node.disabled && node.getAttribute('aria-disabled') !== 'true' && node;
    }, description);
    node.focus({ preventScroll: true });
    node.click();
    return node;
  }
  async function playback() {
    const [p, q] = await Promise.all([
      invoke('player_snapshot', { epoch }),
      invoke('player_queue', { epoch }),
    ]);
    return {
      state: p.state, voice: p.track?.voiceId, playId: p.playId, entryId: p.entryId,
      position: p.position, seeking: p.nativeSeeking, error: p.error, radio: p.voiceDetail?.radio?.id,
      queue: {
        revision: q.revision, current: q.current,
        entries: q.tracks.map((row) => [row.entryId, row.track.voiceId, row.track.id]),
      },
    };
  }
  function dom() {
    const active = document.activeElement;
    const [on, off] = radios();
    const menu = currentItem();
    return {
      trigger: visible(trigger()),
      menu: visible(menu) ? menu.textContent.trim() : null,
      dialog: visible(dialog()),
      selected: on?.checked ? 'on' : off?.checked ? 'off' : null,
      focus: active === on ? 'on' : active === off ? 'off' : active === trigger() ? 'trigger' :
        active === one('.transport-controls') ? 'transport' : active === menu ? 'menu' :
        active === label('确认设置结果', dialog()) ? 'readback' : 'other',
      notice: one('[role="status"]', dialog())?.textContent.trim() ?? null,
      readback: visible(label('确认设置结果', dialog())),
      staleRejected, cohort,
    };
  }
  async function capture(name) {
    // Wait for both layout and compositor so native screenshot receipt matches the DOM.
    await wait(() => [dialog(), one('[role="menu"]')]
      .filter(visible)
      .every((element) => getComputedStyle(element).opacity === '1' &&
        !element.getAnimations().some((animation) => animation.pending || animation.playState === 'running')),
      'menu and modal compositor settle');
    await frame();
    await sleep(100);
    const current = await playback();
    assert(same(current, baseline), `same paused native state at ${name}: ${JSON.stringify(current)}`);
    const stage = `ux_${String(++count).padStart(2, '0')}_${name}`;
    window.__podcastProgressStage = '';
    await emit(stage, { actual: document.visibilityState === 'visible', playback: current, dom: dom() });
    await wait(() => window.__podcastProgressStage === stage, 'host captured stage');
  }
  async function refresh(expected) {
    const request = ++nonce;
    await emit('player_resume_refresh', { nonce: request });
    const reply = await wait(
      () => window.__playerResumeCohort?.[0] === request && window.__playerResumeCohort[1],
      'native signed presentation refresh',
    );
    assert(reply.epoch === epoch && (reply.podcastResume === true) === (expected === 't1'),
      'expected account and observed full PC cohort');
    cohort = expected;
  }
  async function showMenu(enabled) {
    const opener = await wait(() => visible(trigger()) && trigger(), 'current voice More trigger');
    opener.focus({ preventScroll: true });
    opener.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    await wait(() => visible(item(enabled)) && document.activeElement === item(enabled),
      `keyboard focuses observed resume item (${enabled ? 'on' : 'off'})`);
  }
  async function openDialog(enabled) {
    await click(() => item(enabled), 'choose the current radio setting');
    await wait(() => {
      const choice = radios()[enabled ? 0 : 1];
      return visible(dialog()) && choice?.checked && document.activeElement === choice;
    }, 'dialog focuses the observed radio choice');
    const text = dialog().textContent.replace(/\s+/g, ' ');
    for (const words of ['断点续播', '开启后仅对本播客的所有声音生效', '从上一次停止的地方继续播放',
      '每次从头开始播放']) {
      assert(text.includes(words), `official wording: ${words}`);
    }
  }

  try {
    epoch = await wait(async () => {
      const account = await invoke('account_snapshot');
      return account.status === 'authenticated' && account.epoch !== '0' && account.epoch;
    }, 'restore signed synthetic profile');
    await click(() => one('input[aria-label="搜索音乐"]'), 'open search discovery');
    await click(() => label('播客榜：更多'), 'open classic podcast page');
    await click(() => label('播放随心听FM', one('[data-podcast-scene-fm]')), 'play sealed Scene FM program');
    await wait(async () => {
      const p = await playback();
      return p.state === 'playing' && p.voice === '601' && p.position > .2;
    }, 'real decoder advances');
    await click(() => label('暂停', one('.transport-controls')), 'pause the native decoder');
    let previous;
    baseline = await wait(async () => {
      const p = await playback();
      if (p.state !== 'paused' || p.voice !== '601' || p.radio !== '80' || p.seeking ||
          p.error || !Number.isFinite(p.position)) return false;
      const stable = same(previous, p);
      previous = p;
      return stable && p;
    }, 'settled paused clock and committed episode/radio');
    assert(!visible(trigger()), 'unresolved cohort has no resume trigger');
    await capture('before_cohort');

    await refresh('t1');
    await showMenu(true);
    await openDialog(true);
    await capture('opening');

    await click(() => radios()[1], 'change only the dialog draft');
    await click(() => label('关闭断点续播设置', dialog()), 'close without changing the radio');
    await wait(() => !dialog() && document.activeElement === trigger(), 'focus returns to More');
    await capture('cancelled');

    await showMenu(true);
    await openDialog(true);
    await click(() => radios()[1], 'select Off');
    await click(() => label('确定', dialog()), 'confirm Off');
    await wait(() => !dialog() && document.activeElement === trigger(), 'confirmed dialog returns focus');
    await showMenu(false);
    await capture('disabled');

    await openDialog(false);
    await click(() => radios()[0], 'select On');
    const confirm = await click(() => label('确定', dialog()), 'confirm On with a lost acknowledgment');
    assert(confirm.getAttribute('aria-disabled') === 'true' && document.activeElement === confirm,
      'sending preserves focus and suppresses another submit');
    confirm.click(); // This second same-frame submit must not send any request.
    await wait(() => visible(label('确认设置结果', dialog())) &&
      label('确认设置结果', dialog()).getAttribute('aria-disabled') === 'false' &&
      document.activeElement === label('确认设置结果', dialog()) &&
      one('[role="status"]', dialog())?.textContent.includes('未能确认'), 'uncertain dialog focuses only readback');
    await capture('lost_ack');
    await click(() => label('确认设置结果', dialog()), 'read back, never replay');
    await wait(() => !dialog() && document.activeElement === trigger(), 'readback closes and restores focus');
    await showMenu(true);
    await capture('reconciled');

    await openDialog(true);
    await refresh('c');
    await wait(() => !dialog() && !visible(trigger()) &&
      document.activeElement === one('.transport-controls'), 'retired dialog focuses the stable playback group');
    try {
      await invoke('player_podcast_resume', {
        epoch, playId: baseline.playId, entryId: baseline.entryId, action: { kind: 'change', enabled: false },
      });
    } catch (error) {
      staleRejected = error?.code === 'stale';
      if (!staleRejected) throw error;
    }
    assert(staleRejected, 'a stale native menu target must reject without dispatch');
    await capture('retired');
    await emit('complete', { ok: true, stageCount: count, realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', { message: String(error?.message ?? error), stack: error?.stack, step: last,
      dom: dom(), playback: epoch ? await playback().catch(() => null) : null });
  }
})();
