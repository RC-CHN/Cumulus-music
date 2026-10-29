// Production dialog actions with isolated preferences; never starts a download.
(async () => {
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const one = selector => document.querySelector(selector);
  const all = selector => [...document.querySelectorAll(selector)];
  const assert = (ok, message) => { if (!ok) throw Error(message); };
  const wait = async (test, label) => { for (let i = 0; i < 150; i++) { if (test()) return; await sleep(100); } throw Error(label); };
  const emit = (stage, rest = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...rest } });
  const button = text => all('button').find(node => node.textContent.trim() === text);
  const click = node => { assert(node && !node.disabled, `unavailable: ${node?.outerHTML}`); node.focus({ preventScroll: true }); node.click(); };
  const input = (node, value) => { node.value = value; node.dispatchEvent(new Event('input', { bubbles: true })); };
  const rect = node => { const r = node.getBoundingClientRect(); return { x: r.x, y: r.y, right: r.right, bottom: r.bottom, width: r.width, height: r.height }; };
  const hit = node => { const r = rect(node); return node.contains(document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2)); };
  let stageCount = 0;
  const checks = [];
  async function capture(label) {
    await sleep(400);
    const dialog = one('.download-dialog');
    const body = one('.download-settings-body') || dialog;
    const snapshot = {
      width: innerWidth, height: innerHeight, theme: document.documentElement.dataset.theme,
      overflow: document.documentElement.scrollWidth > innerWidth,
      dialog: rect(dialog), body: rect(body), scrollTop: body.scrollTop, scrollHeight: body.scrollHeight,
      header: rect(dialog.querySelector('header')), footer: rect(dialog.querySelector('footer')),
      controls: all('.download-dialog input, .download-dialog select, .download-dialog button').map(node => ({
        label: node.getAttribute('aria-label') || node.textContent.trim() || node.value,
        type: node.type, ...rect(node), hit: hit(node),
      })),
    };
    assert(!snapshot.overflow, 'document overflow');
    await emit(`ux_${String(++stageCount).padStart(2, '0')}_${label}`, snapshot);
    await sleep(1100);
  }
  async function theme(value) {
    click(one('[aria-label="外观主题"]'));
    await wait(() => one('.action-menu'), 'theme menu');
    click(button(value === 'light' ? '浅色' : '深色'));
    await wait(() => document.documentElement.dataset.theme === value && !one('.action-menu'), 'theme');
  }
  async function open() {
    click(button('下载设置'));
    await wait(() => one('.download-dialog[open]'), 'download settings');
  }
  async function close(opener = '下载设置') {
    click(one('[aria-label="关闭下载对话框"]'));
    await wait(() => !one('.download-dialog'), 'closed download settings');
    assert(document.activeElement === button(opener), 'download opener focus not restored');
  }
  try {
    await wait(() => all('.track-row').length, 'daily');
    click(button('下载管理'));
    await wait(() => button('下载设置'), 'downloads page');
    await open();
    input(one('[aria-label="下载目录"]'), window.__downloadProofRoot);
    await wait(() => !button('保存设置')?.disabled, 'download path validation');
    click(button('保存设置'));
    await wait(() => !one('.download-dialog'), 'save isolated empty download directory');
    for (const [width, height, mode] of [[1120,760,'light'],[760,540,'light'],[760,540,'dark'],[1120,760,'dark']]) {
      await emit('resize', { width, height });
      await wait(() => innerWidth === width && innerHeight === height, 'window resize');
      await theme(mode);
      await open();
      input(one('[aria-label="下载目录"]'), window.__downloadProofRoot + '-future');
      await wait(() => one('.download-directory-choice'), 'migration choice');
      const dialog = one('.download-dialog');
      const body = one('.download-settings-body') || dialog;
      body.scrollTop = 0;
      await capture(`download_top_${width}_${mode}`);
      body.scrollTop = body.scrollHeight;
      await capture(`download_bottom_${width}_${mode}`);
      const radios = all('.download-directory-choice input');
      checks.push({ width, theme: mode, radiosCompact: radios.every(node => rect(node).width <= 24 && rect(node).height <= 24),
        choicesReadable: radios.every(node => rect(node.closest('label')).height < 60),
        footerReachable: all('.download-dialog footer button').every(hit) });
      click(radios[1]);
      await wait(() => !!button('保存并迁移'), 'migration choice update');
      await capture(`download_migration_${width}_${mode}`);
      if (!window.__downloadLayoutBaseline) {
        for (const node of all('.download-settings-body input, .download-settings-body select')) {
          node.focus();
          await sleep(100);
          assert(hit(node), `focused field is covered: ${node.getAttribute('aria-label') || node.value}`);
        }
        one('[aria-label="下载音质"]').focus();
        await capture(`download_focus_${width}_${mode}`);
      }
      await close();
    }
    if (!window.__downloadLayoutBaseline) {
      click(button('每日推荐'));
      await wait(() => all('.track-row').length, 'return to daily tracks');
      for (const [width, height, mode] of [[1120,760,'light'],[760,540,'light'],[760,540,'dark'],[1120,760,'dark']]) {
        await emit('resize', { width, height });
        await wait(() => innerWidth === width && innerHeight === height, 'download start resize');
        await theme(mode);
        click(button('下载当前歌曲列表'));
        await wait(() => one('.download-dialog[open]'), 'download options');
        assert(one('.download-selection').title.includes('合成歌曲'), 'selected tracks missing');
        assert(button('开始下载') && !button('开始下载').disabled, 'start action unavailable');
        await capture(`download_start_${width}_${mode}`);
        await close('下载当前歌曲列表');
      }
    }
    await emit('complete', { ok: true, stageCount, checks, realMusicRequests: 0, downloadsStarted: 0, accountWrites: 0 });
  } catch (error) { await emit('failed', { ok: false, stageCount, message: String(error), stack: error.stack }); }
})();
