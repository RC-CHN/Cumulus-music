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
    if (window.__cumulusDeviceReopened) {
      const launcher = () => one('.playerbar .equalizer-toggle');
      await wait(async () => {
        const current = await player();
        return launcher() && !launcher().disabled && current.state === 'idle' &&
          current.queueLen === 0 && !current.playId && !current.deviceParametric;
      }, 'reopened process is ready and has no active playback or tuning');
      click(launcher());
      const eq = await wait(() => one('.equalizer-dialog[open]'), 'ordinary equalizer');
      click(button(eq, '耳机音效'));
      await wait(() => dialog()?.getAttribute('aria-label') === '耳机音效' &&
        text().includes('上次使用') && button(dialog(), '恢复耳机调音'), 'local remembered selection');
      const stored = await invoke('effect_device_remembered', { epoch: account.epoch });
      assert(stored?.brand === 4100 && stored?.model === 4102, 'stored choice belongs to the restored account');
      await capture('01_remembered_local');
      click(button(dialog(), '恢复耳机调音'));
      const configured = await wait(async () => {
        const current = await player();
        return current.deviceParametric?.status === 'configured' && current;
      }, 'explicit restore configures the idle native player');
      assert(configured.state === 'idle' && !configured.playId && configured.queueLen === 0 &&
        configured.deviceParametric.modelName === '合成耳机乙', 'restore does not select or load audio');
      assert((await invoke('effect_device_remembered', { epoch: account.epoch })).revision === stored.revision,
        'restoring the same selection does not rewrite the remembered choice');
      await capture('02_configured_idle');
      click(button(dialog(), '关闭耳机音效管理'));
      await wait(() => !dialog(), 'close the local manager');
      click(button(eq, '关闭均衡器'));
      await wait(() => !one('.equalizer-dialog[open]'), 'return to the ordinary music list');
      click(await wait(() => one('.track-row .track-play:not(:disabled)'), 'daily music play button'));
      await wait(async () => {
        const current = await player();
        return current.state === 'playing' && current.position > 0.35 &&
          current.deviceParametric?.activationId === configured.deviceParametric.activationId;
      }, 'only explicit Play starts audio with the same tuning');
      click(launcher());
      await wait(() => text().includes('合成耳机乙'), 'footer reports the playing configured model');
      await capture('03_explicit_play');
      await emit('complete', { ok: true, rememberedAcrossProcesses: true, stageCount: 3 });
      return;
    }
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
    const currentTuning = () => one('[data-device-tuning]');
    let equalizerRevision = prior.equalizer.revision;
    const sameSong = (value) => value.state === 'playing' && value.playId === prior.playId &&
      value.entryId === prior.entryId && value.equalizer.revision === equalizerRevision;
    const launcher = () => one('.playerbar .equalizer-toggle');
    if (native) {
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
      const firstActivation = (await player()).deviceParametric.activationId;
      assert(/^[a-f0-9]{32}$/.test(firstActivation), 'actor supplies a narrow opaque activation');
      const exclusive = await invoke('effect_peq_activate', {
        epoch: account.epoch, id: 300, attempt: 'closed-fixture',
      }).then(() => false, (error) => error.code === 'headphone_tuning_active');
      assert(exclusive, 'ordinary PEQ activation cannot register while headphone tuning is mounted');
      const wrong = await invoke('effect_device_clear', {
        epoch: account.epoch, brand: 4200, model: 4102, activationId: firstActivation,
      }).then(() => false, (error) => error.code === 'stale');
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
      const replacement = (await player()).deviceParametric;
      assert(replacement.activationId !== firstActivation, 'reapplying the same model makes a new activation');
      const stale = await invoke('effect_device_clear', {
        epoch: account.epoch, brand: replacement.brand, model: replacement.model,
        activationId: firstActivation,
      }).then(() => false, (error) => error.code === 'stale');
      assert(stale && (await player()).deviceParametric?.activationId === replacement.activationId,
        'a stale clear cannot detach a new activation of the same model');
    }
    select(selects()[0], 4200);
    await wait(() => text().includes('此品牌暂无耳机型号。') && selects().length === 1, 'empty brand clears old models');
    await capture(native ? '09_empty_headphones' : '07_empty_headphones');
    select(selects()[0], 4100);
    await Promise.resolve();
    assert(selects()[1]?.textContent.includes('合成耳机乙') && selects()[1]?.value === '',
      'visited brand restores its models without restoring a selection');
    select(selects()[0], 4200);
    await Promise.resolve();
    assert(text().includes('此品牌暂无耳机型号。'), 'visited empty brand restores the empty result');
    click(button(dialog(), '关闭音效目录'));
    await wait(() => !dialog() && document.activeElement === opener, 'return focus to equalizer');
    if (native) {
      click(button(one('.equalizer-dialog[open]'), '关闭均衡器'));
      await wait(() => !one('.equalizer-dialog[open]') && document.activeElement === launcher() &&
        launcher()?.getAttribute('aria-label') === '管理耳机音效' &&
        launcher()?.dataset.headphoneState === 'configured', 'footer reports the active headphone tuning');
      // This synthetic adapter processes PEQ but does not provide a graphic
      // equalizer. Its warning must stay visible beside a configured device.
      const combined = await invoke('player_action', { epoch: account.epoch, action: {
        kind: 'equalizer', expected_revision: equalizerRevision,
        settings: { enabled: true, gains: [3, 0, 0, 0, 0, 0, 0, 0, 0, 0] },
      } });
      equalizerRevision = combined.equalizer.revision;
      await wait(() => launcher()?.dataset.effectState === 'problem' &&
        launcher()?.dataset.equalizerState === 'problem' && launcher()?.dataset.headphoneState === 'configured' &&
        launcher().title.includes('耳机调音') && launcher().title.includes('均衡器'),
      'the headphone shortcut retains the independent graphic EQ warning');
      assert(getComputedStyle(launcher()).whiteSpace === 'nowrap' &&
        document.documentElement.scrollWidth === innerWidth, 'headphone label fits in the compact footer');
      click(launcher());
      await wait(() => dialog()?.getAttribute('aria-label') === '耳机音效' &&
        currentTuning()?.dataset.deviceTuning === 'configured', 'direct management reflects current tuning');
      assert(currentTuning().textContent.includes('合成耳机乙') && !selects().length &&
        button(dialog(), '浏览耳机音效') && button(dialog(), '打开均衡器'),
        'current management offers explicit browse and equalizer without loading either');
      await capture('10_headphones_footer');
      click(button(dialog(), '打开均衡器'));
      await wait(() => !dialog() && one('.equalizer-dialog[open]'), 'graphic equalizer opens separately');
      assert((await player()).deviceParametric?.status === 'configured', 'viewing the graphic equalizer preserves tuning');
      click(button(one('.equalizer-dialog[open]'), '关闭均衡器'));
      await wait(() => !one('.equalizer-dialog[open]') && document.activeElement === launcher(),
        'closing the graphic equalizer returns to the headphone launcher');
      click(launcher());
      await wait(() => currentTuning()?.dataset.deviceTuning === 'configured', 'local management can reopen');
      await emit('effect-peq-fail-runtime');
      const beforeFault = await player();
      await wait(async () => {
        const value = await player();
        return currentTuning()?.dataset.deviceTuning === 'processor_failed' &&
          launcher()?.dataset.headphoneState === 'processor_failed' &&
          value.deviceParametric?.status === 'processor_failed' && sameSong(value) &&
          value.position > beforeFault.position + 0.25;
      }, 'verified detach leaves a manageable failure notice and keeps the song playing');
      assert((await player()).deviceParametric.activationId === beforeFault.deviceParametric.activationId,
        'the failure notice keeps the exact activation identity');
      await capture('11_headphones_fault');
      click(button(currentTuning(), '关闭调音提示'));
      await wait(() => !currentTuning() && document.activeElement === button(dialog(), '浏览耳机音效'),
        'dismiss restores keyboard focus within local headphone management');
      const saved = button(dialog(), '恢复耳机调音');
      assert(saved && !saved.disabled && text().includes('上次使用'),
        'explicit off or failure does not discard the remembered selection');
      click(saved);
      const restoring = await player();
      await wait(async () => {
        const value = await player();
        return currentTuning()?.dataset.deviceTuning === 'configured' &&
          value.deviceParametric?.status === 'configured' &&
          value.deviceParametric.modelName === '合成耳机乙' &&
          currentTuning().textContent.includes('合成耳机乙') && sameSong(value) &&
          value.position > restoring.position + 0.25;
      }, 'explicit restore revalidates the saved model without starting a new track');
      await capture('12_headphones_restored');
      click(button(dialog(), '关闭耳机音效管理'));
      await wait(() => !dialog() && document.activeElement === launcher() &&
        launcher()?.getAttribute('aria-label') === '管理耳机音效', 'closing restores the footer focus');
      click(button(one('.playerbar'), '停止播放'));
      await wait(async () => (await player()).state === 'idle' &&
        launcher()?.getAttribute('aria-label') === '均衡器', 'stop does not restore the remembered tuning');
      const idle = await player();
      await invoke('player_action', { epoch: account.epoch, action: {
        kind: 'queue_for', expected_queue_revision: idle.queueRevision, edit: { kind: 'clear' },
      } });
      assert((await player()).queueLen === 0, 'reopen starts with an empty synthetic queue');
    }
    assert(seenArtwork.some((url) => url.endsWith('-dark.png')) && seenArtwork.every((url) =>
      /^https:\/\/p1\.music\.126\.net\/cumulus-proof-artwork-(light|dark)\.png$/.test(url)), 'only sanitized artwork reached WebView');
    await emit('complete', { ok: true, stageCount: native ? 12 : 7, artworkVerified: true,
      devicesVerified: true, deviceTuningVerified: native, deviceMaterialVerified: true,
      deviceCacheVerified: true, limitedFreeExpiryVerified: true, headphoneFooterVerified: native });
  } catch (error) {
    const image = one('.effect-directory-card img');
    await emit('failed', { message: String(error), body: document.body.innerText.slice(0, 2000),
      seenArtwork, image: image && { complete: image.complete, naturalWidth: image.naturalWidth, inline: image.src.startsWith('data:') } });
  }
})();
