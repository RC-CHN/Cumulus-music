(async () => {
  const one = s => document.querySelector(s), all = s => [...document.querySelectorAll(s)];
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const assert = (ok, why) => { if (!ok) throw Error(why); };
  const wait = async (test, why) => { for (let i = 0; i < 150; i++) { if (test()) return; await sleep(80); } throw Error(why); };
  const emit = (stage, rest = {}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {event: 'ux-proof', payload: {stage, ...rest}});
  const click = node => { assert(node && !node.disabled, 'unavailable action'); node.focus({preventScroll: true}); node.click(); };
  const state = () => one('.playerbar')?.dataset.state;
  const focus = node => { node.setAttribute('tabindex', '-1'); node.focus({preventScroll: true}); return node; };
  let stageCount = 0;
  async function capture(name) {
    assert(scrollY === 0 && document.documentElement.scrollWidth <= innerWidth, 'desktop frame overflow');
    await emit(`ux_${String(++stageCount).padStart(2, '0')}_${name}`, {playerState: state(), realMusicRequests: 0});
    await sleep(700);
  }
  function space(target, options = {}, handled = false) {
    const event = new KeyboardEvent('keydown', {key: ' ', code: 'Space', bubbles: true, cancelable: true, ...options});
    if (handled) event.preventDefault();
    target.dispatchEvent(event);
    return event;
  }
  async function ignored(target, options = {}, handled = false) {
    const event = space(target, options, handled);
    await sleep(400);
    assert(state() === 'paused', 'typing or modal space changed actual playback');
    if (!handled) assert(!event.defaultPrevented, 'player consumed a protected key');
  }
  try {
    await wait(() => one('.track-row'), 'daily songs');
    one('.track-row').dispatchEvent(new MouseEvent('dblclick', {bubbles: true}));
    await wait(() => state() === 'playing' && Number(one('.playerbar').dataset.position) > .15, 'native audio');
    click(one('[aria-label="暂停"]')); await wait(() => state() === 'paused', 'pause');
    await capture('initial_paused');
    click(one('[aria-label="均衡器"]')); await wait(() => one('.equalizer-dialog[open]'), 'modal');
    await ignored(focus(one('.equalizer-dialog h2'))); await capture('modal_space_ignored');
    click(one('[aria-label="关闭均衡器"]')); await wait(() => !one('.equalizer-dialog'), 'modal closed');
    const page = focus(one('.page-view h1'));
    for (const options of [{repeat: true}, {isComposing: true}, {shiftKey: true}, {ctrlKey: true}, {altKey: true}, {metaKey: true}]) await ignored(page, options);
    await ignored(page, {}, true); await capture('handled_and_modified_ignored');
    const input = one('.search-box input') || one('header input'); assert(input, 'search input'); input.focus(); await ignored(input); await capture('input_space_ignored');
    // Browser editability includes plaintext-only and inheritance, not only
    // the literal contenteditable="true" selector. Synthetic editable fixture.
    const editor = document.createElement('div'); editor.contentEditable = 'plaintext-only';
    editor.textContent = '合成输入区域'; one('.page-view').prepend(editor); editor.focus();
    await ignored(editor); editor.remove(); await capture('editable_space_ignored');
    const target = focus(one('.page-view h1'));
    assert(space(target).defaultPrevented, 'page shortcut not consumed'); await wait(() => state() === 'playing', 'page space play'); await capture('page_space_play');
    space(target); await wait(() => state() === 'paused', 'page space pause'); await capture('page_space_pause');
    await emit('complete', {ok: true, stageCount, realMusicRequests: 0, physicalAudioOutputVerified: false});
  } catch (error) { await emit('failed', {ok: false, error: String(error), stageCount, playerState: state()}); }
})();
