// Real desktop IPC and WebView, with the fixture handling the upstream reads.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const emit = (stage, data = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
    event: 'ux-proof', payload: { stage, ...data },
  });
  const assert = (condition, message) => { if (!condition) throw new Error(message); };
  const wait = async (predicate, message) => {
    for (let index = 0; index < 200; index++) {
      const result = predicate();
      if (result) return result;
      await sleep(65);
    }
    throw new Error(message);
  };
  const button = (root, name) => [...root.querySelectorAll('button')].find(
    (node) => node.getAttribute('aria-label') === name || node.textContent.trim() === name,
  );
  const click = (node) => {
    assert(node && !node.disabled, 'missing enabled control');
    node.focus({ preventScroll: true });
    node.click();
  };
  const dialog = () => one('.effect-directory[open]');
  const text = () => dialog()?.textContent ?? '';
  const artworkLoaded = () => (one('.effect-directory-card img')?.naturalWidth ?? 0) > 0;
  const seenArtwork = [];
  const artwork = (value) => {
    if (typeof value !== 'string' || !value.includes('/cumulus-proof-artwork-')) return value;
    seenArtwork.push(value);
    const color = value.includes('-dark.png') ? '#274a66' : '#855748';
    return 'data:image/svg+xml,' + encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" width="124" height="110"><rect width="124" height="110" fill="${color}"/></svg>`);
  };
  const imageSource = Object.getOwnPropertyDescriptor(HTMLImageElement.prototype, 'src');
  Object.defineProperty(HTMLImageElement.prototype, 'src', {
    ...imageSource, set(value) { imageSource.set.call(this, artwork(value)); },
  });
  const attribute = Element.prototype.setAttribute;
  Element.prototype.setAttribute = function (name, value) {
    return attribute.call(this, name, this instanceof HTMLImageElement && name === 'src' ? artwork(value) : value);
  };
  async function theme(value) {
    window.dispatchEvent(new StorageEvent('storage', { key: 'cumulus.appearance.v1', newValue: value }));
    await wait(() => document.documentElement.dataset.theme === value, 'appearance switched');
  }
  async function capture(name) {
    await sleep(160);
    const bounds = dialog().getBoundingClientRect();
    assert(bounds.top >= 0 && bounds.bottom <= innerHeight + 1, 'dialog clipped');
    assert(!/example\.invalid|sentinel/.test(dialog().innerHTML), 'private material displayed');
    await emit(`ux_${name}`);
    await sleep(550);
  }
  try {
    let account;
    for (let index = 0; index < 100; index++) {
      account = await window.__TAURI_INTERNALS__.invoke('account_snapshot');
      if (account.status === 'authenticated') break;
      await sleep(65);
    }
    assert(account?.status === 'authenticated', 'synthetic account not restored');
    await emit('resize', { width: 760, height: 540 });
    await wait(() => innerWidth === 760 && innerHeight === 540, 'compact window size');
    await theme('dark');
    click(await wait(() => { const node = one('.equalizer-toggle'); return node && !node.disabled && node; }, 'EQ launcher'));
    const opener = button(await wait(() => one('.equalizer-dialog[open]'), 'EQ dialog'), '音效目录');
    click(opener);
    await wait(() => text().includes('合成空间音效'), 'featured list');
    click(one('.effect-directory-card'));
    await wait(() => text().includes('当前账号拥有试用权益'), 'read-only trial rights');
    await wait(artworkLoaded, 'dark artwork displayed');
    await capture('01_trial');
    await theme('light');
    await wait(() => seenArtwork.some((url) => url.endsWith('-light.png')) &&
      artworkLoaded(), 'light artwork displayed');
    await capture('02_light_artwork');
    click(button(dialog(), '官方精选音效'));
    await wait(() => text().includes('合成均衡音效'), 'recommended list');
    click(one('.effect-directory-card'));
    await wait(() => text().includes('暂未确认当前账号的权益'), 'uncertain rights');
    await capture('03_unknown');
    const retry = button(dialog(), '重新查询权益');
    const bounds = retry.getBoundingClientRect();
    assert(retry.contains(document.elementFromPoint(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2)),
      'retry must be visible and hit-testable');
    click(retry);
    assert(document.activeElement === one('.effect-directory-card'), 'retry retains focus inside directory');
    await wait(() => text().includes('当前账号没有使用权益'), 'unavailable rights');
    await capture('04_unavailable');
    click(button(dialog(), '更多音效'));
    await wait(() => text().includes('此分类暂无音效'), 'empty group');
    await capture('05_empty');
    click(button(dialog(), '耳机音效'));
    const selects = () => [...dialog().querySelectorAll('.effect-device-directory select')];
    const select = (node, value) => {
      node.value = String(value);
      assert(node.value === String(value), 'missing device option');
      node.dispatchEvent(new Event('change', { bubbles: true }));
    };
    await wait(() => selects()[0]?.textContent.includes('合成耳机品牌'), 'brands loaded');
    select(selects()[0], 4100);
    await wait(() => selects()[1]?.textContent.includes('合成耳机乙'), 'brand-specific models');
    select(selects()[1], 4102);
    await capture('06_headphones');
    select(selects()[0], 4200);
    await wait(() => text().includes('此品牌暂无耳机型号。') && selects().length === 1, 'empty brand clears old models');
    await capture('07_empty_headphones');
    click(button(dialog(), '关闭音效目录'));
    await wait(() => !dialog() && document.activeElement === opener, 'return focus to equalizer');
    assert(seenArtwork.some((url) => url.endsWith('-dark.png')) && seenArtwork.every((url) =>
      /^https:\/\/p1\.music\.126\.net\/cumulus-proof-artwork-(light|dark)\.png$/.test(url)), 'only sanitized artwork reached WebView');
    await emit('complete', { ok: true, stageCount: 7, artworkVerified: true, devicesVerified: true });
  } catch (error) {
    const image = one('.effect-directory-card img');
    await emit('failed', { message: String(error), body: document.body.innerText.slice(0, 2000),
      seenArtwork, image: image && { complete: image.complete, naturalWidth: image.naturalWidth, inline: image.src.startsWith('data:') } });
  }
})();
