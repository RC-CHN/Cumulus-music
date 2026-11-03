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
    click(await wait(() => { const node = one('.equalizer-toggle'); return node && !node.disabled && node; }, 'EQ launcher'));
    const opener = button(await wait(() => one('.equalizer-dialog[open]'), 'EQ dialog'), '音效目录');
    click(opener);
    await wait(() => text().includes('合成空间音效'), 'featured list');
    click(one('.effect-directory-card'));
    await wait(() => text().includes('当前账号拥有试用权益'), 'read-only trial rights');
    await capture('01_trial');
    click(button(dialog(), '官方精选音效'));
    await wait(() => text().includes('合成均衡音效'), 'recommended list');
    click(one('.effect-directory-card'));
    await wait(() => text().includes('暂未确认当前账号的权益'), 'uncertain rights');
    await capture('02_unknown');
    click(button(dialog(), '重新查询权益'));
    assert(document.activeElement === one('.effect-directory-card'), 'retry retains focus inside directory');
    await wait(() => text().includes('当前账号没有使用权益'), 'unavailable rights');
    await capture('03_unavailable');
    click(button(dialog(), '更多音效'));
    await wait(() => text().includes('此分类暂无音效'), 'empty group');
    await capture('04_empty');
    click(button(dialog(), '关闭音效目录'));
    await wait(() => !dialog() && document.activeElement === opener, 'return focus to equalizer');
    await emit('complete', { ok: true, stageCount: 4 });
  } catch (error) {
    await emit('failed', { message: String(error), body: document.body.innerText.slice(0, 2000) });
  }
})();
