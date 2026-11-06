// Real My Podcasts DOM, product model/IPC, and a sealed native badge adapter.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const button = (label, root = document) => [...(root?.querySelectorAll('button') ?? [])]
    .find((node) => node.textContent.trim() === label || node.getAttribute('aria-label') === label);
  const pane = () => one('.podcast-page');
  const card = (id) => one(`.podcast-card[data-radio-id="${id}"]`, pane());
  const badge = (id) => one('[data-podcast-new-badge]', card(id));
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const emit = (stage, payload = {}) => invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...payload },
  });
  const assert = (value, message) => { if (!value) throw Error(message); };
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  async function wait(test, label) {
    for (let n = 0; n < 200; n++) {
      const result = await test();
      if (result) return result;
      await sleep(50);
    }
    throw Error(label);
  }
  function shown(node) {
    if (!node?.isConnected || node.disabled || node.closest('[inert]')) return false;
    const box = node.getBoundingClientRect();
    return box.width > 0 && box.height > 0 &&
      node.contains(document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2));
  }
  function opaque(node) {
    for (let current = node; current; current = current.parentElement) {
      const { opacity, visibility } = getComputedStyle(current);
      if (opacity !== '1' || visibility !== 'visible') return false;
    }
    return true;
  }
  function click(node) {
    assert(shown(node), 'ready visible control');
    node.focus({ preventScroll: true });
    node.click();
  }
  function solidPadding(selector, right = false) {
    const node = one(selector);
    const rect = node.getBoundingClientRect();
    const padding = Number.parseFloat(getComputedStyle(node)[right ? 'paddingRight' : 'paddingLeft']);
    const offset = Math.min(5, padding / 2);
    const x = right ? rect.right - offset : rect.left + offset, y = rect.top + rect.height / 2;
    assert(padding >= 4 && document.elementFromPoint(x, y) === node && opaque(node),
      'visible solid padding: ' + selector);
    for (let current = node; current; current = current.parentElement) {
      const style = getComputedStyle(current);
      assert(style.backgroundImage === 'none', 'solid sample only');
      const rgb = /^rgb\((\d+),\s*(\d+),\s*(\d+)\)$/.exec(style.backgroundColor);
      if (rgb) return { x, y, color: 'rgb(' + rgb.slice(1).join(', ') + ')' };
      assert(style.backgroundColor === 'transparent' ||
        /^rgba\(\d+,\s*\d+,\s*\d+,\s*0\)$/.test(style.backgroundColor), 'transparent ancestor only');
    }
    throw Error('no opaque padding color');
  }
  function badgeLayout(id) {
    const item = badge(id);
    if (!item) return null;
    const cover = one('.podcast-card-cover', card(id)).getBoundingClientRect();
    const badgeBox = item.getBoundingClientRect();
    assert(shown(item) && opaque(item) &&
      badgeBox.left >= cover.left && badgeBox.right <= cover.right &&
      badgeBox.top >= cover.top && badgeBox.bottom < cover.bottom &&
      cover.right - badgeBox.right <= 6 && badgeBox.top - cover.top <= 6,
      'badge stays in top-right of the actual cover: ' + id);
    return { id, text: item.textContent.trim(), label: item.getAttribute('aria-label') };
  }
  const requests = [], detailReads = [];
  let index = 0, last = 'startup';
  async function capture(label, target, captureBadges = true, extra = {}) {
    last = label;
    await wait(() => shown(target()) && opaque(target()), 'settled visible stage: ' + label);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const badges = captureBadges ? ['81', '82', '83'].map(badgeLayout).filter(Boolean) : [];
    assert(pane().scrollWidth <= pane().clientWidth + 1, 'podcast layout fits viewport');
    await emit(`ux_${String(++index).padStart(2, '0')}_${label}`, {
      ...extra, actual: document.visibilityState === 'visible', badges,
      detail: one('.podcast-summary h2', pane())?.textContent.trim() ?? null,
      clearOutcomes: requests.slice(), readCount: detailReads.length,
      screenX, screenY, ratio: devicePixelRatio,
      paintSamples: window.__CUMULUS_PROOF_INVOKE__
        ? target().matches('dialog[open]')
          ? [solidPadding('dialog[open]'), solidPadding('dialog[open]', true)]
          : ['.sidebar-caption', '.playerbar'].map((selector) => solidPadding(selector)) : [],
    });
    await wait(() => window.__podcastProgressStage === `ux_${String(index).padStart(2, '0')}_${label}`,
      'durable stage acknowledgement');
  }
  function detailReady(id) {
    return one('h1', pane())?.textContent.trim() === '播客详情' &&
      one(`[data-voice-id="${9000 + Number(id)}"]`, pane());
  }
  async function back() {
    click(await wait(() => {
      const node = button('返回播客列表', pane());
      return shown(node) && node;
    }, 'real Back button'));
    await wait(() => shown(card('81')) && shown(card('82')) && shown(card('83')),
      'returned to current subscription snapshot');
  }
  function refresh() {
    return button('刷新', pane());
  }
  async function checkRecommendations(epoch) {
    const shelf = () => one('[data-podcast-subscription-recommendations]', pane());
    const rec = (id) => one(`[data-recommended-radio-id="${id}"]`, shelf());
    const recOpen = (id) => one('.recommendation-open', rec(id));
    const currentMenu = () => [...document.querySelectorAll('[role="menu"]')]
      .find((node) => node.getAttribute('aria-label')?.startsWith('推荐播客菜单：') && shown(node));
    const menuGone = () => !one('[role="menu"][aria-label^="推荐播客菜单："]');
    const menuKeys = () => [...currentMenu().querySelectorAll('button[data-menu-key]')]
      .map((node) => node.dataset.menuKey);
    async function openMenu(id, keyboard = false) {
      const anchor = recOpen(id);
      anchor.focus({preventScroll: true});
      if (keyboard) {
        anchor.dispatchEvent(new KeyboardEvent('keydown',
          {key: 'F10', shiftKey: true, bubbles: true, cancelable: true}));
      } else {
        const box = anchor.getBoundingClientRect();
        anchor.dispatchEvent(new MouseEvent('contextmenu',
          {button: 2, clientX: box.left + 20, clientY: box.top + 30, bubbles: true, cancelable: true}));
      }
      return wait(() => shown(currentMenu()) && opaque(currentMenu()) && currentMenu(), 'recommendation menu');
    }
    const visibleIds = () => [...shelf().querySelectorAll('[data-recommended-radio-id]:not([inert])')]
      .map((card) => card.dataset.recommendedRadioId);
    await wait(shelf, 'recommendation shelf');
    assert(one('h2', shelf()).textContent.trim() === '为你推荐' &&
      Boolean(card('83').compareDocumentPosition(shelf()) & Node.DOCUMENT_POSITION_FOLLOWING),
      'shelf comes after subscribed cards');
    const all = [...shelf().querySelectorAll('[data-recommended-radio-id]')]
      .map((node) => node.dataset.recommendedRadioId);
    assert(JSON.stringify(all) === JSON.stringify(['81', '82', '83', '84', '85', '86']),
      'exact original order and six-slot limit');
    shelf().scrollIntoView({ block: 'nearest', behavior: 'instant' });
    await capture('recommended_first', () => rec('81'), false);
    const first = visibleIds();
    assert(first[0] === '81' && first.length < 6, 'first visible group');
    const next = () => button('下一组推荐播客', shelf());
    click(await wait(() => shown(next()) && next(), 'accessible next group'));
    await wait(() => shown(rec('86')) && rec('81').inert &&
      !one('.recommendation-rail', shelf()).getAnimations().length, 'settled last group');
    await capture('recommended_next', () => rec('86'), false);
    const lastGroup = visibleIds();
    assert(lastGroup.length === first.length && lastGroup.at(-1) === '86', 'last visible group');
    await openMenu('86', true);
    assert(JSON.stringify(menuKeys()) === JSON.stringify(['open','play','next','collect','share','copy']),
      'external unsubscribed card offers the official ordinary menu');
    await capture('recommendation_menu_collectible', currentMenu, false);
    click(button('收藏...', currentMenu()));
    const confirm = () => one('dialog[open][aria-label="收藏播客"]');
    await wait(() => shown(confirm()) && opaque(confirm()), 'explicit recommendation collect confirmation');
    await capture('recommendation_menu_collect_confirm', confirm, false);
    click(button('取消', confirm()));
    await wait(() => !confirm() && document.activeElement === recOpen('86'), 'cancel restores original card focus');
    const previous = () => button('上一组推荐播客', shelf());
    click(await wait(() => shown(previous()) && previous(), 'accessible previous group'));
    await wait(() => shown(rec('81')) && !rec('81').inert, 'first group restored');
    const overlay = () => one('.recommendation-play', rec('81'));
    const playingState = async (state) => {
      const player = await invoke('player_snapshot', { epoch });
      return player.state === state && player.track?.voiceId === '9081' &&
        player.origin?.kind === 'podcast' && player.origin.id === '81' &&
        player.mode === 'sequential' && player.playId && player;
    };
    overlay().focus({ preventScroll: true });
    await wait(() => shown(overlay()) && opaque(overlay()), 'visible overlay on focus');
    assert(overlay().getAttribute('aria-label') === '播放 合成推荐 81', 'play label before selection');
    click(overlay());
    const started = await wait(async () => {
      const player = await playingState('playing');
      return player?.position > .2 && player;
    }, 'real native recommendation playback');
    assert(shelf() && !detailReady('81'), 'overlay stays on library');
    await capture('recommended_playing', overlay, false, { player: started });
    for (const [state, label] of [['paused', '继续播放'], ['playing', '暂停']]) {
      const previous = state === 'paused' ? '暂停' : '继续播放';
      await wait(() => overlay().getAttribute('aria-label') === previous + ' 合成推荐 81' &&
        shown(overlay()), 'ready current-source overlay: ' + previous);
      click(overlay());
      const result = await wait(() => playingState(state), 'bound overlay ' + state);
      await wait(() => overlay().getAttribute('aria-label') === label + ' 合成推荐 81',
        'updated overlay state: ' + label);
      assert(result.playId === started.playId && result.entryId === started.entryId &&
        result.queueRevision === started.queueRevision && result.position >= started.position - .05,
        'same original play, entry, queue and position');
      await capture('recommended_' + (state === 'paused' ? 'paused' : 'resumed'),
        overlay, false, { player: result });
    }
    click(recOpen('81'));
    await wait(() => detailReady('81') && detailReads.length === 1, 'recommended detail');
    assert(!shelf() && !requests.length, 'detail is normal and never clears subscription badge');
    await capture('recommended_detail', () => one('.podcast-summary h2', pane()));
    await back();
    assert(badge('81')?.textContent.trim() === '99+', 'original subscription badge survived');
    await wait(shelf, 'recommendations survive Back');
    shelf().scrollIntoView({ block: 'nearest', behavior: 'instant' });
    await capture('recommended_back', () => rec('81'), false);
    await openMenu('81');
    assert(JSON.stringify(menuKeys()) === JSON.stringify(['open','play','next','share','copy']),
      'already subscribed card omits collect and removal');
    await capture('recommendation_menu_subscribed', currentMenu, false);
    click(button('分享...', currentMenu()));
    const share = () => one('dialog[open].track-share-dialog');
    await wait(() => shown(share()) && opaque(share()), 'podcast public share dialog');
    assert(one('input[aria-label="播客分享链接"]', share()).value === 'https://music.163.com/djradio?id=81',
      'exact public podcast link with no attribution or credentials');
    await capture('recommendation_menu_share', share, false);
    click(one('button[aria-label="关闭分享"]', share()));
    await wait(() => !share() && document.activeElement === recOpen('81'), 'share restores original card focus');
    await openMenu('81');
    click(button('复制链接', currentMenu()));
    const copied = () => [...shelf().querySelectorAll('[role="status"]')]
      .find((node) => node.textContent.trim() === '链接已复制');
    await wait(() => copied() && menuGone(), 'native clipboard acknowledged the public link');
    await capture('recommendation_menu_copy', copied, false);

    click(overlay());
    const paused = await wait(() => playingState('paused'), 'pause before queue-only action');
    await openMenu('82');
    assert(!copied(), 'new card action clears previous clipboard feedback');
    click(button('下一首播放', currentMenu()));
    const queued = await wait(async () => {
      const result = await playingState('paused');
      return result?.queueLen === 2 && result;
    }, 'insert next podcast without transport change');
    assert(queued.playId === paused.playId && queued.entryId === paused.entryId &&
      queued.position === paused.position && queued.queueRevision !== paused.queueRevision,
      'queue-only action preserves selected native decoder and clock');
    await wait(menuGone, 'settled next action menu dismissal');
    await capture('recommendation_menu_next', () => recOpen('82'), false, { player: queued });
    await emit('complete', { ok: true, stageCount: index, remoteWrites: 0,
      realMusicRequests: 0, detailReads, first, lastGroup });
  }
  let core, clearOriginal, pageOriginal;
  try {
    const account = await wait(async () => {
      const value = await invoke('account_snapshot');
      return value.status === 'authenticated' && value;
    }, 'sealed authenticated account');
    const provides = one('#app').__vue_app__._context.provides;
    core = provides[Reflect.ownKeys(provides).find((key) =>
      typeof key === 'symbol' && key.description === 'core-client')];
    clearOriginal = core.podcastClearSubscribedBadge;
    pageOriginal = core.podcastPage;
    core.podcastClearSubscribedBadge = async (owner, receipt, id) => {
      assert(owner === account.epoch && /^[0-9a-f]{32}$/.test(receipt), 'real account-bound receipt');
      try {
        const result = await clearOriginal.call(core, owner, receipt, id);
        requests.push([id, 'acknowledged']);
        return result;
      } catch (error) {
        requests.push([id, error?.code ?? 'unexpected']);
        throw error;
      }
    };
    core.podcastPage = async (owner, query, fresh) => {
      const result = await pageOriginal.call(core, owner, query, fresh);
      if (query.scope.kind === 'radio') detailReads.push(query.scope.id);
      return result;
    };

    const libraryEntry = () => button('我的播客', one('.sidebar-navigation[aria-label="我的"]'));
    const readyEntry = await wait(() => {
      const node = libraryEntry();
      return node?.isConnected && !node.disabled && node;
    }, 'personal podcast sidebar entry enabled');
    readyEntry.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'instant' });
    click(await wait(() => {
      const node = libraryEntry();
      return shown(node) && node;
    }, 'real sidebar entry visible'));
    await wait(() => shown(card('81')) && shown(card('82')) && shown(card('83')),
      'signed subscription cards');
    if (window.__podcastRecommendationsProof) {
      await checkRecommendations(account.epoch);
      return;
    }
    assert(badge('81')?.textContent.trim() === '99+' &&
      badge('81').getAttribute('aria-label') === '108 集新节目' &&
      badge('82')?.textContent.trim() === '4' && !badge('83') &&
      !one('[data-podcast-recent-tab] [data-podcast-new-badge]', pane()) &&
      one('[data-podcast-recent-tab]', pane()).textContent.trim() === '最近更新单集',
      'positive per-card count, overflow, zero, and no recent-tab badge');
    await capture('initial_badges', () => badge('81'));

    const input = one('[aria-label="播客编号或链接"]', pane());
    input.value = '81';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    click(await wait(() => {
      const node = button('打开播客', pane());
      return shown(node) && node;
    }, 'ordinary detail input action'));
    await wait(() => detailReady('81') && detailReads.length === 1, 'ordinary detail rendered');
    assert(requests.length === 0, 'ordinary typed detail did not clear');
    await capture('ordinary_detail', () => one('.podcast-summary h2', pane()));
    click(await wait(() => shown(refresh()) && refresh(), 'enabled detail refresh'));
    await wait(() => detailReady('81') && detailReads.length === 2 && shown(refresh()),
      'ordinary detail refresh completed');
    assert(requests.length === 0, 'ordinary detail refresh did not clear');
    await capture('ordinary_refresh', refresh);
    await back();
    assert(badge('81')?.textContent.trim() === '99+', 'ordinary detail and refresh left badge');

    click(card('81'));
    await wait(() => detailReady('81') && requests.length === 1, 'explicit acknowledged card detail');
    assert(JSON.stringify(requests) === JSON.stringify([['81', 'acknowledged']]), 'real clear ACK');
    await capture('confirmed_detail', () => one('.podcast-summary h2', pane()));
    await back();
    await wait(() => !badge('81') && badge('82'), 'confirmed badge gone on Back');
    await capture('confirmed_back', () => badge('82'));

    click(card('82'));
    await wait(() => detailReady('82') && requests.length === 2, 'explicit lost reply still opened detail');
    assert(JSON.stringify(requests[1]) === JSON.stringify(['82', 'network']), 'actual lost clear reply');
    await capture('lost_detail', () => one('.podcast-summary h2', pane()));
    await back();
    assert(badge('82')?.textContent.trim() === '4' && !badge('81'), 'lost reply did not pretend to clear');
    await capture('lost_back', () => badge('82'));

    click(card('82'));
    await wait(() => detailReady('82') && requests.length === 3, 'second explicit detail still opens');
    assert(JSON.stringify(requests[2]) === JSON.stringify(['82', 'stale']), 'same ambiguous fact rejected natively');
    await back();
    assert(badge('82')?.textContent.trim() === '4' && !badge('81'), 'lost badge persists without resend');
    await capture('no_replay', () => badge('82'));
    await emit('complete', { ok: true, stageCount: index, remoteWrites: 0,
      realMusicRequests: 0, clearOutcomes: requests, detailReads });
  } catch (error) {
    await emit('failed', { ok: false, last, error: String(error), requests, detailReads });
  } finally {
    if (core && clearOriginal && pageOriginal) {
      core.podcastClearSubscribedBadge = clearOriginal;
      core.podcastPage = pageOriginal;
    }
  }
})();
