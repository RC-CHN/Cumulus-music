(async () => {
  const sleep = ms => new Promise(r => setTimeout(r, ms));
  const one = s => document.querySelector(s), all = s => [...document.querySelectorAll(s)];
  const assert = (value, why) => { if (!value) throw Error(why); };
  const wait = async (test, why) => { for (let i=0;i<150;i++) { if (test()) return; await sleep(80); } throw Error(why); };
  const button = text => all('button').find(n => n.textContent.trim() === text);
  const click = n => { assert(n && !n.disabled, 'missing/disabled action'); n.focus({preventScroll:true}); n.click(); };
  const emit = (stage, rest={}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {event:'ux-proof', payload:{stage,...rest}});
  const hook = window.__motionProof;
  const toggle = () => one('[name="disable-animation"]');
  const frame = () => new Promise(r => requestAnimationFrame(r));
  const zero = value => value.split(',').every(s => parseFloat(s) === 0);
  const reduced = () => document.documentElement.dataset.motion === 'reduced';
  let stageCount=0;
  const checks=[];
  function noMotion(label) {
    assert(reduced(), `${label}: policy is not reduced`);
    assert(document.getAnimations().every(a => a.playState !== 'running'), `${label}: running animation`);
    for (const n of all('.page-view, .nav-item, dialog[open], .action-menu, .queue-panel'))
      assert(zero(getComputedStyle(n).transitionDuration) && getComputedStyle(n).animationName === 'none', `${label}: CSS motion remains`);
    checks.push(label);
  }
  async function capture(label) {
    await sleep(300);
    assert(document.documentElement.scrollWidth <= innerWidth, 'horizontal overflow');
    await emit(`ux_${String(++stageCount).padStart(2,'0')}_${label}`, {motion:document.documentElement.dataset.motion, theme:document.documentElement.dataset.theme, width:innerWidth,height:innerHeight,overflow:false,checks:[...checks]});
    await sleep(600);
  }
  async function settings() {
    click(one('[aria-label="打开设置"]')); await wait(toggle, 'settings'); await sleep(300);
  }
  async function setDisabled(value) {
    const changed=toggle().checked!==value; if(changed) click(toggle());
    await frame(); await frame();
    assert(toggle().checked===value,'checkbox did not retain selection');
    assert(!changed || document.activeElement===toggle(),'toggle focus lost');
  }
  async function theme(value) {
    click(one(`[name="settings-theme"][value="${value}"]`)); await frame(); await frame();
    assert(document.documentElement.dataset.theme===value,'theme not applied');
  }
  async function page(name, selector) { click(button(name)); await wait(()=>one(selector),name); await sleep(300); }
  try {
    await wait(()=>one('.track-row'),'initial daily songs'); await sleep(500);
    if(localStorage.getItem('cumulus.motion.proof.done')) {
      assert(reduced() && localStorage.getItem('cumulus.motion.v1')==='disabled','motion preference lost across processes');
      noMotion('restart'); await settings(); assert(toggle().checked,'restart checkbox'); await capture('reopened_disabled');
      await setDisabled(false); assert(!reduced(),'enable after reopen'); await capture('reopened_enabled');
      await emit('complete',{ok:true,stageCount,checks,reopened:true,realMusicRequests:0,systemPreferenceEventInjected:true}); return;
    }
    assert(!reduced(),'fresh profile should keep animations');
    await settings(); assert(!toggle().checked,'fresh disabled checkbox');
    assert(!zero(getComputedStyle(one('.nav-item')).transitionDuration),'default navigation motion missing'); await capture('default_enabled');
    await setDisabled(true); noMotion('disabled'); await capture('disabled_light');
    let count=hook.animations.length; click(one('[aria-label="外观主题"]')); await wait(()=>one('.action-menu'),'theme menu'); await frame(); noMotion('menu'); await capture('menu_no_motion'); click(button('深色')); await wait(()=>!one('.action-menu') && document.documentElement.dataset.theme==='dark','theme menu selection'); assert(hook.animations.length===count,'disabled theme created WAAPI fade'); noMotion('dark'); await capture('disabled_dark');
    click(one('[aria-label="播放队列"]')); await wait(()=>one('.queue-panel'),'queue'); await frame(); noMotion('queue'); await capture('queue_no_motion');
    click(one('[aria-label="关闭播放队列"]')); await wait(()=>!one('.queue-panel'),'queue closed');
    const eq=one('[aria-label="均衡器"]'); click(eq); await wait(()=>one('.equalizer-dialog[open]'),'equalizer'); await frame(); noMotion('modal'); await capture('modal_no_motion');
    click(one('[aria-label="关闭均衡器"]')); await wait(()=>!one('.equalizer-dialog'),'equalizer closed'); await sleep(100); assert(document.activeElement===eq,'disabled modal lost focus');
    await page('我的歌单','.playlist-card'); click(all('.playlist-card')[1]); await wait(()=>one('.playlist-edit'),'playlist'); click(one('.playlist-edit')); await wait(()=>one('.playlist-dialog[open]'),'editor');
    await frame(); noMotion('editor'); assert(document.activeElement===one('[aria-label="歌单名称"]'),'editor focus'); await capture('editor_no_motion');
    click(one('[aria-label="关闭歌单操作"]')); await wait(()=>!one('.playlist-dialog'),'editor closed');
    await page('每日推荐','.track-row'); one('.track-row').dispatchEvent(new MouseEvent('dblclick',{bubbles:true}));
    await wait(()=>one('[aria-label="暂停"]') && Number(one('.playerbar').dataset.position)>0.15,'native playing progress');
    click(one('[aria-label="暂停"]')); await wait(()=>one('[aria-label="播放"]'),'paused');
    click(one('[aria-label="歌词"]')); await wait(()=>one('.lyric-line.current'),'actual lyrics'); await frame();
    assert(hook.scrolls.some(s=>String(s.className).includes('lyric-line')&&s.behavior==='instant'),'disabled lyrics did not use instant scroll'); noMotion('lyrics'); await capture('lyrics_instant');
    hook.system(true); await frame(); click(one('[aria-label="收起播放详情"]')); await wait(()=>!one('.lyrics-panel'),'lyrics closed'); await settings();
    await setDisabled(false); assert(reduced(),'explicit enable overrode system accessibility preference'); assert(one('.settings-page').textContent.includes('系统已开启减少动态效果'),'system explanation missing'); noMotion('system'); await capture('system_reduced');
    hook.system(false); await frame(); assert(!reduced(),'system disable did not re-enable motion'); await capture('system_restored');
    const scrollCount=hook.scrolls.length; click(one('[aria-label="歌词"]')); await wait(()=>one('.lyric-line.current'),'enabled lyrics'); await sleep(350); assert(hook.scrolls.slice(scrollCount).some(s=>String(s.className).includes('lyric-line')&&s.behavior==='smooth'),'enabled lyrics did not restore smooth scrolling'); await capture('lyrics_smooth'); click(one('[aria-label="收起播放详情"]')); await wait(()=>!one('.lyrics-panel'),'enabled lyrics closed'); await sleep(250);
    await theme('light');
    // Begin a real theme animation and disable it during its lifetime.
    click(one('[name="settings-theme"][value="dark"]'));
    const fade=hook.animations.at(-1); assert(fade?.playState==='running','theme fade not started');
    click(toggle()); await frame(); assert(fade.playState==='idle','active WAAPI animation not cancelled'); noMotion('cancelled_theme'); await capture('theme_cancelled');
    await setDisabled(false); hook.failWrite(true); await setDisabled(true); assert(reduced(),'failed persistence did not apply current choice');
    assert(localStorage.getItem('cumulus.motion.v1')==='system','failed persistence changed stored value');
    assert(all('[role="alert"]').some(n=>n.textContent.includes('动画设置已应用')),'save failure invisible'); await capture('save_failed');
    hook.failWrite(false); await setDisabled(false); await setDisabled(true); assert(localStorage.getItem('cumulus.motion.v1')==='disabled','retry did not persist'); await capture('save_recovered');
    await emit('resize',{width:760,height:540}); await wait(()=>innerWidth===760,'small window');
    toggle().scrollIntoView({block:'center'}); await capture('small_dark'); await theme('light'); toggle().scrollIntoView({block:'center'}); await capture('small_light');
    localStorage.setItem('cumulus.motion.proof.done','1');
    await emit('complete',{ok:true,stageCount,checks,realMusicRequests:0,systemPreferenceEventInjected:true,lyricScrollVerified:true,lyricSmoothRestored:true,themeCancellationVerified:true,persistenceFailureVerified:true});
  } catch(error) {await emit('failed',{ok:false,error:String(error),stageCount,checks,scrolls:hook?.scrolls,motion:document.documentElement.dataset.motion});}
})();
