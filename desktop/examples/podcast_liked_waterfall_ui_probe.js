(async () => {
  const invoke = (...args) => window.__TAURI_INTERNALS__.invoke(...args);
  const one = (selector, root = document) => root?.querySelector(selector);
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])];
  const byText = (root, label) =>
    root && all('button', root).find((button) => button.textContent.trim().startsWith(label));
  const pane = () => one('.podcast-page');
  const liked = () => one('[data-podcast-liked]', pane());
  const count = () => Number(one('[data-liked-count]', pane())?.textContent.trim());
  const row = (id) => one(`[data-liked-voice-index][data-voice-id="${id}"]`, liked());
  const rows = () => all('[data-liked-voice-index][data-voice-id]', liked());
  const list = () => one('[role="list"][aria-label="点赞的单集列表"]', liked());
  const alert = () => one('[role="alert"]', liked());
  const retry = () => byText(alert(), '重试');
  const spinner = () => one('[role="status"] .spinner', liked());
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (ok, message) => {
    if (!ok) throw Error(message);
  };
  async function wait(predicate, label) {
    for (let i = 0; i < 250; i++) {
      if (predicate()) return;
      await sleep(40);
    }
    throw Error('Timeout: ' + label);
  }
  function capture(stage, data = {}) {
    return invoke('plugin:event|emit', {
      event: 'ux-proof',
      payload: { stage, actual: document.visibilityState === 'visible', ...data },
    });
  }
  const loaded = () => Number(rows()[0]?.getAttribute('aria-setsize'));
  const geometry = () => {
    const mounted = rows();
    assert(mounted.length > 0 && mounted.length < 30, 'mounted rows must be bounded');
    const ids = mounted.map((item) => item.dataset.voiceId);
    const indices = mounted.map((item) => Number(item.dataset.likedVoiceIndex));
    assert(new Set(ids).size === ids.length, 'mounted programmes must be unique');
    assert(indices.every((index, i) => Number(ids[i]) === 1001 + index &&
      Number(mounted[i].getAttribute('aria-posinset')) === index + 1), 'native order and positions');
    assert(mounted.every((item) => Number(item.getAttribute('aria-setsize')) === loaded()), 'same loaded prefix');
    const bounds = pane().getBoundingClientRect();
    const visibleIds = mounted.filter((item) => {
      const rect = item.getBoundingClientRect();
      return rect.bottom > bounds.top + 2 && rect.top < bounds.bottom - 2;
    }).map((item) => item.dataset.voiceId);
    return { ids, indices, visibleIds, mounted: mounted.length, loaded: loaded(), count: count(),
      scrollTop: pane().scrollTop };
  };
  function bottom() {
    pane().scrollTop = pane().scrollHeight;
  }
  function showIndex(index) {
    const outer = pane();
    const listTop = list().getBoundingClientRect().top - outer.getBoundingClientRect().top + outer.scrollTop;
    const height = rows()[0].getBoundingClientRect().height;
    assert(height === 56 || height === 64, 'recognized row density');
    outer.scrollTop = listTop + height * index + 9;
  }
  let step = 'synthetic account and podcast shell';
  try {
    const library = () => byText(document, '我的播客');
    await wait(() => library() && !library().disabled, step);
    library().click();
    const tab = () => byText(one('.podcast-tabs', pane()), '点赞的单集');
    await wait(() => tab() && count() === 145 && !liked(), 'podcast cards and independent count');
    await capture('ux_01_library', { count: count() });

    step = 'exactly one first page';
    tab().click();
    await wait(() => loaded() === 100 && row('1001'), 'first 100, windowed at top');
    assert(!spinner() && !alert(), 'initial page is settled');
    await capture('ux_02_first_page', geometry());

    step = 'scrolling triggers one held continuation';
    bottom();
    await wait(() => row('1100') && spinner(), 'tail triggers native continuation');
    const held = geometry();
    assert(held.loaded === 100 && held.ids.includes('1100'), 'held reply keeps the loaded prefix');
    // Leave and reenter the tail while the request is held.
    showIndex(75);
    await wait(() => row('1080') && !row('1100'), 'leave virtual tail');
    bottom();
    await wait(() => row('1100') && spinner(), 'reach the tail again without a second request');
    await capture('ux_03_tail_held', geometry());

    step = 'failed continuation keeps prefix and can be retried';
    await capture('reject_held_liked_page');
    await wait(() => retry() && row('1100'), 'inline tail retry');
    assert(alert().textContent.includes('已保留当前列表'), 'tail failure wording');
    const failed = geometry();
    assert(failed.loaded === 100, 'tail failure keeps exactly 100 programmes');
    await capture('ux_04_tail_failed', { ...failed, retry: retry().textContent.trim() });
    retry().click();
    await wait(() => loaded() === 137 && !alert(), 'second page extends the old prefix');
    showIndex(96);
    await wait(() => row('1100') && row('1101'), 'both sides of the overlapping boundary');
    const retried = geometry();
    assert(retried.ids.filter((id) => id === '1100').length === 1, 'deduplicated overlapping tail');
    await capture('ux_05_tail_retried', retried);

    step = 'comments from an appended row';
    showIndex(118);
    const episode = () => row('1122');
    const comments = () => one('[aria-label="单集评论"]', episode());
    await wait(() => comments() && !comments().disabled, 'comment control in second page');
    assert(one('[data-liked-voice-more]', episode()), 'appended row retains its actions');
    comments().focus({ preventScroll: true });
    const previous = geometry();
    const screenY = () => episode().getBoundingClientRect().top - pane().getBoundingClientRect().top;
    const previousY = screenY();
    assert(previousY >= 0 && previousY < pane().clientHeight, 'comment row is in the viewport');
    await capture('ux_06_before_comments', { ...previous, voice: '1122', screenY: previousY });
    comments().click();
    const header = () => one('.comment-resource-header[data-resource-kind="voice"][data-resource-id="1122"]');
    await wait(() => header()?.getAttribute('aria-busy') === 'false' &&
      one('[data-comment-resource-name]', header())?.textContent.trim() === '合成单集 1122' &&
      one('[data-comment-read-heading]'), 'standalone comments for exact programme');
    await capture('ux_07_comments', { voice: header().dataset.resourceId });

    step = 'Back restores the liked prefix and scroll position';
    const back = one('[aria-label="返回上一页"]');
    assert(back && !back.disabled, 'Back enabled on comments');
    back.click();
    await wait(() => loaded() === 137 && comments() &&
      Math.abs(pane().scrollTop - previous.scrollTop) <= 2 &&
      Math.abs(screenY() - previousY) <= 2, 'POP restores the same row at the same position');
    await capture('ux_08_back', { ...geometry(), voice: '1122', screenY: screenY() });

    step = 'restored prefix keeps the unspent second cursor';
    bottom();
    await wait(() => loaded() === 145 && row('1145'), 'third page only after new tail scroll');
    await capture('ux_09_final_page', geometry());
    await capture('complete', { ok: true });
  } catch (error) {
    await capture('failed', { step, message: String(error) });
  }
})();
