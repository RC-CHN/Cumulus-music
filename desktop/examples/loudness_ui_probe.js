// Settings controls and native processing are both checked by the Rust host.
(async () => {
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const one = selector => document.querySelector(selector);
  const all = selector => [...document.querySelectorAll(selector)];
  const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
  const emit = (stage, rest = {}) => invoke('plugin:event|emit', {event: 'loudness-proof', payload: {stage, ...rest}});
  const assert = (value, message) => { if (!value) throw Error(message); };
  const wait = async (predicate, label) => { for (let i = 0; i < 200; i++) { if (predicate()) return; await sleep(100); } throw Error(label); };
  const button = text => all('button').find(node => node.textContent.trim() === text);
  const click = async node => { assert(node, 'missing control'); await wait(() => node.isConnected && !node.disabled, 'disabled control'); node.click(); await sleep(150); };
  const section = () => one('.loudness-settings');
  const checkbox = () => one('[name="loudness-enabled"]');
  const phase = () => one('.loudness-status')?.dataset.phase;
  const settled = () => section()?.getAttribute('aria-busy') === 'false' && phase() !== 'pending';
  let epoch = '', step = 'bootstrap', count = 0;
  async function settings() { await click(one('[aria-label="打开设置"]')); await wait(() => section() && settled(), 'settings ready'); section().scrollIntoView({block:'center'}); await sleep(400); }
  async function toggle() { const revision = section().dataset.revision; checkbox().focus(); await click(checkbox()); await wait(() => settled() && section().dataset.revision !== revision, 'preference committed'); assert(document.activeElement === checkbox(), 'toggle lost keyboard focus'); }
  async function play(index) {
    await click(button('本地音乐'));
    await wait(() => all('[aria-label^="播放本地歌曲"]').length === 3, 'local rows');
    const title = ['01 有音量信息', '02 无音量信息', '03 异常音量信息'][index];
    const target = all('[aria-label^="播放本地歌曲"]').find(node => node.getAttribute('aria-label').includes(title));
    await click(target);
    await wait(() => one('.playerbar [aria-label="暂停"]') && Number(one('[aria-label="播放进度"]')?.value) > .2, 'native playback progress');
    await settings();
  }
  async function capture(name, status, enabled) {
    await wait(() => settled() && phase() === status, `status ${status}, found ${phase()}`);
    section().scrollIntoView({block:'center'}); await sleep(450);
    const r = section().getBoundingClientRect(), bounds = one('.main-content').getBoundingClientRect();
    const controls = [...section().querySelectorAll('button,input')];
    const contained = document.documentElement.scrollWidth <= innerWidth && section().scrollWidth <= section().clientWidth + 1 && r.top >= bounds.top - 1 && r.bottom <= bounds.bottom + 1;
    const hittable = controls.every(node => { const box = node.getBoundingClientRect(); const hit = document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2); return hit === node || node.contains(hit); });
    assert(contained, 'settings section overflow'); assert(hittable, 'settings control covered');
    if (checkbox()) assert(checkbox().checked === enabled, 'checkbox state disagrees');
    const stage = `loudness_${String(++count).padStart(2,'0')}_${name}`;
    window.__loudnessAck = '';
    await emit(stage, {epoch, expect:{status, enabled}, width:innerWidth, height:innerHeight, theme:document.documentElement.dataset.theme, contained, hittable});
    await wait(() => window.__loudnessAck === stage, 'native acknowledgment'); await sleep(950);
  }
  async function layout(width, height, theme) {
    await emit('resize', {width, height}); await wait(() => innerWidth === width && innerHeight === height, 'resize');
    await click(one('[aria-label="外观主题"]')); await wait(() => one('.action-menu'), 'theme menu');
    await click(button(theme === 'light' ? '浅色' : '深色'));
    await wait(() => document.documentElement.dataset.theme === theme && !one('.action-menu'), 'theme applied');
  }
  try {
    await wait(() => one('.welcome'), 'guest ready'); epoch = (await invoke('account_snapshot')).epoch;
    await settings();
    const mode = window.__loudnessCase;
    if (mode === 'damaged') {
      step = 'repair'; await capture('damaged', 'failed', false);
      await click(button('恢复默认音量均衡设置')); await capture('repaired', 'off', false);
      await toggle(); await play(0); await capture('repaired_processing', 'processing', true);
    } else if (mode === 'reopen') {
      step = 'reopen'; await wait(() => one('.playerbar [aria-label="播放"]'), 'restore paused');
      assert(checkbox().checked, 'saved enabled preference lost');
      await click(one('.playerbar [aria-label="播放"]'));
      await capture('reopened_processing', 'processing', true);
      await toggle(); await capture('reopened_off', 'off', false);
    } else {
      await capture('initial', 'off', false); await toggle(); await capture('enabled_idle', 'no_track', true);
      step = 'play'; await play(0);
      if (mode === 'missing') {
        await capture('missing_filter', 'failed', true);
        await toggle(); await capture('missing_disabled', 'off', false);
        assert(one('.playerbar [aria-label="暂停"]'), 'ordinary playback stopped');
      } else {
        for (const [width,height,theme] of [[1120,760,'light'],[1120,760,'dark'],[760,540,'dark'],[760,540,'light']]) {
          step = `layout ${width} ${theme}`; await layout(width,height,theme); await capture(`processing_${width}_${theme}`, 'processing', true);
        }
        step = 'bypass'; await toggle(); await capture('off_while_playing', 'off', false); await toggle();
        step = 'no gain'; await play(1); await capture('no_gain', 'no_gain', true);
        step = 'invalid gain'; await play(2); await capture('invalid_gain', 'invalid_gain', true);
        step = 'save'; await play(0); await wait(() => phase() === 'processing', 'final processing');
        const seek = one('[aria-label="播放进度"]'); seek.value = '12'; seek.dispatchEvent(new Event('change', {bubbles:true}));
        await wait(() => Number(seek.value) >= 11 && !one('.playerbar [aria-label="暂停"]')?.disabled, 'seek');
        await click(one('.playerbar [aria-label="暂停"]')); await wait(() => one('.playerbar [aria-label="播放"]'), 'pause');
        await capture('saved_paused', 'processing', true);
      }
    }
    await emit('complete', {ok:true, mode, stageCount:count, realMusicRequests:0, physicalAudioOutputVerified:false, physicalDisplayVerified:false, actualDecoder:true, audioOutput:'null'});
  } catch (error) { await emit('failed', {ok:false, step, status:phase(), localTitles:all('[aria-label^="播放本地歌曲"]').map(node => node.getAttribute('aria-label')), error:error instanceof Error ? error.message : 'loudness proof failure'}); }
})();
