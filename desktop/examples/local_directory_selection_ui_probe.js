// Guest local folder settings in real WebView and native scanner. The host
// drives the real GTK folder chooser in the first stage.
(async () => {
  const one = (selector, root = document) => root.querySelector(selector);
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, data = {}) => invoke('plugin:event|emit', {
    event: 'local-proof', payload: { stage, ...data },
  });
  const delay = (ms) => new Promise((done) => setTimeout(done, ms));
  const check = (condition, why) => { if (!condition) throw Error(why); };
  const wait = async (test, why) => {
    const end = performance.now() + 16000;
    while (performance.now() < end) {
      const result = await test();
      if (result) return result;
      await delay(60);
    }
    throw Error(why);
  };
  const button = (text, root = document) => [...root.querySelectorAll('button')].find(
    (item) => item.textContent.trim() === text || item.getAttribute('aria-label') === text,
  );
  const click = (item) => {
    check(item && !item.disabled, 'enabled control: ' + (item?.textContent ?? 'missing'));
    item.focus({ preventScroll: true });
    item.click();
  };
  const dialog = () => one('dialog[data-local-directories][open]');
  const trigger = () => button('选择目录', one('.local-page') ?? document);
  const box = () => dialog()?.querySelector('input[type="checkbox"]');
  function inlineOption() {
    const checkbox = box(), option = checkbox?.closest('label')?.querySelector('span');
    if (!checkbox || !option) return false;
    const a = checkbox.getBoundingClientRect(), b = option.getBoundingClientRect();
    return a.width > 0 && a.right < b.left && Math.abs(a.top - b.top) < 10 &&
      b.right <= dialog().getBoundingClientRect().right;
  }
  const query = {
    keyword: '', sort: 'title', descending: true, group: 'list', groupKey: null,
    includeMissing: true, offset: 0, limit: 200, revision: null,
  };
  const read = () => invoke('local_page', { query });
  const dirs = () => invoke('local_directory_selection');
  const data = async () => {
    const page = await read();
    const roots = (await dirs()).entries.map(({ checked, paused }) => ({ checked, paused }));
    return { count: page.total, roots };
  };
  let step = 'bootstrap', count = 0;
  async function capture(label, extra = {}) {
    step = label;
    const stage = `ux_${String(++count).padStart(2, '0')}_${label}`;
    await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));
    window.__folderCaptureAck = '';
    await emit(stage, { ...(await data()), ...extra });
    if (window.__localProof.capture)
      await wait(() => window.__folderCaptureAck === stage, 'capture acknowledgement: ' + label);
  }
  const focusReturned = () => !one('dialog[data-local-directories]') && document.activeElement === trigger();
  const open = async () => {
    click(trigger());
    return wait(() => {
      const d = dialog();
      return d && d.getAttribute('aria-busy') === 'false' &&
        getComputedStyle(d).opacity === '1' && button('确认', d) && !button('确认', d).disabled && d;
    }, 'settled directory selection');
  };
  const confirm = async () => {
    click(button('确认', dialog()));
    await wait(focusReturned, 'return to directory trigger');
  };
  const visible = (count) => [...document.querySelectorAll('.local-tracks tbody tr')].length === count;
  try {
    await wait(() => one('.welcome') && !one('.main-content .spinner'), 'guest bootstrap');
    const account = await invoke('account_snapshot');
    check(account.status !== 'authenticated', 'fresh guest only');
    const { epoch } = account;
    click(button('本地音乐'));
    await wait(() => one('.local-page[data-local-ready="true"]'), 'local library');
    const initial = await open();
    check((await read()).total === 0 && (await dirs()).entries.length === 0, 'fresh empty library');
    click(button('添加文件夹', initial));
    await capture('picker_requested');
    await wait(() => box() && !box().disabled, 'native chosen directory staged');
    check(!box().checked && !(await dirs()).entries[0].checked && (await read()).total === 0,
      'new directory starts unchecked without scanning');
    click(box());
    check(box().checked, 'draft is checked in current modal');
    click(button('关闭目录选择', dialog()));
    await wait(focusReturned, 'cancel restores exact focus');
    await open();
    check(!box().checked && (await read()).total === 0, 'cancel preserves registration only');
    check(inlineOption(), 'checkbox and wrapped path share one row');
    await capture('unchecked_after_cancel', { inlineOption: true });
    click(box());
    await confirm();
    await wait(async () => visible(3) && (await read()).roots[0]?.status === 'ready', 'actual native scan');
    await capture('checked');

    click(one('[aria-label="播放本地歌曲 雨声"]'));
    await wait(async () => {
      const v = await invoke('player_snapshot', { epoch });
      return v.state === 'playing' && v.track?.name === '雨声' && v.position > 0.25;
    }, 'ordinary local native clock');
    click(one('.playerbar [aria-label="暂停"]'));
    const held = await wait(async () => {
      const v = await invoke('player_snapshot', { epoch });
      return v.state === 'paused' && v.position > 0.25 && v;
    }, 'same paused native entry');
    const stable = async () => {
      const v = await invoke('player_snapshot', { epoch });
      return v.state === 'paused' && v.entryId === held.entryId && v.playId === held.playId &&
        v.queueRevision === held.queueRevision && Math.abs(v.position - held.position) < .05;
    };
    one('.local-roots').open = true;
    click(button('暂停扫描'));
    await wait(() => button('恢复扫描'), 'ordinary pause applied');
    await open();
    check(box().checked && dialog().textContent.includes('已暂停自动扫描') && visible(3),
      'pause does not deselect or hide the directory');
    check(inlineOption(), 'paused annotation stays beside the checkbox');
    await capture('paused_root', { stable: await stable(), inlineOption: true });
    click(box());
    check(visible(3) && (await dirs()).entries[0].checked, 'draft removal has not committed');
    await confirm();
    await wait(async () => visible(0) && (await read()).total === 0, 'confirmed directory excluded');
    check(await stable(), 'deselection does not restart or close paused playback');
    await capture('deselected', { stable: true });
    await open();
    check(!box().checked && !dialog().textContent.includes('已暂停自动扫描'), 'deselected distinct from paused');
    click(box());
    await confirm();
    await wait(async () => visible(3) && (await read()).roots[0]?.status === 'ready', 'directory restored by confirm');
    check(await stable(), 'restoring directory preserves paused native entry and clock');
    await capture('rechecked', { stable: true });
    await emit('complete', { ok: true, stages: count, realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', { ok: false, step, error: error?.message ?? String(error), stack: error?.stack,
      body: document.body.innerText.slice(-5500) });
  }
})();
