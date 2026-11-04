// Closed synthetic UX host. Uses the real appearance controls and WebView storage.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => [...document.querySelectorAll(selector)];
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const emit = (stage, detail) =>
    window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
      event: 'ux-proof', payload: { stage, ...detail },
    });
  function check(ok, message) {
    if (!ok) throw Error(message);
  }
  async function wait(test, label) {
    for (let i = 0; i < 120; i++) {
      if (test()) return;
      await sleep(75);
    }
    throw Error(label);
  }
  function click(node) {
    check(node && !node.disabled, 'missing or disabled control');
    node.focus({ preventScroll: true });
    node.click();
  }
  const skin = (name) => one(`.skin-card[data-skin="${name}"]`);
  const entry = () => one('button[aria-label="换肤"]');
  const active = () => one('.skin-card[aria-pressed="true"]')?.dataset.skin;
  const dialog = () => one('.skin-color-dialog[open]');
  const theme = () => document.documentElement.dataset.theme;
  const stored = () => localStorage.getItem('cumulus.appearance.v1');
  const color = () => document.documentElement.style.getPropertyValue('--custom-theme-color');
  const swatch = (name) => dialog()?.querySelector(`.skin-swatch[aria-label="${name}"]`);
  const confirm = () => dialog()?.querySelector('.primary');
  const cancel = () => dialog()?.querySelector('.secondary');
  const sidebar = () => getComputedStyle(one('.sidebar')).backgroundColor;
  const expectedCustom = '{"theme":"custom","color":"#42b66d"}';
  let stageCount = 0;

  async function capture(label) {
    await sleep(380);
    check(document.documentElement.scrollWidth <= innerWidth, 'horizontal document overflow');
    const chip = dialog()?.querySelector('.skin-current-color');
    const sample = chip ?? one('.sidebar');
    const { left, top, width, height } = sample.getBoundingClientRect();
    check(width > 0 && height > 0, 'sample target is not visible');
    const paintSamples = [
      {
        x: left + (chip ? width / 2 : 12),
        y: top + height / 2,
        color: getComputedStyle(sample).backgroundColor,
      },
    ];
    const playerFill = getComputedStyle(one('.play-toggle')).backgroundColor;
    const confirmFill = confirm() ? getComputedStyle(confirm()).backgroundImage : null;
    if (confirmFill && theme() === 'custom') {
      const expectedFill = document.createElement('span');
      expectedFill.style.color = `color-mix(in srgb, ${color()} 43%, #100910)`;
      document.body.append(expectedFill);
      const newFill = getComputedStyle(expectedFill).color;
      expectedFill.remove();
      check(playerFill === newFill && confirmFill.includes(newFill), 'buttons retained stale colors during live preview');
    }
    await emit(`ux_${String(++stageCount).padStart(2, '0')}_${label}`, {
      captureHandshake: true,
      theme: theme(), active: active(), color: color(), stored: stored(), sidebar: sidebar(),
      ratio: devicePixelRatio, screenX, screenY, width: innerWidth, height: innerHeight,
      dialogOpen: !!dialog(), paintSamples,
      playerFill, confirmFill,
    });
  }

  async function openCenter() {
    await wait(() => entry() && !entry().disabled, 'header skin entry did not become available');
    click(entry());
    await wait(() => skin('custom'), 'skin route did not open');
    check(all('.skin-card').map((node) => node.dataset.skin).join(',') === 'light,red,dark,custom', 'official card order');
    check(!one('.skin-center').textContent.includes('音乐桌面') && !one('.skin-center').textContent.includes('头像挂件'), 'unsupported affordance');
  }
  async function openCustom() {
    click(skin('custom'));
    await wait(() => dialog(), 'custom dialog did not open');
  }
  async function closeCustom(node) {
    click(node);
    await wait(() => !dialog(), 'custom dialog did not close');
    check(document.activeElement === skin('custom'), 'focus did not return to custom card');
  }
  try {
    if (window.__CUMULUS_BUNDLED_THEME_REOPEN__) {
      await wait(() => one('.app-shell') && entry() && !entry().disabled, 'reopened window not ready');
      check(theme() === 'custom' && color() === '#42b66d' && stored() === expectedCustom, 'unconfirmed draft crossed process boundary');
      await openCenter();
      check(active() === 'custom', 'custom not selected on restart');
      await capture('reopened_applied');
      await openCustom();
      check(swatch('翠绿')?.getAttribute('aria-pressed') === 'true', 'confirmed color missing in reopened dialog');
      click(swatch('粉色'));
      check(color() === '#f8789b' && stored() === expectedCustom, 'reopened edit was persisted before Apply');
      await closeCustom(cancel());
      check(theme() === 'custom' && color() === '#42b66d' && stored() === expectedCustom, 'cancel did not restore committed custom');
      await capture('reopened_cancel');
    } else {
      await openCenter();
      click(skin('light'));
      await wait(() => theme() === 'light', 'light did not apply');
      check(active() === 'light' && stored() === 'light' && sidebar() === 'rgb(248, 248, 251)', 'light not applied or saved');
      await capture('light');
      click(skin('dark'));
      await wait(() => theme() === 'dark', 'dark did not apply');
      check(active() === 'dark' && stored() === 'dark' && sidebar() === 'rgb(24, 25, 32)', 'dark not applied or saved');
      await capture('dark');
      click(skin('red'));
      await wait(() => theme() === 'red', 'red did not apply');
      check(active() === 'red' && stored() === 'red' && sidebar() === 'rgb(255, 245, 245)', 'classic red is not a distinct applied theme');
      await capture('red');
      await openCustom();
      check(confirm().disabled && theme() === 'red', 'opening color editor changed theme');
      click(swatch('翠绿'));
      check(theme() === 'custom' && color() === '#42b66d' && stored() === 'red', 'swatch not shown as unsaved live preview');
      check(active() === 'red', 'preview was treated as a committed card');
      const hue = dialog().querySelector('.skin-hue');
      hue.value = '260';
      hue.dispatchEvent(new Event('input', { bubbles: true }));
      check(color() !== '#42b66d' && stored() === 'red', 'slider did not update unsaved preview');
      await capture('live_slider_preview');
      await closeCustom(cancel());
      check(theme() === 'red' && color() === '' && stored() === 'red', 'cancel did not restore prior skin');
      await capture('cancelled_to_red');
      await openCustom();
      click(swatch('翠绿'));
      await wait(() => confirm() && !confirm().disabled, 'Apply was not enabled after a color selection');
      await closeCustom(confirm());
      check(theme() === 'custom' && active() === 'custom' && stored() === expectedCustom, 'explicit custom Apply failed');
      await capture('custom_applied');
      click(one('button[aria-label="返回上一页"]'));
      await wait(() => !one('.skin-center') && entry().getAttribute('aria-pressed') === 'false', 'header Back did not return');
      await openCenter();
      check(active() === 'custom', 'return navigation reset the applied appearance');
      await emit('resize', { width: 760, height: 540 });
      await wait(() => innerWidth === 760 && innerHeight === 540, 'compact viewport');
      await openCustom();
      click(swatch('橙色'));
      check(color() === '#fb8c55' && stored() === expectedCustom, 'latest draft leaked into storage');
      const footer = dialog().querySelector('footer').getBoundingClientRect();
      check(footer.bottom < innerHeight && footer.top > 0, 'compact controls are clipped');
      await capture('compact_uncommitted_exit');
      // Exit with the draft still open. The second OS process must see the previous Apply.
    }
    await emit('complete', { ok: true, stageCount, reopened: !!window.__CUMULUS_BUNDLED_THEME_REOPEN__ });
  } catch (error) {
    await emit('failed', { ok: false, stageCount, message: String(error), stack: error.stack });
  }
})();
