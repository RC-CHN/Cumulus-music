// Production WebView and native clipboard, using the sealed search/audio fixture.
window.__searchVoiceLinksRun = async ({ one, wait, click, key, fresh, choose, capture, emit, fixture, assert, theme }) => {
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const menu = () => one('.action-menu[role="menu"]:not([inert])');
  const menuItem = name => menu()?.querySelector(`[data-menu-key="${name}"]`);
  const dialog = () => one('dialog.track-share-dialog[open]:not([inert])');
  const copyButton = () => dialog()?.querySelector('button.primary');
  const status = () => one('[data-search-voice-link-status]');
  const publicLink = id => `https://music.163.com/dj?id=${id}`;
  async function clipboard(expected) {
    window.__searchClipboardAck = null;
    await emit('search-clipboard-check', { expected });
    await wait(() => window.__searchClipboardAck !== null, 'native clipboard acknowledgement');
    assert(window.__searchClipboardAck, 'native clipboard holds exactly ' + expected);
    return expected;
  }
  async function openMenu(scope, id, keyboard = false) {
    const trigger = one(`${scope} [data-result-key="voice:${id}"] .menu-trigger`);
    assert(trigger, 'episode More for ' + id);
    trigger.scrollIntoView({ block: 'center' });
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    if (keyboard) {
      trigger.focus({ preventScroll: true });
      trigger.dispatchEvent(new KeyboardEvent('keydown', {
        key: 'F10', shiftKey: true, bubbles: true, cancelable: true,
      }));
    } else click(trigger);
    await wait(() => menuItem('play') && document.activeElement === menuItem('play'), 'program menu ' + id);
    return trigger;
  }
  await wait(() => one('.track-row'), 'daily ready');
  await theme(1120, 760, 'light');
  await fixture('voice_next');
  const { epoch } = await invoke('account_snapshot');
  const snapshot = () => invoke('player_snapshot', { epoch });
  const queue = () => invoke('player_queue', { epoch });
  await invoke('play_selection', {
    epoch, selection: { kind: 'songs', ids: ['1', '2'], index: 0, quality: 'standard' },
  });
  const until = async (predicate, label) => {
    for (let i = 0; i < 120; i++) {
      const value = await snapshot();
      if (predicate(value)) return value;
      await new Promise(resolve => setTimeout(resolve, 65));
    }
    throw Error(label);
  };
  await until(value => value.track?.id === '1' && value.state === 'playing' && value.position > .15,
    'real null decoder starts catalog audio');
  await invoke('player_action', { epoch, action: { kind: 'pause', paused: true } });
  const baseline = await until(value => value.state === 'paused', 'catalog decoder is paused');
  const queueBaseline = await queue();
  async function observe(label, extra = {}) {
    const value = await snapshot(), queued = await queue();
    const entries = view => view.tracks.map(row => row.entryId);
    assert(value.state === 'paused' && value.track?.id === '1' &&
      value.playId === baseline.playId && value.entryId === baseline.entryId &&
      value.position === baseline.position && queued.revision === queueBaseline.revision &&
      entries(queued).join(',') === entries(queueBaseline).join(','),
    'program link does not change the decoder, paused clock or exact queue');
    await capture(label, {
      ...extra,
      playback: {
        playId: value.playId, entryId: value.entryId, position: value.position,
        queueRevision: queued.revision, entryIds: entries(queued),
      },
    });
  }
  async function menuPaint() {
    const choice = menuItem('play');
    const bounds = choice.getBoundingClientRect();
    const x = bounds.right - 12, y = bounds.top + bounds.height / 2;
    window.__searchPointerAck = false;
    await emit('search-pointer', { x, y });
    await wait(() => window.__searchPointerAck && choice.matches(':hover') &&
      choice.contains(document.elementFromPoint(x, y)) &&
      /^rgb\(\d+, \d+, \d+\)$/.test(getComputedStyle(choice).backgroundColor),
    'the actual menu is under the native pointer with opaque hover paint');
    const color = getComputedStyle(choice).backgroundColor;
    return { screenX, screenY, ratio: devicePixelRatio, paintSamples: [{ x, y, color }] };
  }

  await fresh('合成声音公开链接');
  const best = await openMenu('.search-best-section', 410);
  assert(!menuItem('copy') && !menuItem('share'), 'best match has no unverified program links');
  await observe('voice_link_best_match', await menuPaint());
  key(document.activeElement, 'Escape');
  await wait(() => !menu() && document.activeElement === best, 'best match Escape restores focus');

  const comprehensive = await openMenu('[data-result-section^="search_block_voice:"]', 401);
  assert(menuItem('share')?.textContent.trim() === '分享…' &&
    menuItem('copy')?.textContent.trim() === '复制链接', 'ordinary comprehensive menu has both actions');
  await observe('voice_link_comprehensive_menu', await menuPaint());
  click(menuItem('copy'));
  await wait(() => status()?.textContent.trim() === '链接已复制' &&
    document.activeElement === comprehensive && !dialog(), 'direct copy confirms and restores focus');
  await observe('voice_link_comprehensive_copy', { clipboard: await clipboard(publicLink(401)) });

  await choose('voices');
  assert(!status(), 'old category copy notice is retired');
  const classified = await openMenu('.search-voice-table', 402, true);
  click(menuItem('share'));
  await wait(() => dialog()?.querySelector('h2')?.textContent === '分享声音', 'classified program share dialog');
  const link = dialog().querySelector('[aria-label="声音分享链接"]');
  assert(link?.readOnly && link.value === publicLink(402) &&
    dialog().querySelector('strong')?.textContent.includes('合成声音 2') && !status(),
    'share shows the exact episode identity and link without copying');
  await observe('voice_link_classified_share', {
    link: link.value, clipboard: await clipboard(publicLink(401)),
  });
  click(copyButton());
  await wait(() => dialog()?.querySelector('[role="status"]')?.textContent.trim() === '链接已复制',
    'modal copy confirmed');
  await observe('voice_link_classified_copy', { clipboard: await clipboard(publicLink(402)) });
  click(dialog().querySelector('[aria-label="关闭分享"]'));
  await wait(() => !one('dialog.track-share-dialog[open]') && document.activeElement === classified,
    'closing share restores the exact classified More control');

  await openMenu('.search-voice-table', 401);
  click(menuItem('share'));
  await wait(() => dialog()?.querySelector('[aria-label="声音分享链接"]')?.value === publicLink(401),
    'old dialog captures a different link');
  const stale = copyButton();
  // Submit a new visit without sending any production share action. The old
  // DOM reference is deliberately retained across the real route transition.
  await fresh('另一次合成搜索');
  await wait(() => !one('dialog.track-share-dialog[open]') && !stale.isConnected, 'old share retires');
  stale.click();
  await observe('voice_link_retired', { clipboard: await clipboard(publicLink(402)) });
};
