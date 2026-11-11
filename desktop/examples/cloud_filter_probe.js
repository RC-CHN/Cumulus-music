// Actual CloudPage input, production IPC, and an isolated account list.
(async () => {
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = payload => invoke('plugin:event|emit', { event: 'mini-proof', payload });
  const page = () => document.querySelector('.cloud-page');
  const input = () => page()?.querySelector('[aria-label="搜索云盘歌曲"]');
  const ids = () => [...(page()?.querySelectorAll('[data-cloud-id]') ?? [])].map(row => row.dataset.cloudId);
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const assert = (value, label) => { if (!value) throw new Error(label); };
  let count = 0;
  let last = 'startup';
  async function wait(check, label) {
    for (let i = 0; i < 200; i++) {
      if (check()) return;
      await sleep(65);
    }
    throw new Error(label);
  }
  async function ready(expected, label) {
    await wait(() => page()?.dataset.cloudLoading === 'false' &&
      page()?.dataset.cloudReady === 'true' && JSON.stringify(ids()) === JSON.stringify(expected), label);
  }
  function type(value, isComposing = false) {
    const control = input();
    control.value = value;
    control.dispatchEvent(new InputEvent('input', { bubbles: true, isComposing, data: value }));
  }
  async function capture(label) {
    last = label;
    const stage = `ux_${String(++count).padStart(2, '0')}_${label}`;
    await emit({
      kind: 'snapshot', stage, label, ids: ids(), keyword: input().value,
      captureTitle: 'Cumulus', windowKind: 'main', width: innerWidth, height: innerHeight,
      screenX, screenY, ratio: devicePixelRatio, visible: document.visibilityState === 'visible',
      focused: document.hasFocus(), inputFocused: document.activeElement === input(),
    });
    await wait(() => window.__miniRecorded === stage, 'native ' + stage);
  }
  try {
    const cloud = () => [...document.querySelectorAll('button')].find(button => button.textContent.trim() === '音乐云盘');
    await wait(() => cloud() && document.querySelector('.track-row .track-play:not(:disabled)'),
      'synthetic account has loaded its playable daily list');
    const account = await invoke('account_snapshot');
    assert(account.status === 'authenticated', 'synthetic account restored');
    cloud().click();
    const all = ['1008', '1007', '1006', '1005', '1004', '1003', '1002', '1001'];
    await ready(all, 'full cloud list');
    await capture('cloud_filter_initial');
    input().focus({ preventScroll: true });
    type('Ａ＋Ｂ');
    await ready(['1001'], 'fullwidth literal plus filters without form submit');
    await capture('cloud_filter_width_literal');

    input().dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }));
    type('樂', true);
    await ready(['1002', '1001'], 'IME draft filters while composing');
    assert(document.activeElement === input(), 'in-list projection preserves IME input focus');
    await capture('cloud_filter_composing');
    input().value = '台湾 录音';
    input().dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: '台湾 录音' }));
    await ready(['1003'], 'composition end matches first translation');
    await capture('cloud_filter_translation');

    type('文件独有');
    await ready([], 'filename is not a cloud-origin match');
    assert(page().textContent.includes('没有找到相关歌曲'), 'shows the cloud empty-query state');
    await capture('cloud_filter_filename_excluded');
    page().querySelector('[aria-label="清空云盘搜索"]').click();
    await ready(all, 'clear restores uploaded order');
    await capture('cloud_filter_cleared');
    await emit({ kind: 'complete', stage: 'complete', ok: true, count });
  } catch (error) {
    await emit({
      kind: 'failed', stage: 'failed', error: String(error), last, ids: ids(),
      keyword: input()?.value, body: page()?.textContent,
    });
  }
})();
