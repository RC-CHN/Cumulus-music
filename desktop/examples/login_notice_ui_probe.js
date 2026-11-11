// Production DOM actions and IPC; native fixtures supply HTTP/browser/CAPTCHA.
(async () => {
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const assert = (condition, message) => { if (!condition) throw Error(message); };
  const all = selector => [...document.querySelectorAll(selector)].filter(node =>
    !node.closest('.tab-leave-active,.modal-leave-active'));
  const one = selector => all(selector)[0];
  const button = (label, root = document) => [...root.querySelectorAll('button')].find(node =>
    !node.closest('.tab-leave-active,.modal-leave-active') &&
    (node.textContent.trim() === label || node.getAttribute('aria-label') === label));
  const emit = (stage, extra = {}) => invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...extra } });
  const wait = async (test, label) => {
    const deadline = performance.now() + 24000;
    while (performance.now() < deadline) { if (await test()) return; await sleep(40); }
    throw Error(label);
  };
  async function settle() {
    await wait(async () => {
      await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      return !document.getAnimations().some(animation =>
        Number.isFinite(animation.effect?.getComputedTiming().endTime) &&
        (animation.pending || animation.playState === 'running'));
    }, 'finite animations settled');
  }
  function visible(node) {
    if (!node?.isConnected || node.closest('[inert]')) return false;
    const rect = node.getBoundingClientRect(), style = getComputedStyle(node);
    return rect.width > 0 && rect.height > 0 && style.visibility !== 'hidden' &&
      rect.top >= -1 && rect.left >= -1 && rect.right <= innerWidth + 1 && rect.bottom <= innerHeight + 1;
  }
  function hit(node) {
    const rect = node.getBoundingClientRect();
    const target = document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2);
    return target === node || node.contains(target);
  }
  async function click(node) {
    assert(node?.isConnected, 'control exists');
    await wait(() => !node.disabled && !node.closest('[inert]'), 'control enabled');
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    assert(visible(node) && hit(node), 'visible control hit testing: ' + (node.getAttribute('aria-label') || node.textContent.trim()));
    node.click();
    await sleep(0);
    await settle();
  }
  async function fill(label, value) {
    const node = one(`[aria-label="${label}"]`);
    assert(node && !node.disabled, 'editable ' + label);
    node.focus(); node.value = value; node.dispatchEvent(new Event('input', { bubbles: true }));
    await sleep(0);
  }
  let serial = 0, count = 0, last = 'boot';
  async function mode(value) {
    const id = ++serial;
    await invoke('plugin:event|emit', { event: 'login-notice-fixture', payload: { id, mode: value } });
    await wait(() => window.__loginNoticeFixture?.id === id, 'native fixture acknowledgement');
    return window.__loginNoticeFixture.fixture;
  }
  async function nativeWait(test, label) {
    let snapshot;
    await wait(async () => {
      snapshot = await mode('inspect');
      if (test(snapshot)) return true;
      await sleep(100);
      return false;
    }, label);
    return snapshot;
  }
  async function theme(value) {
    localStorage.setItem('cumulus.appearance.v1', value);
    dispatchEvent(new StorageEvent('storage', { key: 'cumulus.appearance.v1', newValue: value }));
    await wait(() => document.documentElement.dataset.theme === value, 'theme applied');
    await settle();
  }
  async function resize(width, height) {
    await emit('resize', { width, height, scale: devicePixelRatio });
    await wait(() => innerWidth === width && innerHeight === height, 'CSS viewport size');
    await settle();
  }
  const ready = () => one('.account-button') && !one('.account-button').disabled;
  const status = () => one('[data-login-status]')?.dataset.loginStatus;
  const qrStatus = () => one('[data-security-qr-status]')?.dataset.securityQrStatus;
  async function open() {
    // A leaving native <dialog> still occupies the top layer even when the
    // scenario selector filters its exiting animation. Wait for real removal.
    await wait(() => !document.querySelector('dialog[open]'), 'previous modal fully closed');
    await click(one('.account-button'));
    await wait(() => one('.login-methods') || one('.account-add'), 'account/login dialog opened');
    if (one('.account-add')) {
      await click(one('.account-add'));
      await wait(() => one('.login-methods'), 'new candidate login dialog');
    }
    await settle();
  }
  async function returnPhone() {
    await click(button('返回手机登录'));
    await wait(() => !one('.login-notice,.penalty-verification,.security-qr-verification') &&
      one('[aria-label="登录手机号"]') && status() === 'ready' && !one('.tab-leave-active'), 'phone return');
    assert(!one('[aria-label="登录密码"]'), 'secret discarded on phone return');
    await settle();
  }
  async function submitCase(value) {
    if (!one('dialog.login-dialog[open]')) await open();
    if (one('.login-notice,.penalty-verification,.security-qr-verification')) await returnPhone();
    await mode(value);
    await click(button('邮箱', one('.login-methods')));
    await wait(() => one('[aria-label="登录邮箱"]') && status() === 'ready' &&
      !one('.tab-leave-active'), 'email input ready');
    await fill('登录邮箱', 'notice@example.test');
    await fill('登录密码', 'fixture-password');
    if (!one('[aria-label="同意登录协议"]').checked) await click(one('[aria-label="同意登录协议"]'));
    await click(one('.login-submit'));
  }
  async function notice(kind) {
    await wait(() => one(`.login-notice[data-notice-kind="${kind}"]`), kind + ' notice');
    await settle();
    assert(!one('.login-methods') && !one('[aria-label="登录密码"]'), 'dedicated notice with no retained password');
  }
  async function penalty() {
    await wait(() => one('.penalty-verification'), 'native penalty view');
    await settle();
    assert(!one('[aria-label="登录密码"]'), 'penalty password input removed');
  }
  function assertPrivateProjection() {
    const text = document.body.textContent;
    assert(!text.includes('NATIVE_NOTICE_PRIVATE') && !text.includes('NATIVE_PENALTY_PRIVATE') &&
      !text.includes('NATIVE_QR_PRIVATE') && !text.includes('synthetic-native-only-'), 'native secrets absent from DOM text');
    assert(!one('.login-notice-content strong'), 'server notice markup remains plain text');
    assert(!one('.login-notice-content a'), 'server notice cannot create authorization links');
  }
  async function stage(name, extra = {}) {
    last = name;
    await settle();
    assertPrivateProjection();
    const dialog = one('dialog[open]'), focus = document.activeElement;
    const overflow = document.documentElement.scrollWidth > innerWidth + 1 ||
      (!!dialog && (dialog.scrollWidth > dialog.clientWidth + 1 || !visible(dialog)));
    assert(!overflow, 'dialog and viewport bounds');
    const focused = document.hasFocus();
    const focusValid = !!focus?.isConnected && focus !== document.body &&
      (!dialog || dialog.contains(focus)) && !focus.closest('[inert]');
    assert(focused && focusValid, 'live modal/page focus');
    const controls = dialog ? [...dialog.querySelectorAll('button,input,select')]
      .filter(node => !node.disabled && visible(node)).map(node => ({
        label: node.getAttribute('aria-label') || node.textContent.trim(), hit: hit(node),
      })) : [];
    assert(controls.every(control => control.hit), 'visible controls are not occluded');
    const risk = one('.login-notice'), qr = one('.security-qr-verification');
    await emit(`ux_${String(++count).padStart(2, '0')}_${name}`, {
      ...extra,
      width: innerWidth, height: innerHeight, ratio: devicePixelRatio,
      screenX: window.screenX, screenY: window.screenY,
      theme: document.documentElement.dataset.theme, focused, focusValid, focusVisible: visible(focus), overflow,
      controls, status: status() ?? null, noticeKind: risk?.dataset.noticeKind ?? null,
      noticeTitle: one('.login-notice-title')?.textContent.trim() ?? null,
      noticeParagraphs: all('.login-notice-content p').map(node => node.textContent),
      securityQrStatus: qr?.dataset.securityQrStatus ?? null,
      qrRendered: !!one('.security-qr-frame svg[role="img"]'),
      penaltyCurrent: one('.penalty-steps [aria-current="step"] strong')?.textContent.trim() ?? null,
      penaltyText: one('.penalty-instruction')?.textContent.trim() ?? null,
      penaltySteps: all('.penalty-steps li').map(node => ({ passed: node.dataset.passed === 'true', current: node.getAttribute('aria-current') === 'step' })),
      passwordLength: one('[aria-label="登录密码"]')?.value.length ?? 0,
      dialogScrollable: !!dialog && dialog.scrollHeight > dialog.clientHeight + 1,
      dialogScrollTop: dialog?.scrollTop ?? null,
      dialogs: dialog ? [{ open: true, opacity: getComputedStyle(dialog).opacity,
        background: getComputedStyle(dialog).backgroundColor, rect: dialog.getBoundingClientRect().toJSON() }] : [],
    });
  }
  try {
    await wait(() => ready() && one('.welcome'), 'guest bootstrap');
    await theme('light'); await resize(1120, 760);
    one('.account-button').focus();
    await stage('guest_ready_light', { expectGuest: true });
    await submitCase('seed_old');
    await wait(() => ready() && !one('dialog') && one('.account-button').textContent.includes('邮箱验证账号'), 'old synthetic account established');
    await nativeWait(snapshot => snapshot.communityBadgeReads['2'] === 1,
      'one passive followed-song badge read after the first account adoption');

    await submitCase('risk_8810'); await notice('risk');
    assert(one('.login-notice-title').textContent === '合成账号风险说明', '8810 official toast title');
    assert(all('.login-notice-content p').length === 2, '8810 official paragraph array');
    await stage('risk_8810_light', { expectNickname: '邮箱验证账号', code: 8810 });
    await returnPhone();
    assert(one('#login-title').textContent === '手机登录', 'risk back returns to phone');

    await theme('dark'); await resize(760, 540);
    await submitCase('risk_10004'); await notice('risk');
    assert(one('.login-notice-title').textContent === '合成风险说明 — 10004', '10004 title');
    assert(one('.login-notice-content').textContent.includes('<strong>合成文字，应作为正文显示</strong>'), 'HTML-looking notice stays literal');
    await stage('risk_10004_long_small_dark', { expectNickname: '邮箱验证账号', code: 10004 });
    const longContent = one('.login-notice-content');
    const candidates = [longContent, one('.login-notice'), one('.login-panel'), one('dialog[open]')];
    const scroll = candidates.find(node => node && node.scrollHeight > node.clientHeight + 20 && /auto|scroll/.test(getComputedStyle(node).overflowY));
    assert(scroll, 'long notice has a real bounded scroll region');
    scroll.scrollTop = scroll.scrollHeight;
    scroll.dispatchEvent(new Event('scroll', { bubbles: true }));
    await settle();
    assert(scroll.scrollTop > 20, 'long notice scroll moved');
    // A focusable production scroll region keeps focus local without changing
    // its DOM contract. Outer-dialog scrolling retains the existing focus.
    if (scroll.matches('[tabindex]')) scroll.focus({ preventScroll: true });
    await stage('risk_10004_scrolled_small_dark', { expectNickname: '邮箱验证账号', scrollMoved: true, scrollTop: scroll.scrollTop });

    await submitCase('disabled'); await notice('disabled');
    assert(one('.login-notice-title').textContent === '用户禁止登录', '304 explicit disabled account copy');
    await stage('disabled_304_small_dark', { expectNickname: '邮箱验证账号', code: 304 });
    await theme('light'); await resize(1120, 760);
    await submitCase('upgrade_8820'); await notice('upgrade');
    await click(button('查看社区版本'));
    await nativeWait(snapshot => snapshot.browserDocuments.length === 1, 'fixed native community destination');
    await stage('upgrade_8820_light', { expectNickname: '邮箱验证账号', code: 8820 });
    await theme('dark'); await resize(760, 540);
    await submitCase('upgrade_8810'); await notice('upgrade');
    await click(button('查看社区版本'));
    await nativeWait(snapshot => snapshot.browserDocuments.length === 2, 'second native community destination');
    await stage('upgrade_8810_small_dark', { expectNickname: '邮箱验证账号', code: 8810 });

    await theme('light'); await resize(1120, 760);
    await submitCase('penalty_upgrade'); await penalty();
    assert(one('.penalty-steps [aria-current="step"]').textContent.includes('更新官方客户端'), 'penalty upgrade has entry priority');
    assert(one('.penalty-steps').textContent.includes('official-component-v3'), 'official minVersion preserved');
    await stage('penalty_upgrade_light', { expectNickname: '邮箱验证账号', code: 8831 });
    await theme('dark'); await resize(760, 540);
    await submitCase('penalty_identity'); await penalty();
    assert(one('.penalty-instruction').textContent.includes('验证账号身份'), 'identity external boundary');
    await stage('penalty_identity_small_dark', { expectNickname: '邮箱验证账号', code: 8831 });
    await submitCase('penalty_unknown'); await penalty();
    assert(one('.penalty-steps').textContent.includes('当前客户端尚不支持此验证步骤') &&
      !one('.penalty-steps').textContent.includes('FUTURE_SECURITY_ACTION'), 'unknown step uses plain user guidance');
    const steps = one('.penalty-steps');
    assert(steps.children.length === 32 && steps.scrollHeight > steps.clientHeight + 20, 'bounded long penalty list');
    steps.scrollTop = steps.scrollHeight;
    steps.focus({ preventScroll: true });
    await settle();
    assert(steps.scrollTop > 20 && !one('dialog[open]').scrollTop &&
      one('dialog[open]').scrollHeight <= one('dialog[open]').clientHeight + 1,
      'independent penalty scrolling retains dialog controls');
    await stage('penalty_unknown_small_dark', { expectNickname: '邮箱验证账号', code: 8831,
      stepsScrolled: steps.scrollTop > 20 });
    await mode('penalty_status_fail'); await click(one('.penalty-check'));
    await wait(() => button('重新核对状态') && !button('重新核对状态').disabled, 'penalty read failure is retryable');
    await stage('penalty_failed_small_dark', { expectNickname: '邮箱验证账号', statusReads: 1 });
    await mode('penalty_status_unknown'); await click(one('.penalty-check'));
    await wait(() => one('.penalty-instruction')?.textContent.includes('暂不能在 Cumulus'), 'unknown next-step boundary after retry');
    await stage('penalty_retried_small_dark', { expectNickname: '邮箱验证账号', statusReads: 2 });
    await mode('penalty_status_passed'); await click(one('.penalty-check'));
    await wait(() => button('重新登录') && one('.penalty-result'), 'all-passed and identity 301 requires new login');
    assert(!one('.penalty-error'), 'candidate identity failure does not label current account expired');
    await nativeWait(snapshot => snapshot.identity301 === 1, 'penalty identity-only 301 read');
    await stage('penalty_passed_301_small_dark', { expectNickname: '邮箱验证账号', statusReads: 3 });
    await click(button('重新登录'));
    await wait(() => one('[aria-label="登录手机号"]') && status() === 'ready' && !one('.penalty-verification'), 'penalty restart uses a new phone flow');
    assert(!one('[aria-label="登录密码"]'), 'new password must be supplied explicitly');
    await stage('penalty_restart_small_dark', { expectNickname: '邮箱验证账号', noPasswordReplay: true });

    await theme('light'); await resize(1120, 760);
    await submitCase('qr_seed');
    await wait(() => qrStatus() === 'waiting_scan' && one('.security-qr-frame svg[role="img"]'), 'seeded official security ticket rendered');
    assert((await mode('inspect')).legacyCreates === 0, 'first security ticket is used without creating another');
    await click(button('网易云音乐 APP'));
    await nativeWait(snapshot => snapshot.browserDocuments.length === 3, 'fixed native official app document');
    await stage('security_qr_seed_light', { expectNickname: '邮箱验证账号', firstTicketReused: true });
    await mode('qr_802'); await wait(() => qrStatus() === 'waiting_confirmation', 'security QR 802');
    await stage('security_qr_802_light', { expectNickname: '邮箱验证账号', code: 802 });
    await theme('dark'); await resize(760, 540);
    await mode('qr_expired'); await wait(() => qrStatus() === 'expired', 'security QR expires');
    await stage('security_qr_expired_small_dark', { expectNickname: '邮箱验证账号', code: 800 });
    await mode('qr_refresh'); await click(button('点击重试', one('.security-qr-frame')));
    await wait(() => qrStatus() === 'waiting_scan' && one('.security-qr-frame svg[role="img"]'), 'explicit security QR refresh');
    assert((await mode('inspect')).legacyCreates === 1, 'one explicit legacy refresh');
    await stage('security_qr_refreshed_small_dark', { expectNickname: '邮箱验证账号' });
    await mode('qr_captcha'); await wait(() => qrStatus() === 'challenge' && button('打开安全验证'), 'security QR 8821 challenge');
    await stage('security_qr_captcha_small_dark', { expectNickname: '邮箱验证账号', code: 8821 });
    await click(button('打开安全验证'));
    await wait(() => qrStatus() === 'waiting_scan' && one('.security-qr-frame svg[role="img"]'), 'native provider resumes same security QR');
    await nativeWait(snapshot => snapshot.captchaCalls === 1 && snapshot.proofPolls === 1, 'native-only CAPTCHA proof consumed once');
    await stage('security_qr_captcha_resumed_small_dark', { expectNickname: '邮箱验证账号', nativeProofOnly: true });

    await mode('qr_hold');
    await nativeWait(snapshot => snapshot.pending === 'qr_poll', 'held synthetic QR authorization');
    await click(button('关闭登录'));
    await wait(() => !one('dialog'), 'close cancels native QR flow');
    await nativeWait(snapshot => snapshot.heldReceiverClosed, 'native cancelled poll future dropped');
    await mode('release');
    await nativeWait(snapshot => snapshot.pending === null && snapshot.cancelledReplies === 1, 'late QR reply discarded');
    await stage('security_qr_late_cancelled_small_dark', { expectNickname: '邮箱验证账号', lateReplyCancelled: true });

    await theme('light'); await resize(1120, 760);
    await submitCase('qr_authorize');
    await wait(() => one('.security-qr-verification'), 'new security QR candidate');
    await nativeWait(snapshot => snapshot.pending === 'identity_read' && snapshot.identityAuthorized === 1, '803 requires held identity read');
    await wait(() => one('.security-qr-checking'), 'slow native verification has visible feedback');
    assert(!one('.security-qr-description,.security-qr-download,.security-qr-frame svg'),
      'pending verification does not request another scan');
    await stage('security_qr_identity_pending_light', { expectNickname: '邮箱验证账号', code: 803, identityReadBeforeAdoption: true });
    await mode('release');
    await wait(() => ready() && !one('dialog') && one('.account-button').textContent.includes('短信验证账号'), 'verified candidate adopted after identity read');
    const adopted = await nativeWait(snapshot => snapshot.communityBadgeReads['5'] === 1,
      'one passive followed-song badge read after the verified QR account adoption');
    assert(!adopted.paths['/api/login/token/refresh/v1'],
      'verified QR adoption does not synthesize a native foreground renewal event');
    await stage('security_qr_adopted_light', { expectNickname: '短信验证账号', code: 803 });
    await emit('complete', { ok: true, stageCount: count, loginNotices: true,
      expectNickname: '短信验证账号', returnedToPhone: true, noPasswordReplay: true,
      nativeProofOnly: true, lateReplyCancelled: true, identityReadBeforeAdoption: true });
  } catch (error) {
    await emit('failed', { ok: false, last, error: String(error), stack: error.stack,
      body: document.body.innerText.slice(-1200), status: status() ?? null, qrStatus: qrStatus() ?? null });
  }
})();
