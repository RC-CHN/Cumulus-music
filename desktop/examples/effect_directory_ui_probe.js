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
      const result = await predicate();
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
    const native = !!window.__cumulusDeviceNative;
    const invoke = (command, args) => window.__TAURI_INTERNALS__.invoke(command, args);
    const player = () => invoke('player_snapshot', { epoch: account.epoch });
    if (native) {
      await invoke('play_selection', { epoch: account.epoch, selection: {
        kind: 'songs', ids: ['1'], index: 0, quality: 'standard',
      } });
      await wait(async () => (await player()).state === 'playing', 'native fixture playback');
    }
    await emit('resize', { width: 760, height: 540 });
    await wait(() => innerWidth === 760 && innerHeight === 540, 'compact window size');
    await theme('dark');
    click(await wait(() => { const node = one('.equalizer-toggle'); return node && !node.disabled && node; }, 'EQ launcher'));
    const opener = button(await wait(() => one('.equalizer-dialog[open]'), 'EQ dialog'), '音效目录');
    click(opener);
    await wait(() => text().includes('合成空间音效'), 'featured list');
    assert(one('[data-effect-membership="svip"][data-effect-limited-free]')?.textContent.trim() === 'SVIP 限免',
      'future catalogue trial shows the limited-free tier');
    click(one('.effect-directory-card'));
    await wait(() => text().includes('当前账号拥有试用权益'), 'read-only trial rights');
    await wait(artworkLoaded, 'dark artwork displayed');
    await capture('01_trial');
    await theme('light');
    await wait(() => seenArtwork.some((url) => url.endsWith('-light.png')) &&
      artworkLoaded(), 'light artwork displayed');
    await capture('02_light_artwork');
    await wait(() => one('[data-effect-membership="svip"]')?.textContent.trim() === 'SVIP',
      'catalogue trial badge expires without a new request');
    assert(text().includes('当前账号拥有试用权益'), 'expiry does not rewrite fresh rights readback');
    click(button(dialog(), '官方精选音效'));
    await wait(() => text().includes('合成未标识音效'), 'recommended list');
    const cards = [...dialog().querySelectorAll('.effect-directory-card')];
    assert(cards[0].querySelector('[data-effect-membership="vip"]')?.textContent.trim() === 'VIP' &&
      !cards[1].querySelector('[data-effect-membership]') && !text().includes('限免'),
      'only known tiers get decorative badges; elapsed trial does not show limited-free');
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
    const material = () => one('.effect-device-material:not([data-device-tuning])');
    const state = () => material()?.dataset.materialCheck;
    assert(!state(), 'choosing a model does not fetch its material');
    const prior = await player();
    click(button(material(), '检查调音材料'));
    await wait(() => state() === 'standalone_peq' && button(material(), '启用耳机调音'),
      'exact model material is verified and previewed without applying it');
    assert(!/synthetic-bound-peq|[a-f0-9]{32}/.test(material().innerHTML),
      'the material URL and digest stay native');
    await capture('06_headphones');
    select(selects()[1], 4101);
    await wait(() => !state(), 'selecting a different model clears the old result');
    click(button(material(), '检查调音材料'));
    await wait(() => state() === 'no_material', 'model without a descriptor is unavailable');
    select(selects()[1], 4102);
    await wait(() => !state(), 'restoring the first model requires an explicit check');
    click(button(material(), '检查调音材料'));
    await wait(() => state() === 'standalone_peq', 'revalidated model reuses its verified material');
    const after = await player();
    assert(after.state === prior.state && after.equalizer.revision === prior.equalizer.revision &&
      after.boundParametric === prior.boundParametric && !after.deviceParametric,
    'material checks do not change playback effects');
    if (native) {
      const currentTuning = () => one('[data-device-tuning]');
      const sameSong = (value) => value.state === 'playing' && value.playId === prior.playId &&
        value.entryId === prior.entryId && value.equalizer.revision === prior.equalizer.revision;
      const apply = async () => {
        click(button(material(), '启用耳机调音'));
        const start = await player();
        await wait(async () => {
          const value = await player();
          return currentTuning()?.dataset.deviceTuning === 'configured' && sameSong(value) &&
            value.position > start.position + 0.3 && value.deviceParametric?.brand === 4100 &&
            value.deviceParametric.model === 4102 && !value.boundParametric;
        }, 'the exact native device filter keeps the same song moving');
      };
      await apply();
      assert(document.activeElement === button(currentTuning(), '关闭耳机调音'),
        'keyboard focus follows the completed apply action');
      await capture('07_headphones_enabled');
      const exclusive = await invoke('effect_peq_activate', {
        epoch: account.epoch, id: 300, attempt: 'closed-fixture',
      }).then(() => false, (error) => error.code === 'headphone_tuning_active');
      assert(exclusive, 'ordinary PEQ activation cannot register while headphone tuning is mounted');
      const wrong = await invoke('effect_device_clear', {
        epoch: account.epoch, brand: 4200, model: 4102,
      }).then(() => false, () => true);
      assert(wrong && (await player()).deviceParametric?.status === 'configured',
        'an unrelated device identity cannot clear the installed filter');
      click(button(currentTuning(), '关闭耳机调音'));
      const clearing = await player();
      await wait(async () => {
        const value = await player();
        return !currentTuning() && !value.deviceParametric && sameSong(value) &&
          value.position > clearing.position + 0.25;
      }, 'disable removes only tuning and retains the same playing song');
      assert(document.activeElement === button(material(), '启用耳机调音'),
        'keyboard focus returns to applying the selected model');
      await capture('08_headphones_disabled');
      await apply();
      await emit('effect-peq-fail-runtime');
      const beforeFault = await player();
      await wait(async () => {
        const value = await player();
        return currentTuning()?.dataset.deviceTuning === 'processor_failed' &&
          value.deviceParametric?.status === 'processor_failed' && sameSong(value) &&
          value.position > beforeFault.position + 0.25;
      }, 'a verified native detach keeps the same song playing after a filter fault');
      await capture('09_headphones_fault');
      click(button(currentTuning(), '关闭调音提示'));
      await wait(() => !currentTuning(), 'clear the detached filter notice');
    }
    select(selects()[0], 4200);
    await wait(() => text().includes('此品牌暂无耳机型号。') && selects().length === 1, 'empty brand clears old models');
    await capture(native ? '10_empty_headphones' : '07_empty_headphones');
    select(selects()[0], 4100);
    await Promise.resolve();
    assert(selects()[1]?.textContent.includes('合成耳机乙') && selects()[1]?.value === '',
      'visited brand restores its models without restoring a selection');
    select(selects()[0], 4200);
    await Promise.resolve();
    assert(text().includes('此品牌暂无耳机型号。'), 'visited empty brand restores the empty result');
    click(button(dialog(), '关闭音效目录'));
    await wait(() => !dialog() && document.activeElement === opener, 'return focus to equalizer');
    assert(seenArtwork.some((url) => url.endsWith('-dark.png')) && seenArtwork.every((url) =>
      /^https:\/\/p1\.music\.126\.net\/cumulus-proof-artwork-(light|dark)\.png$/.test(url)), 'only sanitized artwork reached WebView');
    await emit('complete', { ok: true, stageCount: native ? 10 : 7, artworkVerified: true,
      devicesVerified: true, deviceTuningVerified: native, deviceMaterialVerified: true,
      deviceCacheVerified: true, limitedFreeExpiryVerified: true });
  } catch (error) {
    const image = one('.effect-directory-card img');
    await emit('failed', { message: String(error), body: document.body.innerText.slice(0, 2000),
      seenArtwork, image: image && { complete: image.complete, naturalWidth: image.naturalWidth, inline: image.src.startsWith('data:') } });
  }
})();
