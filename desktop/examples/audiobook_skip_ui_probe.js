// Drive the production PodcastPage, modal and footer against native playback.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const findText = (text, root = document) =>
    [...root.querySelectorAll('button')].find((button) => button.textContent.trim() === text);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const assert = (condition, label) => { if (!condition) throw Error(label); };
  const visible = (node) => node && !node.matches(':disabled') &&
    node.getClientRects().length > 0 && getComputedStyle(node).visibility === 'visible';
  async function wait(check, label, timeout = 12000) {
    const deadline = performance.now() + timeout;
    do {
      const result = await check();
      if (result) return result;
      await sleep(45);
    } while (performance.now() < deadline);
    throw Error(label);
  }
  async function click(get, label) {
    const element = await wait(() => {
      const candidate = get();
      return visible(candidate) && candidate;
    }, label);
    element.scrollIntoView({ block: 'nearest' });
    element.focus({ preventScroll: true });
    element.click();
    return element;
  }
  const labelled = (text, root) => one('[aria-label="' + text + '"]', root);
  const dialog = () => one('dialog.audiobook-skip-dialog[open]');
  const confirm = () => one('button[type="submit"]', dialog());
  const startRail = () => labelled('跳过片头', dialog());
  const endRail = () => labelled('跳过片尾', dialog());
  const choice = (text) => {
    const node = dialog();
    if (!node) return undefined;
    const label = [...node.querySelectorAll('label')].find((item) => one('strong', item)?.textContent === text);
    return one('input[type="radio"]', label ?? null);
  };
  const select = (text) => click(() => choice(text), 'choose ' + text);
  const footer = () => one('.playerbar');
  const detail = '播单更多操作';
  const playerMenu = '当前声音更多操作';
  let epoch;
  let stageCount = 0;
  let checkpointPoll = 0;
  const player = () => invoke('player_snapshot', { epoch });
  const duration = window.__audiobookDuration;

  function snapshot(value) {
    return value && { state: value.state, position: value.position, voice: value.track?.voiceId,
      media: value.track?.id, playId: value.playId, queueRevision: value.queueRevision,
      duration: value.timeline?.total, timeline: value.timeline?.mode,
      available: value.audiobookSkipAvailable,
    };
  }
  async function capture(name, value, detailState = {}) {
    const stage = 'ux_' + String(++stageCount).padStart(2, '0') + '_' + name;
    window.__audiobookStage = '';
    await emit(stage, { actual: document.visibilityState === 'visible',
      player: snapshot(value), ...detailState });
    await wait(() => window.__audiobookStage === stage, 'native stage saved: ' + stage);
  }
  async function completedCheckpoint() {
    const nonce = ++checkpointPoll;
    await emit('checkpoint_poll', { nonce });
    const reply = await wait(() => window.__audiobookCheckpoint?.[0] === nonce &&
      window.__audiobookCheckpoint, 'synthetic checkpoint query');
    return reply[1];
  }
  async function openMenu(label, text) {
    const trigger = await click(() => labelled(label), label);
    const item = await wait(() => {
      const panel = document.getElementById(trigger.getAttribute('aria-controls') ?? '');
      const node = one('[data-menu-key="skip"]', panel);
      return visible(node) && node;
    }, 'skip menu action');
    assert(item.textContent.trim() === text, 'skip menu text is ' + item.textContent.trim());
    return item;
  }
  async function openDialog(label, text) {
    const item = await openMenu(label, text);
    await click(() => item, 'open audiobook skip dialog');
    await wait(() => dialog() && one('h2', dialog())?.textContent === '跳过片头片尾' &&
      visible(choice('智能开启')), 'dialog and independently read smart option');
  }
  function range(input, value) {
    assert(visible(input), 'range is editable');
    input.value = String(value);
    input.dispatchEvent(new Event('input', { bubbles: true }));
  }
  async function save() {
    await click(confirm, 'explicitly confirm settings');
    await wait(() => !dialog(), 'save closes the dialog');
  }
  async function beginVoice(id, minimum, maximum, previous) {
    const row = () => one('[data-voice-id="' + id + '"]');
    await click(() => one('button[aria-label^="播放 "]', row() ?? null), 'play exact episode ' + id);
    return wait(async () => {
      const p = await player();
      return p.track?.voiceId === id && p.track?.id === String(Number(id) + 1000) &&
        p.state === 'playing' && p.timeline?.mode === 'full' && p.timeline?.total === duration &&
        p.playId !== previous && p.position >= minimum && p.position < maximum && p;
    }, 'authorized full voice at expected start: ' + id + '/' + minimum);
  }
  async function pauseSeek(playing, seconds) {
    await click(() => labelled('暂停', footer()), 'pause the native decoder');
    const seek = await wait(async () => {
      const p = await player();
      const slider = labelled('播放进度', footer());
      return p.state === 'paused' && p.playId === playing.playId &&
        p.queueRevision === playing.queueRevision && !p.nativeSeeking &&
        visible(slider) && slider.isConnected && slider;
    }, 'paused native play and its rendered seek control settle');
    range(seek, seconds);
    seek.dispatchEvent(new Event('change', { bubbles: true }));
    return wait(async () => {
      const p = await player();
      return p.state === 'paused' && p.playId === playing.playId &&
        p.queueRevision === playing.queueRevision &&
        !p.nativeSeeking && Math.abs(p.position - seconds) < .12 && p;
    }, 'paused native seek settles');
  }
  try {
    await click(() => findText('我的播客'), 'my podcasts navigation');
    await click(() => findText('有声书'), 'open the actual audiobook collection');
    await click(() => one('[data-radio-id="8"]'), 'open actual synthetic audiobook card');
    await wait(() => one('[data-voice-id="501"]'), 'radio detail episodes');
    ({ epoch } = await invoke('account_snapshot'));
    await openDialog(detail, '跳过片头尾');
    assert(choice('不开启').checked && !startRail() && !endRail(), 'initial preference is Off');
    await select('手动设置');
    await wait(() => startRail() && endRail(), 'manual sliders appear');
    assert(confirm().disabled, 'manual submit requires editing a rail');
    range(startRail(), 35);
    range(endRail(), 65);
    await wait(() => startRail()?.getAttribute('aria-valuetext') === '35 秒' &&
      endRail()?.getAttribute('aria-valuetext') === '25 秒' && !confirm().disabled,
    'reverse tail rail shows its saved value');
    await capture('manual_draft', undefined, { start: startRail().value, endRail: endRail().value });
    await save();
    await capture('manual_saved');

    const first = await beginVoice('501', duration === 220 ? 35 : .15, duration === 220 ? 40 : 5);
    await capture(duration === 220 ? 'manual_intro' : 'short_intro_ignored', first);
    if (duration === 199) {
      const before = await pauseSeek(first, 177.4);
      await capture('short_paused_tail', before);
      await click(() => labelled('播放', footer()), 'resume short voice');
      const after = await wait(async () => {
        const p = await player();
        return p.playId === first.playId && p.track?.voiceId === '501' &&
          p.state === 'playing' && p.position > 178.5 && p;
      }, 'short native clock passes manual tail threshold without a skip');
      await capture('short_tail_ignored', after);
    } else {
      const detailEntry = await openMenu(detail, '跳过片头尾（已开启）');
      await capture('enabled_label', await player(), { label: detailEntry.textContent.trim() });
      detailEntry.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
      await wait(() => labelled(detail)?.getAttribute('aria-expanded') === 'false', 'close detail action menu');
      await openDialog(playerMenu, '跳过片头片尾');
      assert(choice('手动设置').checked && startRail().value === '35' &&
        endRail().value === '65' && confirm().disabled, 'footer reopens persisted manual values');
      await capture('manual_readback', await player(), { start: startRail().value, endRail: endRail().value });
      range(startRail(), 65);
      await save();
      const unchanged = await player();
      assert(unchanged.playId === first.playId && unchanged.position >= 35 && unchanged.position < 61,
        'editing the intro does not jump the currently playing voice');
      await capture('live_edit_no_jump', unchanged);

      const paused = await pauseSeek(first, 197.4);
      await sleep(450);
      await capture('paused_past_tail', paused);
      await click(() => labelled('播放', footer()), 'resume into tail skip');
      const second = await wait(async () => {
        const p = await player();
        return p.track?.voiceId === '502' && p.playId !== first.playId && p.state === 'playing' &&
          p.timeline?.total === duration && p.position >= 65 && p.position < 70 && p;
      }, 'native skip reaches the next episode at the new intro', 9000);
      await wait(completedCheckpoint, 'synthetic full-length completion for the skipped episode');
      await capture('tail_advanced', second);

      await openDialog(playerMenu, '跳过片头片尾');
      await select('智能开启');
      await save();
      await capture('smart_saved', await player());
      const smart = await beginVoice('501', 45, 50, second.playId);
      await capture('smart_intro', smart);

      await openDialog(detail, '跳过片头尾（已开启）');
      assert(choice('智能开启').checked, 'persisted Smart mode opens from the detail menu');
      await select('不开启');
      await save();
      await capture('off_saved', await player());
      const disabledLabel = await openMenu(detail, '跳过片头尾');
      disabledLabel.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
      await wait(() => labelled(detail)?.getAttribute('aria-expanded') === 'false', 'close disabled label menu');
      const unskipped = await beginVoice('502', .15, 5, smart.playId);
      await capture('off_intro', unskipped);

      await click(() => findText('返回播客列表'), 'return to podcast collection');
      await click(() => findText('播客'), 'switch to ordinary podcast collection');
      await click(() => one('[data-radio-id="9"]'), 'open ordinary podcast card');
      await wait(() => one('[data-voice-id="601"]'), 'ordinary episode loaded');
      assert(!labelled(detail), 'ordinary podcast detail has no audiobook command');
      const ordinary = await beginVoice('601', .15, 5, unskipped.playId);
      await wait(() => !labelled(playerMenu), 'ordinary playback has no audiobook command');
      await capture('ordinary', ordinary, { detailMenu: false, footerMenu: false });
    }
    await emit('complete', { ok: true, stageCount, realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', { ok: false, error: String(error), stack: error?.stack, stageCount,
      player: epoch ? snapshot(await player().catch(() => null)) : null,
      text: one('.podcast-page')?.innerText.slice(0, 600), modal: dialog()?.innerText,
    });
  }
})();
