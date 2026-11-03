// Production Vue, native IPC and isolated synthetic HTTP. No real login or SMS.
(async () => {
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (condition, message) => { if (!condition) throw Error(message); };
  const all = (selector) => [...document.querySelectorAll(selector)].filter((node) => !node.closest('.tab-leave-active,.modal-leave-active,.reuse-step-leave-active'));
  const one = (selector) => all(selector)[0];
  const button = (label) => all('button').find((node) => node.textContent.trim() === label || node.getAttribute('aria-label') === label);
  const wait = async (fn, label) => {
    const deadline = performance.now() + 24000;
    while (performance.now() < deadline) { if (await fn()) return; await sleep(40); }
    throw Error(label);
  };
  async function settle() {
    await wait(async () => {
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      return !document.getAnimations().some((a) => Number.isFinite(a.effect?.getComputedTiming().endTime) && (a.pending || a.playState === 'running'));
    }, 'finite animations');
  }
  function visible(node) {
    if (!node?.isConnected || node.closest('[inert]')) return false;
    const rect = node.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0 && rect.top >= 0 && rect.left >= 0 && rect.bottom <= innerHeight + 1 && rect.right <= innerWidth + 1;
  }
  function hit(node) {
    const rect = node.getBoundingClientRect(), top = document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2);
    return node === top || node.contains(top);
  }
  async function click(node) {
    assert(node?.isConnected, 'control exists');
    await wait(() => !node.disabled, 'enabled control');
    node.scrollIntoView({ block: 'nearest' });
    node.focus({ preventScroll: true });
    assert(visible(node) && hit(node), 'visible control hit test');
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
  const status = () => one('[data-login-status]')?.dataset.loginStatus;
  const idle = () => status() === 'ready' && !one('.tab-leave-active');
  async function tab(label) {
    await click(button(label));
    await wait(idle, 'credential ready');
    await settle();
  }
  async function theme(value) {
    localStorage.setItem('cumulus.appearance.v1', value);
    dispatchEvent(new StorageEvent('storage', { key: 'cumulus.appearance.v1', newValue: value }));
    await wait(() => document.documentElement.dataset.theme === value, 'theme'); await settle();
  }
  const emit = (stage, value = {}) => invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...value } });
  let fixtureSerial = 0;
  async function mode(value) {
    const id = ++fixtureSerial;
    await invoke('plugin:event|emit', { event: 'account-login-fixture', payload: { id, mode: value } });
    await wait(() => window.__accountLoginAck === id, 'native fixture acknowledgement');
  }
  async function resize(width, height) {
    await emit('resize', { width, height, scale: devicePixelRatio });
    await wait(() => innerWidth === width && innerHeight === height, 'viewport'); await settle();
  }
  async function country(alpha) {
    await click(one('[aria-label="国际区号"]'));
    await fill('搜索国家或地区', alpha);
    await wait(() => one(`[data-country="${alpha}"]`), 'country result');
    await click(one(`[data-country="${alpha}"]`));
    await wait(async () => (await invoke('login_presentation')).selectedCountry === alpha, 'country persisted');
  }
  let count = 0, last = 'boot';
  async function stage(name, extra = {}) {
    last = name; await settle();
    const dialog = one('dialog[open]'), popup = one('.login-country-menu,.login-email-suggestions');
    const rect = dialog?.getBoundingClientRect();
    const overflow = document.documentElement.scrollWidth > innerWidth + 1 || (dialog && (dialog.scrollWidth > dialog.clientWidth + 1 || !visible(dialog))) || (popup && !visible(popup));
    assert(!overflow, 'dialog/menu viewport bounds');
    const scrollable = !!dialog && dialog.scrollHeight > dialog.clientHeight + 1;
    assert(!scrollable, 'ordinary credential and recovery pages fit without scrolling');
    const focus = document.activeElement;
    assert(document.hasFocus() && focus && focus !== document.body && (!dialog || dialog.contains(focus)) && !focus.closest('[inert]'), 'live modal/page focus');
    const controls = all('dialog[open] button,dialog[open] input').filter((node) => !node.disabled && visible(node) && (!popup || popup.contains(node)))
      .map((node) => ({ label: node.getAttribute('aria-label') || node.textContent.trim(), hit: hit(node) }));
    assert(controls.every((node) => node.hit), 'exposed controls hit testing');
    const list = one('.login-country-list');
    const rendered = all('[data-country]').length;
    if (list) assert(rendered <= 18 && popup.getBoundingClientRect().height <= 315.1, 'bounded official country menu');
    await emit(`ux_${String(++count).padStart(2, '0')}_${name}`, {
      ...extra, width: innerWidth, height: innerHeight, ratio: devicePixelRatio,
      screenX: window.screenX, screenY: window.screenY, theme: document.documentElement.dataset.theme,
      focused: document.hasFocus(), focusValid: true, overflow: false, dialogScrollable: false, controls,
      dialogs: dialog ? [{ open: true, opacity: getComputedStyle(dialog).opacity, background: getComputedStyle(dialog).backgroundColor, rect: rect.toJSON() }] : [],
      region: one('[aria-label="国际区号"]')?.textContent.trim() ?? null,
      countryRows: rendered, countryScrollTop: list?.scrollTop ?? null,
      countryMenu: popup?.getBoundingClientRect().toJSON() ?? null,
      consent: one('[aria-label="同意登录协议"]')?.checked ?? null,
      consentRequired: !!one('.login-consent-tip'), validation: one('.credential-error')?.textContent.trim() ?? null,
      passwordLength: one('[aria-label="登录密码"]')?.value.length ?? 0,
      otpLength: one('[aria-label="短信验证码"]')?.value.length ?? 0,
    });
  }
  try {
    await wait(() => one('.account-button') && !one('.account-button').disabled && one('.welcome'), 'guest bootstrap');
    await theme('light'); await resize(1120, 760);
    await click(one('.account-button')); await wait(() => one('.login-methods'), 'login modal');
    await tab('手机'); await wait(() => !one('[aria-label="国际区号"]').disabled, 'country metadata');
    assert(!one('[aria-label="同意登录协议"]').checked, 'consent defaults unchecked');
    await stage('phone_default_light', { expectGuest: true });
    for (const name of ['《服务条款》', '《隐私政策》', '《儿童隐私政策》']) {
      await click(button(name));
      await wait(() => !one('.login-legal-links button').disabled, 'public document completed');
    }
    assert(!one('[aria-label="同意登录协议"]').checked, 'reading documents does not imply consent');
    await stage('legal_documents_light', { expectGuest: true });
    await mode('browser_failed'); await click(button('《隐私政策》'));
    await wait(() => one('.credential-error')?.textContent.includes('浏览器未能打开'), 'browser failure visible');
    await stage('legal_launch_failed_light', { expectGuest: true });
    await theme('dark'); await resize(760, 540);
    await stage('legal_launch_failed_small_dark', { expectGuest: true });
    await resize(1120, 760); await theme('light');
    await mode('normal'); await click(button('《隐私政策》'));
    await wait(() => !one('.credential-error'), 'explicit document retry clears error');
    await fill('登录手机号', '13800000000');
    await click(one('.login-submit')); assert(one('.login-consent-tip') && status() === 'ready', 'unchecked SMS is blocked');
    await stage('sms_consent_required_light', { expectGuest: true });
    await click(one('.login-submit')); assert(one('[aria-label="登录手机号"]').value === '13800000000' && !one('[aria-label="短信验证码"]'), 'unchecked number submission retains identifier without advancing');
    await stage('submit_consent_required_light', { expectGuest: true });
    await click(one('[aria-label="同意登录协议"]')); await fill('登录手机号', '1380000000');
    await click(one('.login-submit')); assert(one('.credential-error')?.textContent.includes('11位'), 'strict China length');
    await stage('invalid_phone_light', { expectGuest: true });
    await click(one('[aria-label="国际区号"]')); await stage('country_picker_light', { expectGuest: true });
    one('[aria-label="搜索国家或地区"]').dispatchEvent(new KeyboardEvent('keydown', { key: 'End', bubbles: true, cancelable: true }));
    await wait(() => one('[data-country="ZM"]'), 'last country rendered');
    await stage('country_tail_keyboard_light', { expectGuest: true });
    await fill('搜索国家或地区', 'CA'); await wait(() => one('[data-country="CA"]'), 'Canada');
    await click(one('[data-country="CA"]'));
    await wait(async () => (await invoke('login_presentation')).selectedCountry === 'CA', 'Canada selected');
    assert(one('[aria-label="国际区号"]').textContent.includes('+1'), 'shared calling code');
    await country('US'); await country('CA');
    await stage('shared_calling_code_light', { expectGuest: true });
    await theme('dark'); await resize(760, 540); await country('MY');
    await fill('登录手机号', '11111111'); await click(one('.login-submit'));
    assert(one('.credential-error')?.textContent.includes('9或10位'), 'multiple official lengths');
    await stage('multiple_lengths_small_dark', { expectGuest: true });
    await click(one('[aria-label="国际区号"]'));
    await wait(() => {
      const node = one('[data-country="MY"]'), list = one('.login-country-list');
      if (!node || !list) return false;
      const row = node.getBoundingClientRect(), bounds = list.getBoundingClientRect();
      return row.top >= bounds.top - 1 && row.bottom <= bounds.bottom + 1;
    }, 'reopened country menu scrolls to the current region');
    assert(document.getElementById(one('[aria-label="搜索国家或地区"]').getAttribute('aria-activedescendant'))?.dataset.country === 'MY', 'current region owns keyboard focus');
    await stage('country_picker_small_dark', { expectGuest: true });
    await fill('搜索国家或地区', '不存在的地区'); await stage('country_empty_small_dark', { expectGuest: true });
    one('[aria-label="搜索国家或地区"]').dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    await wait(() => !one('.login-country-menu'), 'Escape closes only menu');
    assert(one('dialog[open]') && document.activeElement === one('[aria-label="国际区号"]'), 'Escape retains modal and restores trigger focus');
    await country('HK'); await fill('登录手机号', '51234567');
    await click(one('.login-options input[type="checkbox"]')); await tab('密码登录'); await fill('登录密码', 'fixture-password');
    await click(button('忘记密码')); await wait(() => one('.phone-password-help') && !one('.tab-leave-active'), 'phone recovery handoff');
    assert(!one('[aria-label="登录密码"]') && one('#login-title').textContent === '忘记密码', 'password discarded on recovery');
    await stage('phone_recovery_small_dark', { expectGuest: true });
    await theme('light'); await stage('phone_recovery_small_light', { expectGuest: true });
    await tab('其他登录方式');
    assert(one('[aria-label="登录手机号"]').value === '51234567' && one('[aria-label="登录密码"]').value === '', 'recovery returns identifier without secret');
    assert(one('[aria-label="同意登录协议"]').checked && !one('.login-options input[type="checkbox"]').checked, 'consent/remember survive recovery');
    await stage('recovery_return_small_light', { expectGuest: true });
    await tab('邮箱'); await theme('dark'); await fill('登录邮箱', 'name');
    await stage('email_suggestions_small_dark', { expectGuest: true });
    const email = one('[aria-label="登录邮箱"]');
    email.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true }));
    email.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
    await wait(() => one('[aria-label="登录邮箱"]').value === 'name@163.com' && document.activeElement === one('[aria-label="登录密码"]'), 'keyboard completion moves to password');
    await stage('email_completed_small_dark', { expectGuest: true });
    await click(button('忘记密码'));
    await wait(() => !button('忘记密码').disabled, 'native email recovery handoff');
    assert(one('[aria-label="登录邮箱"]').value === 'name@163.com', 'email recovery preserves input');
    await stage('email_recovery_handoff_small_dark', { expectGuest: true });
    await fill('登录邮箱', 'alice@example.test'); await fill('登录密码', 'fixture-password');
    await click(one('[aria-label="同意登录协议"]')); await click(one('.login-submit'));
    assert(one('.login-consent-tip') && one('[aria-label="登录密码"]').value === 'fixture-password', 'unchecked email retains password without native login');
    await stage('email_consent_required_small_dark', { expectGuest: true });
    await click(one('[aria-label="同意登录协议"]')); await click(one('.login-submit'));
    await wait(() => !one('dialog') && one('.account-button').textContent.includes('邮箱验证账号'), 'explicit synthetic email login');
    await theme('light'); await stage('email_authorized_small_light', { expectNickname: '邮箱验证账号' });
    await click(one('.account-button')); await wait(() => one('.account-add'), 'saved accounts'); await click(one('.account-add'));
    await wait(() => one('.login-methods'), 'new login modal'); await tab('手机');
    await wait(() => one('[aria-label="国际区号"]').textContent.includes('+852'), 'saved country restored');
    assert(!one('[aria-label="同意登录协议"]').checked && one('[aria-label="登录手机号"]').value === '', 'new modal resets consent and identifier');
    await stage('saved_region_small_light', { expectNickname: '邮箱验证账号' });
    await country('CA'); await fill('登录手机号', '1111111111'); await click(one('[aria-label="同意登录协议"]'));
    await click(one('.login-submit')); await wait(() => one('[data-phone-phase="code_required"]'), 'one explicit synthetic SMS'); await settle();
    await wait(() => document.activeElement === one('[aria-label="短信验证码"]'), 'SMS completion returns focus to OTP rather than a legal link');
    await stage('region_sms_cooldown_small_light', { expectNickname: '邮箱验证账号' });
    await tab('密码登录'); await tab('验证码登录'); await click(one('.login-submit')); await wait(() => one('[data-phone-phase="code_required"]'), 'same phone code page'); await settle();
    assert(/秒后重发/.test(one('.login-code button').textContent) && one('.login-code button').disabled, 'cooldown survives country-aware mode switching');
    await stage('region_sms_return_small_light', { expectNickname: '邮箱验证账号' });
    await click(one('[aria-label="关闭登录"]')); await wait(() => !one('dialog'), 'closed');
    await stage('closed_small_light', { expectNickname: '邮箱验证账号' });
    await emit('complete', { ok: true, stageCount: count, expectNickname: '邮箱验证账号', realMusicRequests: 0, entry: true });
  } catch (error) {
    await emit('failed', { last, error: String(error), stack: error.stack, body: document.body.innerText.slice(-1800), dialog: one('dialog[open]')?.outerHTML, focusedElement: document.activeElement?.outerHTML });
  }
})();
