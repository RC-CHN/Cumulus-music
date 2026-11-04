// Real membership UI, native playback and reversible likes on closed synthetic data.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, payload = {}) => invoke('plugin:event|emit', {event:'ux-proof', payload:{stage,...payload}});
  async function wait(check, message) {
    for (let i = 0; i < 180; i++) {
      const found = await check();
      if (found) return found;
      await sleep(50);
    }
    throw Error(message);
  }
  const card = () => one('.sign-card');
  const dialog = () => one('.sign-dialog[open]');
  const past = () => one('[aria-label="查看30日乐签"]');
  const close = () => one('.sign-dialog[open] [aria-label="关闭乐签"]');
  const message = () => card()?.querySelector('.sign-notice[role="status"]');
  const member = () => [...document.querySelectorAll('.sidebar button:not(:disabled)')].find((node) => node.textContent.trim() === '会员中心');
  const portalTrigger = () => one('.member-portal-trigger');
  const portalDialog = () => one('.member-portal-dialog[open]');
  async function portal(stage, cancel, trigger = portalTrigger(), dynamic = false) {
    trigger.focus();
    trigger.click();
    const qr = await wait(() => portalDialog()?.querySelector('svg[role="img"]'), 'public member portal QR');
    const modal = portalDialog();
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await Promise.all(modal.getAnimations().map(animation => animation.finished.catch(() => {})));
    const rect = modal.getBoundingClientRect();
    const width = dynamic ? 180 : 155;
    const tip = dynamic ? '请打开 网易云音乐APP 扫码' : '使用云音乐 App 扫描二维码';
    if (dynamic !== modal.classList.contains('member-offer-qr-dialog') ||
        dynamic === !!modal.querySelector('h2') ||
        qr.getBoundingClientRect().width !== width ||
        (dynamic && (rect.width !== 376 || rect.height !== Math.min(520, innerHeight - 32))))
      throw Error('fixed membership and dynamic offer QR must keep their distinct official layout');
    if (!modal.contains(document.activeElement) || rect.x < 0 || rect.y < 0 ||
        rect.right > innerWidth || rect.bottom > innerHeight || qr.querySelector('path').getAttribute('d').length < 1000 ||
        !modal.textContent.includes(tip) || modal.querySelector('a, iframe, img') ||
        modal.textContent.includes('://'))
      throw Error('public handoff must fit, trap focus and render only the native QR matrix');
    await emit(stage, {qrBounds:qr.getBoundingClientRect().toJSON()});
    if (cancel) modal.dispatchEvent(new Event('cancel', {cancelable:true}));
    else modal.querySelector('[aria-label="关闭会员中心二维码"]').click();
    await wait(() => !portalDialog() && document.activeElement === trigger, 'member portal restores exact trigger focus');
  }
  let step = 'entry';
  try {
    (await wait(member, 'member sidebar ready')).click();
    await wait(() => message()?.textContent.includes('暂时无法读取乐签'), 'initial failure is separate from member content');
    await emit('ux_01_member_sign_initial_failure');
    message().querySelector('button').click();
    await wait(() => past() && one('.sign-today') && card().querySelectorAll('.sign-days li').length === 3, 'card retry displays all official slots');
    if (card().querySelector('.sign-texture') || card().querySelectorAll('.sign-days button').length !== 2)
      throw Error('unsigned stamp is interactive or unsafe texture leaked');
    await emit('ux_02_member_sign_card');
    step = 'portal-light';
    await portal('ux_03_member_portal_light', false);

    step = 'past';
    past().focus();
    past().click();
    await wait(() => dialog()?.querySelector('[role="alert"]'), 'detail failure is visible within modal');
    await emit('ux_04_member_sign_detail_retry');
    dialog().querySelector('[role="alert"] button').click();
    await wait(() => dialog()?.querySelector('h3')?.textContent === '合成历史乐签歌曲', 'historical detail retry');
    if (!dialog().textContent.includes('合成歌手') || !dialog().textContent.includes('保持好奇') ||
      !dialog().textContent.includes('来自合成评论者') || !dialog().textContent.includes('累计打卡 5') ||
      !dialog().textContent.includes('再打卡 2') || !dialog().textContent.includes('1天黑胶体验') ||
      !dialog().querySelector('[aria-label="乐签会员中心二维码"]') || dialog().querySelector('img'))
      throw Error('recorded detail projection does not match official shape');
    await emit('ux_05_member_sign_detail');
    const {epoch} = await invoke('account_snapshot');
    (await wait(() => dialog()?.querySelector('[aria-label="播放 合成历史乐签歌曲"]:not(:disabled)'), 'historical song can play')).click();
    const playback = await wait(async () => {
      const value = await invoke('player_snapshot', {epoch});
      return value.state === 'playing' && value.track?.id === '2' && value.position > 0.15 && value;
    }, 'record song reaches native output');
    if (playback.queueLen !== 1) throw Error('record playback selects wrong queue');
    step = 'sign-song-actions';
    const control = (label) => dialog()?.querySelector(`[aria-label="${label} 合成历史乐签歌曲"]`);
    const heart = await wait(() => control('喜欢'), 'sign song can be liked');
    heart.focus();
    heart.click();
    await wait(() => control('取消喜欢') === heart && heart.getAttribute('aria-pressed') === 'true',
      'liked membership is read back on the same control');
    const pause = await wait(() => control('暂停'), 'sign player shows current track pause');
    pause.focus();
    pause.click();
    await wait(async () => {
      const value = await invoke('player_snapshot', {epoch});
      return value.state === 'paused' && value.playId === playback.playId &&
        value.queueRevision === playback.queueRevision && control('继续播放') === pause;
    }, 'pause keeps the same native song and queue');
    await emit('ux_10_member_sign_song_actions');
    control('继续播放').click();
    await wait(async () => {
      const value = await invoke('player_snapshot', {epoch});
      return value.state === 'playing' && value.playId === playback.playId &&
        value.queueRevision === playback.queueRevision && control('暂停') === pause;
    }, 'sign resume keeps the same native song and queue');
    if (document.activeElement !== pause) throw Error('song playback stole focus');
    control('取消喜欢').click();
    await wait(() => control('喜欢') === heart && heart.getAttribute('aria-pressed') === 'false',
      'unlike confirms and restores the original state');
    close().click();
    await wait(() => !dialog() && document.activeElement === past(), 'native close returns focus to record');
    one('.playerbar [aria-label="停止播放"]').click();

    step = 'late';
    await emit('resize', {width:760,height:540});
    await wait(() => innerWidth === 760 && innerHeight === 540, 'compact membership window');
    one('[aria-label="外观主题"]').click();
    (await wait(() => [...document.querySelectorAll('.action-menu button')].find((node) => node.textContent.trim() === '深色'), 'dark mode choice')).click();
    await wait(() => document.documentElement.dataset.theme === 'dark', 'dark theme');
    step = 'portal-dark';
    const offer = await wait(() => {
      const node = one('.member-card-offer');
      return node?.textContent.trim() === '查看合成会员优惠' && node;
    }, 'official delivery label appears on the separate offer');
    const {value: observedOffer} = await invoke('member_card_offer', {epoch, refresh:false});
    if (observedOffer.mode !== 'qr') throw Error('expected a native mode-three receipt');
    await portal('ux_06_member_offer_qr_dark', true, offer, true);
    step = 'late';
    past().click();
    await wait(() => dialog()?.querySelector('[role="status"]'), 'held native detail is pending');
    await sleep(100);
    dialog().dispatchEvent(new Event('cancel', {cancelable:true}));
    one('.sign-today').click();
    await wait(() => dialog()?.querySelector('h3')?.textContent === '合成今日乐签歌曲', 'today resolves through its own record');
    await sleep(850);
    if (dialog()?.querySelector('h3')?.textContent !== '合成今日乐签歌曲') throw Error('late historical result overwrites today');
    if (!/每月打卡得\s+VIP\s+体验/.test(dialog().textContent) || dialog().textContent.includes('PRIVATE'))
      throw Error('claimed reward branch does not match the official footer');
    const bounds = dialog().getBoundingClientRect();
    if (bounds.x < 0 || bounds.y < 0 || bounds.right > innerWidth || bounds.bottom > innerHeight)
      throw Error('compact dark modal is clipped');
    await emit('ux_07_member_sign_today');
    close().click();

    step = 'refresh';
    one('[aria-label="刷新会员内容"]').click();
    await wait(() => message()?.textContent.includes('正在显示上次读取'), 'refresh failure preserves card');
    await wait(() => one('.member-card-offer')?.textContent.includes('在 App 开通 VIP'),
      'protected redirect falls back to the official App guide');
    if (document.body.textContent.includes('不能显示的合成优惠') ||
        one('.member-card-offer').getAttribute('aria-haspopup') !== 'dialog')
      throw Error('unsafe offer was promoted or made actionable');
    const rejected = await invoke('member_card_offer_qr', {epoch, revision:observedOffer.revision}).then(
      () => false, (error) => error.code === 'stale',
    );
    if (!rejected) throw Error('a replaced mode-three receipt still renders a QR');
    if (!past() || !one('.sign-today')) throw Error('good card disappeared on refresh failure');
    await emit('ux_08_member_sign_cached');
    message().querySelector('button').click();
    await wait(() => card()?.textContent.includes('今日未打卡'), 'updated card shows unsigned today');
    if (one('.sign-today') || card().querySelector('[aria-label="查看今天乐签"]')) throw Error('unsigned today can start a mutation');
    await emit('ux_09_member_sign_unsigned');
    await emit('complete', {ok:true, stageCount:10, historyPlayback:playback.track.id, nativeModalFocus:true, noLateDetail:true, memberPortalQr:true, memberOfferVerified:true, memberOfferQr:true, signSongActions:true});
  } catch (error) {
    await emit('failed', {step, message:error.message, card:card()?.textContent, dialog:dialog()?.textContent, focus:document.activeElement?.outerHTML.slice(0,260)});
  }
})();
