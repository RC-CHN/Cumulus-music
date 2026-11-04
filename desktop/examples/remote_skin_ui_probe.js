// No real API, CDN, account or package involved; see remote_skin_fixture.rs.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const emit = (stage, detail) =>
    window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
      event: 'ux-proof', payload: { stage, ...detail },
    });
  function check(value, message) {
    if (!value) throw Error(message);
  }
  async function wait(test, message) {
    for (let i = 0; i < 160; i++) {
      if (test()) return;
      await sleep(55);
    }
    throw Error(message);
  }
  function click(node) {
    check(node && !node.disabled, 'missing control');
    node.focus({preventScroll:true});
    node.click();
  }
  const centre = () => one('.skin-center');
  const preview = () => one('.skin-preview[open]');
  const selector = 'button[aria-label="预览 碧色云间"]';
  const skinState = () => one('.skin-read-state')?.textContent;
  const theme = () => document.documentElement.dataset.theme;
  const stored = () => localStorage.getItem('cumulus.appearance.v1');
  let stageCount = 0;
  async function capture(name, sample = null, color = null) {
    await sleep(240);
    const samples = [];
    if (sample) {
      const rect = sample.getBoundingClientRect();
      const x = rect.left + (sample.matches('.sidebar') ? 12 : rect.width / 2);
      const y = rect.top + rect.height / 2;
      check(x >= 0 && x < innerWidth && y >= 0 && y < innerHeight, 'hidden image');
      samples.push({x, y, color});
    }
    await emit(`ux_${String(++stageCount).padStart(2, '0')}_${name}`, {
      captureHandshake: true, ratio: devicePixelRatio, screenX, screenY,
      paintSamples: samples, theme: theme(), active: document.activeElement?.getAttribute('aria-label'),
    });
  }
  try {
    await wait(() => one('[aria-label="换肤"]:not(:disabled)'), 'app ready');
    const before = [theme(), stored()];
    click(one('[aria-label="换肤"]'));
    await wait(() => skinState()?.includes('正在加载'), 'catalogue loading');
    await capture('catalogue_loading', one('.sidebar'), getComputedStyle(one('.sidebar')).backgroundColor);
    await wait(() => skinState()?.includes('加载失败'), 'catalogue error');
    check(all('.skin-card').length === 4, 'bundled cards lost on network failure');
    await capture('catalogue_failed', one('.sidebar'), getComputedStyle(one('.sidebar')).backgroundColor);
    click(one('.skin-read-state button'));
    await wait(() => one('.remote-skin-art img')?.naturalWidth === 128, 'catalogue thumbnails');
    check(all('.skin-group-nav button').map((node)=>node.textContent.trim()).join(',') === '官方,合成新品,合成精选', 'catalogue order and empty group');
    check(all('.remote-skin-card').length === 3, 'catalogue item count');
    check(!one('[aria-label="预览 无公开预览"]'), 'unsafe preview exposed an action');
    check(one('.remote-skin-info small')?.textContent === '黑胶 SVIP', 'catalogue membership badge');
    check(!centre().innerHTML.includes('not-forwarded') && !centre().innerHTML.includes('private-package'), 'upstream URL exposed');
    await capture('catalogue_ready_top', one('.sidebar'), getComputedStyle(one('.sidebar')).backgroundColor);
    click(all('.skin-group-nav button')[1]);
    await wait(() => all('.skin-group-nav button')[1].getAttribute('aria-current') === 'location', 'selected skin group');
    check(one('.remote-skin-card').getBoundingClientRect().width <= one('.skin-card').getBoundingClientRect().width + 2, 'sparse groups stretched their cards');
    await capture('catalogue_read', one('.remote-skin-art img'), 'rgb(56, 134, 186)');
    click(one(selector));
    await wait(() => preview()?.textContent.includes('正在加载预览'), 'preview loading');
    await capture('preview_loading');
    await wait(() => preview()?.textContent.includes('预览图片加载失败'), 'preview interruption');
    await capture('preview_failed');
    click(preview().querySelector('.skin-preview-art button'));
    await wait(() => preview()?.querySelector('img')?.naturalWidth === 128, 'rendered preview');
    check(preview().textContent.includes('皮肤预览') && preview().textContent.includes('碧色云间'), 'preview identity');
    check(preview().querySelectorAll('button').length === 1, 'unsupported apply or purchase button');
    check(preview().querySelector('img').src.startsWith('data:image/png;base64,'), 'unexpected original URL in preview');
    await capture('preview_ready', preview().querySelector('img'), 'rgb(99, 173, 110)');
    click(preview().querySelector('[aria-label="关闭皮肤预览"]'));
    await wait(() => !preview() && document.activeElement === one(selector), 'return to card');
    check(theme() === before[0] && stored() === before[1], 'preview changed theme preference');
    await capture('preview_closed', one('.remote-skin-art img'), 'rgb(56, 134, 186)');
    click(one('[aria-label="返回上一页"]'));
    await wait(() => !centre(), 'back to previous page');
    click(one('[aria-label="换肤"]'));
    await wait(() => skinState()?.includes('暂无在线皮肤'), 'empty catalogue on reopening');
    check(all('.skin-card').length === 4 && !one('.remote-skin-card'), 'empty catalogue incorrectly replaced built-ins');
    await capture('catalogue_empty', one('.sidebar'), getComputedStyle(one('.sidebar')).backgroundColor);
    await emit('complete', {ok:true,stageCount});
  } catch (error) {
    await emit('failed', {ok:false,stageCount,message:String(error),stack:error.stack});
  }
})();
