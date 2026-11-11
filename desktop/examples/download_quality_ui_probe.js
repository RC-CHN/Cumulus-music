// Real settings and single-song download over the isolated native download fixture.
(async () => {
  const one = (selector, root = document) => root?.querySelector(selector);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (value, message) => {
    if (!value) throw Error(message);
  };
  async function wait(read, message, attempts = 260) {
    for (let i = 0; i < attempts; i++) {
      const value = await read();
      if (value) return value;
      await sleep(50);
    }
    throw Error(message);
  }
  const invoke = (command, args) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, data = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...data } });
  const button = (name, root = document) =>
    [...(root?.querySelectorAll('button') ?? [])].find(
      (node) => node.getAttribute('aria-label') === name || node.textContent.trim() === name,
    );
  function click(node) {
    assert(node && !node.matches(':disabled,[aria-disabled=true]'), 'enabled control: ' + node?.outerHTML);
    node.focus({ preventScroll: true });
    node.click();
  }
  const settingsButton = () => one('.download-navigation-actions [aria-label="下载设置"]');
  const dialog = () => one('dialog.download-dialog[open]');
  const choiceGroup = () => one('.download-quality-choices', dialog());
  const choiceOrder = ['沉浸环绕声', '超清母带', '高清臻音', 'Hi-Res', '无损', '极高', '标准'];
  function choices() {
    return [...(choiceGroup()?.querySelectorAll('label') ?? [])].map((label) => {
      const radio = one('input', label);
      const name = one('.download-quality-name', label).textContent.trim();
      return { name, radio, disabled: radio.matches(':disabled'), checked: radio.checked,
        caption: one('small', label)?.textContent.trim() ?? '' };
    });
  }
  let lastChoiceLayout = [];
  function assertChoices(verified) {
    const rows = choices();
    assert(JSON.stringify(rows.map((row) => row.name)) === JSON.stringify(choiceOrder), 'official settings rows');
    assert(rows.slice(0, 2).every((row) => row.disabled && row.caption === (verified ? '需黑胶 SVIP' : '权益待核对')), 'SVIP rows');
    assert(rows.slice(2, 5).every((row) => row.disabled === !verified && row.caption === (verified ? '' : '权益待核对')), 'verified VIP rows');
    assert(rows.slice(5).every((row) => !row.disabled && !row.caption), 'free rows');
    // Glyph ranges have different ascents for Latin and CJK fallback fonts.
    // Measure the laid-out name and caption boxes, not the individual glyphs.
    lastChoiceLayout = rows.map((row) => {
      const label = row.radio.closest('label');
      return { quality: row.name, top: Math.round(label.getBoundingClientRect().top),
        name: one('.download-quality-name', label).getBoundingClientRect().toJSON(),
        caption: one('small', label)?.getBoundingClientRect().toJSON() ?? null };
    });
    const firstRow = new Map();
    for (const { top, name, caption } of lastChoiceLayout) {
      assert(name.width > 0 && name.height > 0, 'quality name has a visible layout box');
      assert(!caption || (caption.width > 0 && caption.height > 0 &&
        caption.top >= name.bottom && Math.abs(caption.left - name.left) < 1),
        'rights caption stays under the name');
      assert(!firstRow.has(top) || Math.abs(firstRow.get(top) - name.top) < 1,
        'quality names align even when a neighbour has an access caption');
      firstRow.set(top, name.top);
    }
  }
  function retainedVivid() {
    return dialog()?.textContent.includes('保留已保存的音质：臻音全景声') &&
      choices().every((row) => !row.checked);
  }
  const readSettled = () => choiceGroup() && !dialog()?.textContent.includes('正在读取下载音质');
  let stage = 'startup', count = 0;
  async function capture(label, extra = {}) {
    stage = label;
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    assert(document.documentElement.scrollWidth <= innerWidth, 'no document overflow');
    const rows = choices().map(({ radio: _radio, ...row }) => row);
    await emit('ux_' + String(++count).padStart(2, '0') + '_' + label, { rows, ...extra });
  }
  async function openSettings() {
    const origin = await wait(settingsButton, 'download settings origin');
    click(origin);
    await wait(readSettled, 'download settings choice read');
    return origin;
  }
  async function closeSettings(origin, save = false) {
    click(button(save ? '保存设置' : '取消', dialog()));
    await wait(() => !dialog() && document.activeElement === origin, 'settings origin focus');
  }
  try {
    await wait(() => one('.track-row[data-track-id="1"]'), 'synthetic catalogue track');
    window.__downloadPageAck = null;
    await emit('download-page-fixture', { mode: 'seed_quality' });
    await wait(() => window.__downloadPageAck, 'synthetic startup membership failure and seed');
    assert(window.__downloadPageAck.ok, window.__downloadPageAck.message);

    click(button('下载管理'));
    const origin = await openSettings();
    assertChoices(false);
    assert(retainedVivid(), 'unverified, hidden stored Vivid survives initial open');
    await capture('startup_unknown', { retained: true });

    click(button('刷新下载音质', dialog()));
    const retry = await wait(() => {
      const alert = dialog()?.querySelector('[role="alert"]');
      return alert && button('重试', alert);
    }, 'refresh failure and retry');
    assert(retainedVivid() && button('保存设置', dialog()).disabled, 'failed read keeps prior choice inert');
    await capture('manual_failure', { retained: true, message: retry.closest('[role="alert"]').textContent.trim() });
    click(retry);
    await wait(() => readSettled() && !button('重试', dialog()) && !choices()[4]?.disabled, 'membership recovery');
    assertChoices(true);
    assert(choices()[2].checked && !retainedVivid(), 'verified Vinyl read clamps the saved default before returning');
    await capture('membership_recovered', { clamped: 'jyeffect' });

    click(choices()[4].radio);
    await closeSettings(origin, true);
    const reopened = await openSettings();
    assertChoices(true);
    assert(choices()[4].checked && !retainedVivid(), 'lossless saved on reopen');
    await capture('default_saved');
    await closeSettings(reopened);

    click(button('返回上一页'));
    const more = await wait(() => one('.track-row[data-track-id="1"] [aria-label^="更多操作"]'), 'return to song');
    click(more);
    const download = await wait(
      () => one('.action-menu[aria-label="歌曲操作"] [data-menu-key="download"]'),
      'single song download action',
    );
    click(download);
    const quality = await wait(() => one('select[aria-label="下载音质"]', dialog()), 'single download form');
    assert(quality.value === 'lossless', 'single-download form starts at the saved default');
    quality.value = 'exhigh';
    quality.dispatchEvent(new Event('change', { bubbles: true }));
    assert(quality.value === 'exhigh', 'per-download choice');
    await capture('single_download_choice', { selected: quality.value });
    click(button('开始下载', dialog()));
    await wait(() => !dialog() && settingsButton(), 'download task accepted and downloads page entered');

    const { epoch } = await invoke('account_snapshot');
    const task = await wait(async () => {
      const view = await invoke('downloads', { epoch, query: {
        tab: 'completed', category: 'song', keyword: '', sort: 'completed', descending: true, offset: 0, limit: 100,
      } });
      return view.tasks.find((row) => row.track.id === '1' && row.status === 'complete' && !row.cleanupPending);
    }, 'native synthetic download publication');
    assert(task.requestedQuality === 'exhigh' && task.actualQuality === 'standard', 'native request and authoritative actual quality');
    const after = await openSettings();
    assertChoices(true);
    assert(choices()[4].checked, 'different download did not update the saved default');
    await capture('default_preserved', { requested: task.requestedQuality, actual: task.actualQuality });
    await closeSettings(after);
    await emit('complete', { ok: true, downloadQualityUi: true, stageCount: count,
      realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', { step: stage, stageCount: count, message: String(error), stack: error.stack,
      viewport: [innerWidth, innerHeight], lastChoiceLayout });
  }
})();
