// Production Vue/IPC/native phone flow; only HTTP and host clock are synthetic.
(async () => {
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const assert = (value, message) => { if (!value) throw Error(message); };
  const all = selector => [...document.querySelectorAll(selector)].filter(node => !node.closest('.tab-leave-active,.modal-leave-active,.reuse-step-leave-active'));
  const one = selector => all(selector)[0];
  const button = text => all('button').find(node => node.textContent.trim() === text || node.getAttribute('aria-label') === text);
  const wait = async (check, message) => { const end = performance.now() + 24000; while (performance.now() < end) { if (await check()) return; await sleep(40); } throw Error(message); };
  async function settle() {
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await wait(() => !document.querySelector('.tab-enter-active,.tab-leave-active,.reuse-step-enter-active,.reuse-step-leave-active') && !document.getAnimations().some(a => Number.isFinite(a.effect?.getComputedTiming().endTime) && (a.pending || a.playState === 'running')), 'finite transitions');
    await sleep(60);
  }
  function visible(node) { const r = node?.getBoundingClientRect(); return !!r && r.width > 0 && r.height > 0 && r.left >= 0 && r.right <= innerWidth + 1 && r.top >= 0 && r.bottom <= innerHeight + 1; }
  function hit(node) { const r = node.getBoundingClientRect(), target = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2); return target === node || node.contains(target); }
  async function click(node) { await wait(() => node?.isConnected && !node.disabled && !node.closest('[inert]'), 'enabled control'); await settle(); node.scrollIntoView({ block: 'nearest' }); node.focus({ preventScroll: true }); assert(visible(node) && hit(node), 'pointer hit'); node.click(); await sleep(0); }
  async function fill(label, value) { await wait(() => one(`[aria-label="${label}"]`) && !one(`[aria-label="${label}"]`).disabled, 'editable ' + label); const input = one(`[aria-label="${label}"]`); input.focus(); input.value = value; input.dispatchEvent(new Event('input', { bubbles: true })); await sleep(0); return input; }
  const emit = (stage, data = {}) => invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...data } });
  let serial = 0, count = 0, last = 'boot', current = '邮箱验证账号';
  async function mode(value) { const id = ++serial; await invoke('plugin:event|emit', { event: 'phone-entry-fixture', payload: { id, mode: value } }); await wait(() => window.__phoneEntryFixtureAck === id, 'native fixture acknowledgement'); return window.__phoneEntryFixtureSnapshot; }
  async function theme(value) { localStorage.setItem('cumulus.appearance.v1', value); dispatchEvent(new StorageEvent('storage', { key: 'cumulus.appearance.v1', newValue: value })); await wait(() => document.documentElement.dataset.theme === value, 'theme'); await settle(); }
  async function resize(width, height) { await emit('resize', { width, height, scale: devicePixelRatio }); await wait(() => innerWidth === width && innerHeight === height, 'native resize'); await settle(); }
  const panel = () => one('.phone-sms-login'), phase = () => panel()?.dataset.phonePhase;
  const idle = () => !one('.login-flow[aria-busy="true"]');
  async function ready(value) { await wait(() => phase() === value && idle(), 'phone ' + value); await settle(); }
  async function stage(name, extra = {}) {
    last = name; await settle(); const d = one('dialog[open]'), r = d?.getBoundingClientRect();
    assert(document.documentElement.scrollWidth <= innerWidth + 1 && (!d || visible(d) && d.scrollWidth <= d.clientWidth + 1), 'viewport bounds');
    assert(!d || d.scrollHeight <= d.clientHeight + 1, 'phone page fits without dialog scrolling');
    assert(document.hasFocus() && document.activeElement !== document.body && (!d || d.contains(document.activeElement)) && !document.activeElement.closest('[inert]'), 'owned focus');
    const controls = all('dialog[open] button,dialog[open] input').filter(node => !node.disabled && visible(node)).map(node => ({ label: node.getAttribute('aria-label') || node.textContent.trim(), hit: hit(node) }));
    assert(controls.every(control => control.hit), 'exposed controls hit testing');
    if (panel()) {
      assert(!one('.login-status').textContent.trim() && !one('.login-restart'), 'phone flow owns status and recovery actions');
      if (panel().dataset.phoneStep === 'nickname') assert(!one('dialog .brand-mark') && !one('.login-methods') && one('#login-title').textContent === '希望大家怎么称呼你呢？', 'official nickname header');
      if (panel().dataset.phoneStep === 'nickname') { const field = one('.reuse-name-field').getBoundingClientRect(), primary = one('.reuse-nickname .reuse-primary').getBoundingClientRect(); assert(Math.abs(field.width - primary.width) < .25 && Math.abs(field.width - 300) < .25 && primary.height === 40 && one('[aria-label="注册昵称"]').getBoundingClientRect().height === 36, 'official full-width nickname field and primary geometry'); }
      if (phase() === 'code_required') assert(!one('[aria-label="登录手机号"]') && !one('[aria-label="国际区号"]'), 'immutable code recipient');
    }
    await emit(`ux_${String(++count).padStart(2, '0')}_${name}`, { expectNickname: current, preserveEpoch: true, ...extra, width: innerWidth, height: innerHeight, ratio: devicePixelRatio, screenX: window.screenX, screenY: window.screenY, theme: document.documentElement.dataset.theme, phase: phase() ?? null, step: panel()?.dataset.phoneStep ?? null, issue: panel()?.dataset.phoneIssue ?? null, overflow: false, focusValid: true, focused: document.hasFocus(), controls, otpLength: one('[aria-label="短信验证码"]')?.value.length ?? 0, nicknameLength: one('[aria-label="注册昵称"]')?.value.length ?? 0, canReconcile: !!button('核对登录状态'), hasRegister: !!button('完成注册'), hasDefault: !!button('以默认昵称注册'), dialogs: d ? [{ open: true, opacity: getComputedStyle(d).opacity, background: getComputedStyle(d).backgroundColor, rect: r.toJSON() }] : [] });
  }
  async function open() {
    await click(one('.account-button'));
    if (!one('.login-methods')) { await wait(() => one('.account-add'), 'account chooser'); await click(one('.account-add')); }
    await wait(() => one('.login-methods'), 'login methods'); await click(button('手机')); await ready('number');
    await wait(() => !one('[aria-label="登录手机号"]').disabled, 'country metadata');
    await fill('登录手机号', '13800138000');
    if (one('.login-options input[type="checkbox"]').checked) await click(one('.login-options input[type="checkbox"]'));
  }
  async function startCode() { if (!one('[aria-label="同意登录协议"]').checked) await click(one('[aria-label="同意登录协议"]')); await click(one('.login-submit')); await ready('code_required'); assert(document.activeElement === one('[aria-label="短信验证码"]'), 'code step focus'); }
  async function submitCode() { await fill('短信验证码', '123456'); await click(one('.login-submit')); }
  async function nickname(value = 'new') { await mode(value); await mode('advance'); await open(); await startCode(); await submitCode(); await ready('nickname'); assert(document.activeElement === one('[aria-label="注册昵称"]'), 'nickname focus'); }
  async function close() { await click(button('关闭登录')); await wait(() => !one('dialog[open]'), 'dialog closed'); await settle(); }
  async function logged() { await wait(() => !one('dialog') && one('.account-button').textContent.includes('注册账号'), 'native identity committed'); current = '注册账号'; await settle(); }
  try {
    await wait(() => one('.welcome') && one('.account-button') && !one('.account-button').disabled, 'guest bootstrap'); await theme('light'); await resize(1120, 760);
    await click(one('.account-button')); await wait(() => one('.login-methods'), 'initial modal'); await click(button('邮箱'));
    await fill('登录邮箱', 'alice@example.test'); await fill('登录密码', 'fixture-password'); await click(one('[aria-label="同意登录协议"]')); await click(one('.login-submit'));
    await wait(() => !one('dialog') && one('.account-button').textContent.includes(current), 'baseline account');
    await stage('baseline_account_light', { rememberEpoch: true, preserveEpoch: false });
    await open(); assert(!one('[aria-label="短信验证码"]'), 'number and code pages separated'); await stage('number_light');
    await click(one('.login-submit')); assert(one('.login-consent-tip'), 'number consent blocks preflight and SMS'); await stage('consent_required_light');
    await mode('check-failed'); await click(one('[aria-label="同意登录协议"]')); await click(one('.login-submit')); await ready('check_failed'); await stage('check_failed_light');
    await theme('dark'); await resize(760, 540); await stage('check_failed_small_dark');
    await mode('hold-check'); await click(button('重试')); await wait(async () => (await mode('read')).pending, 'held check'); await stage('check_inflight_dark'); await close(); await mode('release'); await stage('check_cancelled_dark');
    await theme('light'); await resize(1120, 760); await mode('new'); await mode('advance'); await open(); await startCode(); await stage('code_light');
    await theme('dark'); await resize(760, 540); await stage('code_small_dark');
    await mode('bad-code'); await submitCode(); await ready('code_required'); assert(one('.credential-error').textContent.includes('验证码错误') && !button('完成注册'), 'bad SMS cannot register'); await stage('code_bad_dark'); await fill('短信验证码', '111111'); assert(!one('.credential-error'), 'editing removes stale code rejection'); await click(button('清空验证码')); assert(one('[aria-label="短信验证码"]').value === '' && document.activeElement === one('[aria-label="短信验证码"]'), 'clear code keeps input focus');
    await mode('new'); await submitCode(); await ready('nickname'); await stage('nickname_small_dark');
    await theme('light'); await resize(1120, 760); await stage('nickname_light');
    await fill('注册昵称', 'a'); await click(button('完成注册')); await ready('nickname'); assert(one('.verification-error').textContent.includes('不能少于'), 'native length validation'); await stage('nickname_short_light');
    await theme('dark'); await mode('taken'); await fill('注册昵称', '重复昵称'); await click(button('完成注册')); await ready('nickname'); assert(one('.verification-error').textContent.includes('已使用'), 'readonly duplicate check'); await stage('nickname_taken_dark');
    await mode('new'); await click(button('换一个推荐昵称')); await ready('nickname'); assert(one('[aria-label="注册昵称"]').value === '合成新昵称', 'native nickname hint'); await stage('nickname_refresh_dark'); await click(button('清空昵称')); assert(one('[aria-label="注册昵称"]').value === '' && button('完成注册').disabled, 'clear nickname disables empty registration'); await click(button('换一个推荐昵称')); await ready('nickname');
    await mode('anti-abuse'); await click(button('完成注册')); await ready('nickname'); assert(one('.verification-error').textContent.includes('安全检查'), 'known registration rejection'); await stage('registration_rejected_dark');
    await mode('rate-limit'); await click(button('完成注册')); await ready('nickname'); assert(one('.verification-error').textContent.includes('频繁'), 'known rate rejection'); await stage('registration_rate_limited_dark');
    await mode('sms-invalid'); await click(button('完成注册')); await ready('code_required'); assert(!button('完成注册') && one('[aria-label="短信验证码"]').value === '', 'invalid authorization clears verified code'); await stage('registration_sms_invalid_dark');
    await click(button('修改手机号')); await ready('number'); await mode('no-default'); await mode('advance'); await startCode(); await submitCode(); await ready('nickname'); assert(!button('以默认昵称注册'), 'source preflight controls skip permission'); await theme('light'); await stage('nickname_no_default_light');
    await click(button('取消')); await ready('number'); await stage('nickname_cancelled_light');
    await startCode(); await submitCode(); await ready('nickname'); const draft = await fill('注册昵称', '关闭清空昵称'); await close(); assert(draft.value === '', 'nickname scrubbed on retained exit DOM'); await stage('nickname_closed_light');
    await nickname(); await click(button('以默认昵称注册')); await logged(); await stage('registered_default_light', { rememberEpoch: true, preserveEpoch: false });
    await mode('existing'); await mode('advance'); await open(); await startCode(); await stage('existing_code_light'); await submitCode(); await logged(); await stage('existing_login_light', { rememberEpoch: true, preserveEpoch: false });
    await nickname(); await fill('注册昵称', '明确新昵称'); await click(button('完成注册')); await logged(); await stage('registered_named_light', { rememberEpoch: true, preserveEpoch: false });
    await nickname(); await mode('partial-register'); await click(button('以默认昵称注册')); await ready('recovery_required'); assert(button('核对登录状态') && !button('完成注册'), 'partial receipt offers readonly recovery only'); await theme('dark'); await resize(760, 540); await stage('registration_partial_dark');
    await mode('new'); await click(button('核对登录状态')); await logged(); await stage('registration_reconciled_dark', { rememberEpoch: true, preserveEpoch: false });
    await theme('light'); await nickname(); await mode('lost-register'); await click(button('以默认昵称注册')); await ready('recovery_required'); assert(!button('核对登录状态') && !button('完成注册'), 'missing receipt cannot reconcile or resubmit'); await stage('registration_lost_light');
    await click(button('重新短信登录')); await ready('number'); await startCode(); assert(one('.reuse-boundary')?.textContent.includes('仅进行短信登录核对'), 'durable registration fence'); await stage('registration_retry_blocked_light');
    await mode('new'); await submitCode(); await logged(); await stage('registration_fresh_login_light', { rememberEpoch: true, preserveEpoch: false });
    await nickname(); await theme('dark'); await mode('hold-register'); await click(button('以默认昵称注册')); await wait(async () => (await mode('read')).pending, 'held registration'); await stage('register_inflight_dark'); await close(); await mode('release'); await stage('register_closed_dark');
    await open(); await startCode(); await submitCode(); await logged(); await stage('registration_held_fresh_login_dark', { rememberEpoch: true, preserveEpoch: false });
    await theme('light'); await mode('send-failed'); await mode('advance'); await open(); await startCode(); assert(one('.credential-error')?.textContent.includes('发送失败'), 'explicit SMS failure'); await stage('send_failed_light'); await close();
    await mode('send-lost'); await mode('advance'); await open(); await startCode(); await theme('dark'); assert(one('.credential-error')?.textContent.includes('发送结果未确认'), 'lost send not automatically repeated'); await stage('send_unknown_small_dark'); await close();
    await emit('complete', { ok: true, stageCount: count, expectNickname: current, preserveEpoch: true, realMusicRequests: 0, phoneEntry: true });
  } catch (error) {
    await emit('failed', { last, error: String(error), stack: error.stack, body: document.body.innerText.slice(-2000), panel: panel()?.outerHTML ?? null, focusedElement: document.activeElement?.outerHTML });
  }
})();
