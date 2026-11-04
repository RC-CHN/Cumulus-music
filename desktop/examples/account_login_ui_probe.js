// Production DOM actions, native IPC and synthetic HTTP. No credential/session dumps.
(async () => {
  const invoke = (command, args = {}) =>
    window.__TAURI_INTERNALS__.invoke(command, args);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (value, message) => {
    if (!value) throw Error(message);
  };
  const one = (selector) => document.querySelector(selector);
  const all = (selector, root = document) =>
    [...(root?.querySelectorAll(selector) ?? [])].filter(
      (node) =>
        !node.closest(
          "[inert],.tab-leave-active,.modal-leave-active,.reuse-step-leave-active",
        ),
    );
  const button = (text, root = document) =>
    all("button", root).find(
      (node) =>
        node.textContent.trim() === text ||
        node.getAttribute("aria-label") === text,
    );
  const emit = (stage, payload = {}) =>
    invoke("plugin:event|emit", {
      event: "ux-proof",
      payload: { stage, ...payload },
    });
  let count = 0,
    serial = 0,
    last = "boot";
  const wait = async (test, label) => {
    for (let i = 0; i < 600; i++) {
      if (await test()) return;
      await sleep(40);
    }
    throw Error(label);
  };
  async function settle() {
    await wait(async () => {
      await new Promise((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(resolve)),
      );
      return !document
        .getAnimations()
        .some(
          (a) =>
            Number.isFinite(a.effect?.getComputedTiming().endTime) &&
            (a.pending || a.playState === "running"),
        );
    }, "finite animations");
  }
  function visible(node) {
    if (!node?.isConnected) return false;
    const r = node.getBoundingClientRect();
    return (
      r.width > 0 &&
      r.height > 0 &&
      getComputedStyle(node).visibility !== "hidden" &&
      r.top >= -1 &&
      r.bottom <= innerHeight + 1 &&
      r.left >= -1 &&
      r.right <= innerWidth + 1
    );
  }
  function hit(node) {
    const r = node.getBoundingClientRect(),
      target = document.elementFromPoint(
        r.left + r.width / 2,
        r.top + r.height / 2,
      );
    return target === node || node.contains(target);
  }
  function click(node) {
    assert(node?.isConnected && !node.disabled, "enabled control");
    node.scrollIntoView({ block: "nearest" });
    node.focus({ preventScroll: true });
    assert(visible(node) && hit(node), "control visible and not occluded");
    node.click();
  }
  function fill(label, value) {
    const node = all(`[aria-label="${label}"]`)[0];
    assert(node && !node.disabled, "editable " + label);
    node.scrollIntoView({ block: "nearest" });
    node.focus();
    node.value = value;
    node.dispatchEvent(new Event("input", { bubbles: true }));
  }
  const status = () => all("[data-login-status]")[0]?.dataset.loginStatus;
  const ready = () =>
    one(".account-button") && !one(".account-button").disabled;
  async function mode(value) {
    const id = ++serial;
    await invoke("plugin:event|emit", {
      event: "account-login-fixture",
      payload: { id, mode: value },
    });
    await wait(() => window.__accountLoginAck === id, "fixture mode ACK");
  }
  async function theme(value) {
    localStorage.setItem("cumulus.appearance.v1", value);
    dispatchEvent(
      new StorageEvent("storage", {
        key: "cumulus.appearance.v1",
        newValue: value,
      }),
    );
    await wait(() => document.documentElement.dataset.theme === value, "theme");
    await settle();
  }
  async function resize(width, height) {
    await emit("resize", { width, height, scale: devicePixelRatio });
    await wait(
      () => innerWidth === width && innerHeight === height,
      "CSS viewport",
    );
    await settle();
  }
  async function stage(name, extra = {}) {
    last = name;
    await settle();
    const focus = document.activeElement,
      dialog = (window.__deviceRemovalProof && focus?.closest("dialog[open]")) || one("dialog[open]");
    const controls = dialog
      ? [...dialog.querySelectorAll("button,input,select")]
          .filter(
            (node) =>
              !node.disabled && node.getClientRects().length && visible(node),
          )
          .map((node) => ({
            label: node.getAttribute("aria-label") || node.textContent.trim(),
            hit: hit(node),
          }))
      : [];
    const overflow =
      document.documentElement.scrollWidth > innerWidth ||
      (!!dialog &&
        (dialog.scrollWidth > dialog.clientWidth + 1 || !visible(dialog)));
    const focused = document.hasFocus(),
      focusValid =
        !!focus &&
        focus !== document.body &&
        (!dialog || dialog.contains(focus)) &&
        !focus.closest("[inert]");
    const dialogScrollable =
      !!dialog && dialog.scrollHeight > dialog.clientHeight + 1;
    const ordinaryLogin =
      dialog?.matches(".login-dialog") &&
      (one(".qr-frame") || ["ready", "code_sent"].includes(status()));
    if (!window.__accountLoginBaseline && ordinaryLogin)
      assert(
        !dialogScrollable,
        "ordinary login fits without vertical scrolling",
      );
    assert(!overflow, "dialog/viewport overflow");
    if (!window.__accountLoginBaseline)
      assert(focused && focusValid, "live visible modal/page focus");
    assert(
      controls.every((node) => node.hit),
      "visible controls hit testing",
    );
    await emit(`ux_${String(++count).padStart(2, "0")}_${name}`, {
      ...extra,
      width: innerWidth,
      height: innerHeight,
      theme: document.documentElement.dataset.theme,
      focused,
      focusValid,
      overflow,
      dialogScrollable,
      controls,
      screenX: window.screenX,
      screenY: window.screenY,
      ratio: devicePixelRatio,
      dialogs: dialog
        ? [
            {
              open: true,
              opacity: getComputedStyle(dialog).opacity,
              background: getComputedStyle(dialog).backgroundColor,
              rect: dialog.getBoundingClientRect().toJSON(),
            },
          ]
        : [],
      status: status() ?? null,
      phone: all('[aria-label="登录手机号"]')[0]?.value ?? null,
      country: all('[aria-label="国际区号"]')[0]?.value ?? null,
      passwordLength: all('[aria-label="登录密码"]')[0]?.value.length ?? 0,
      codeLength: all('[aria-label="短信验证码"]')[0]?.value.length ?? 0,
    });
  }
  async function tab(label) {
    click(button(label, one(".login-methods")));
    // Vue can still expose the outgoing ready status until its next patch.
    // Wait for the selected panel and its native begin-login reply together.
    await sleep(0);
    await wait(
      () =>
        button(label, one(".login-methods")).getAttribute("aria-pressed") === "true" &&
        !one(".tab-leave-active") &&
        (label === "扫码"
          ? one(".qr-frame")
          : status() === "ready" &&
            all(`[aria-label="${label === "邮箱" ? "登录邮箱" : "登录手机号"}"]`).some(
              (node) => !node.disabled && !node.closest("[inert]"),
            )),
      "login method ready",
    );
    await settle();
  }
  async function open() {
    click(one(".account-button"));
    await wait(() => one(".login-methods"), "login dialog");
    await settle();
  }
  async function closeLogin() {
    click(button("关闭登录"));
    await wait(() => !one("dialog"), "login closed");
    await settle();
  }
  async function logged(name = "邮箱验证账号") {
    await wait(
      () =>
        ready() &&
        !one("dialog") &&
        one(".account-button").textContent.includes(name),
      "native account committed",
    );
    await settle();
  }
  async function chooser() {
    click(one(".account-button"));
    await wait(
      () => one(".account-add") && !one('.account-dialog [role="status"]'),
      "account chooser loaded",
    );
    await wait(
      () =>
        ["refreshed", "not_due"].includes(
          one(".session-renewal")?.dataset.state,
        ),
      "automatic session renewal",
    );
    await settle();
  }
  async function country(alpha) {
    click(one('[aria-label="国际区号"]'));
    await wait(() => one('[aria-label="搜索国家或地区"]'), 'country picker');
    fill('搜索国家或地区', alpha);
    await wait(() => one(`[data-country="${alpha}"]`), 'country option');
    await settle();
    click(one(`[data-country="${alpha}"]`));
    await wait(() => !one('.login-country-menu'), 'country selection');
  }
  async function email(
    address = "alice@example.test",
    password = "fixture-password",
  ) {
    await tab("邮箱");
    fill("登录邮箱", address);
    fill("登录密码", password);
    if (!one('[aria-label="同意登录协议"]').checked)
      click(one('[aria-label="同意登录协议"]'));
    click(one(".login-submit"));
  }
  async function accountDetail() {
    const expectNickname = "邮箱验证账号";
    const profile = ".account-profile-page";
    const devices = ".login-devices-page";
    const membership = () => page(devices)?.querySelector(".login-device-membership");
    const limits = () => all(".login-device-limit-value", membership()).map((node) => node.textContent);
    const selectedTier = () => membership()?.querySelector("th[data-current-tier]")?.dataset.currentTier;
    const page = (selector) =>
      all(selector).find((node) => !node.closest(".page-leave-active"));
    const state = (selector) =>
      page(selector)?.querySelector("[data-account-read-state]")?.dataset
        .accountReadState ?? page(selector)?.dataset.accountReadState;
    const settingsEntry = (kind) => one(`[data-testid="account-${kind}-open"]`);
    const back = async (kind) => {
      click(one(".back-button"));
      await wait(() => settingsEntry(kind), "settings restored");
      await settle();
    };
    await open();
    await email();
    await logged();
    click(button("打开设置"));
    await wait(() => settingsEntry("profile"), "profile entry");
    await settle();
    click(settingsEntry("profile"));
    await wait(
      () => page(profile)?.textContent.includes("这是合成的账号资料"),
      "profile data",
    );
    const profileText = page(profile).textContent;
    assert(profileText.includes("浙江省 · 杭州市"), "recovered region label");
    assert(profileText.includes("p***@e***") && !profileText.includes("private-fixture"), "masked email");
    await stage("profile_ready_light", { expectNickname });
    await back("devices");

    await mode("devices_hold");
    click(settingsEntry("devices"));
    await wait(() => state(devices) === "loading", "device loading");
    assert(!membership(), "membership hidden while loading");
    await stage("devices_loading_light", { expectNickname });
    await mode("release");
    await wait(() => all(".login-device-card", page(devices)).length === 2, "device rows");
    const cards = all(".login-device-card", page(devices));
    assert(cards[0].textContent.includes("测试桌面"), "current device pinned");
    assert(!page(devices).innerHTML.includes("fixture-private-key"), "device key excluded");
    assert(membership()?.textContent.includes("设备额度合成账号"), "membership response identity");
    assert(membership().textContent.includes("已登录1台手机/电脑/平板设备"), "device count");
    assert(selectedTier() === "vip" && limits().join(",") === "5台,9台,11台", "VIP current and service tier rules");
    assert(!membership().querySelector("a,button") && !membership().textContent.includes("家庭会员"), "read-only membership card");
    await stage("devices_ready_light", { expectNickname });
    click(cards[0]);
    await wait(() => one(".login-device-detail")?.textContent.includes("杭州"), "device detail");
    await stage("device_detail_light", { expectNickname });
    click(button("关闭设备详情"));
    await wait(() => !one(".login-device-detail"), "device dialog closed");
    await settle();
    await back("devices");

    await theme("dark");
    await resize(880, 670);
    await mode("devices_failed");
    click(settingsEntry("devices"));
    await wait(() => state(devices) === "error", "device error");
    assert(!membership(), "membership hidden after read failure");
    await stage("devices_error_dark", { expectNickname });
    await mode("devices_empty");
    click(one('[data-testid="account-devices-retry"]'));
    await wait(() => state(devices) === "empty", "confirmed empty");
    assert(!membership(), "membership hidden for empty response");
    await stage("devices_empty_dark", { expectNickname });
    await mode("devices_ordinary");
    click(one('[data-testid="account-devices-retry"]'));
    await wait(() => selectedTier() === "ordinary", "ordinary membership after retry");
    assert(membership().textContent.includes("畅听合成账号") && !membership().textContent.includes("设备额度合成账号"), "fresh read replaces membership identity");
    assert(membership().textContent.includes("已登录设备数未知"), "missing device count");
    assert(limits().join(",") === "4台,8台,10台", "ordinary alias and default quota rules");
    await stage("devices_ordinary_dark", { expectNickname });
    await emit("complete", { ok: true, accountDetail: true, stageCount: count, expectNickname });
  }
  async function profileEdit() {
    const expectNickname = "邮箱验证账号";
    const test = (id) => one(`[data-testid="${id}"]`);
    const state = () => one(".account-profile-edit-page")?.dataset.profileEditState;
    const hasSignature = (text) =>
      one('[data-profile-field="signature"]')?.textContent.includes(text);
    await open();
    await email();
    await logged();
    click(button("打开设置"));
    await wait(() => test("account-profile-open"), "profile entry");
    await settle();
    click(test("account-profile-open"));
    await wait(() => hasSignature("这是合成的账号资料"), "profile ready");
    await settle();
    click(test("account-binding-qr-open"));
    await wait(() => one("svg[data-account-binding-qr]"), "local binding QR");
    await stage("binding_qr_light", { expectNickname });
    click(button("关闭扫码设置"));
    await wait(() => !one(".account-binding-qr-dialog"), "binding QR closed");
    await settle();
    async function edit(signature) {
      click(test("account-profile-edit-open"));
      await wait(() => state() === "ready" && test("profile-edit-signature"), "edit ready");
      await wait(() =>
        test("profile-edit-province")?.selectedOptions[0]?.textContent.includes("浙江省（合成）") &&
        test("profile-edit-city")?.selectedOptions[0]?.textContent.includes("杭州市（合成）"),
      "server region or validated cache selected");

      await settle();
      const field = test("profile-edit-signature");
      field.focus();
      field.value = signature;
      field.dispatchEvent(new Event("input", { bubbles: true }));
      await wait(() => !test("profile-edit-save")?.disabled, "valid edit");
      await settle();
    }
    await edit("桌面验证：已保存");
    await stage("profile_edit_dirty_light", { expectNickname });
    click(test("profile-edit-save"));
    await wait(
      () => hasSignature("桌面验证：已保存") &&
        one('[data-profile-field="region"]')?.textContent.includes("杭州市（合成）"),
      "profile readback uses cached region name",
    );
    await stage("profile_edit_saved_light", { expectNickname });
    await theme("dark");
    await resize(880, 670);
    await mode("profile_ack_lost");
    await edit("桌面验证：回包丢失");
    click(test("profile-edit-save"));
    await wait(() => state() === "recovery", "uncertain write recovery");
    await stage("profile_edit_recovery_dark", { expectNickname });
    await mode("normal");
    click(test("profile-edit-reconcile"));
    await wait(() => hasSignature("桌面验证：回包丢失"), "read-only recovery applied");
    await stage("profile_edit_reconciled_dark", { expectNickname });
    await emit("complete", { ok: true, profileEdit: true, profileRegionsVerified: true, stageCount: count, expectNickname });
  }
  async function accountAvatar() {
    const expectNickname = "邮箱验证账号";
    const test = (id) => one(`[data-testid="${id}"]`);
    const dialog = () => one(".account-avatar-dialog");
    const signature = () => test("profile-edit-signature");
    const retained = "未提交的简介草稿";
    await open();
    await email();
    await logged();
    click(button("打开设置"));
    await wait(() => test("account-profile-open"), "profile entry");
    await settle();
    click(test("account-profile-open"));
    await wait(() => test("account-profile-edit-open"), "profile details");
    await settle();
    click(test("account-profile-edit-open"));
    await wait(() => signature() && test("account-avatar-open")?.getAttribute("aria-disabled") === "false", "avatar editor available");
    await settle();
    signature().value = retained;
    signature().dispatchEvent(new Event("input", { bubbles: true }));
    async function crop() {
      click(test("account-avatar-open"));
      await wait(() => test("account-avatar-crop")?.dataset.crop && !test("account-avatar-save")?.disabled, "avatar crop ready");
      await settle();
      const input = test("account-avatar-zoom");
      assert(input, "zoom control");
      const previous = test("account-avatar-crop").dataset.crop;
      input.value = input.max;
      input.dispatchEvent(new Event("input", { bubbles: true }));
      await wait(() => test("account-avatar-crop").dataset.crop !== previous, "crop reacts to zoom");
    }
    async function confirmed() {
      await wait(() => !dialog() && signature()?.value === retained &&
        test("account-avatar-open")?.getAttribute("aria-disabled") === "false", "avatar confirmed and text draft retained");
    }
    await crop();
    await stage("avatar_crop_light", { expectNickname });
    click(test("account-avatar-save"));
    await confirmed();
    await stage("avatar_confirmed_light", { expectNickname });
    await theme("dark");
    await resize(880, 670);
    await mode("avatar_ack_lost");
    await crop();
    click(test("account-avatar-save"));
    await wait(() => dialog()?.dataset.avatarPhase === "binding" && !test("account-avatar-reconcile")?.disabled, "avatar readback only");
    await stage("avatar_recovery_dark", { expectNickname });
    await mode("normal");
    click(test("account-avatar-reconcile"));
    await confirmed();
    await stage("avatar_reconciled_dark", { expectNickname });
    await emit("complete", { ok: true, accountAvatar: true, stageCount: count, expectNickname });
  }
  async function activeSwitch() {
    const source = "短信验证账号";
    const target = "邮箱验证账号";
    const test = (id) => one(`[data-testid="${id}"]`);
    const eligibility = () => test("quick-login-eligibility")?.dataset.quickEligibilityState;
    const phase = () => test("active-server-switch-panel")?.dataset.serverQuickPhase;
    async function check() {
      click(test("quick-login-eligibility-check"));
      await wait(() => eligibility() === "ready", "explicit active switch eligibility");
      assert(all('[data-quick-eligible="true"]').length === 2, "both saved records checked");
      assert(all('[data-testid="active-server-switch-open"]').length === 1, "only the other account can be switched to");
      assert(!test("server-quick-login-open"), "active picker has no guest exchange action");
      await settle();
    }
    async function enterConsent() {
      click(test("active-server-switch-open"));
      await wait(() => phase() === "consent", "active switch consent");
      const copy = test("active-server-switch-panel").textContent;
      assert(copy.includes(source) && copy.includes(target), "consent names both source and target");
      await settle();
    }
    async function exitAttempt() {
      click(test("active-server-switch-cancel"));
      await wait(() => !phase() && eligibility() === "idle", "returned to active picker");
      assert(one(".account-button").textContent.includes(source), "original account remains active");
      await settle();
    }

    await open();
    await email();
    await logged(target);
    await chooser();
    click(button("＋ 添加账号"));
    await wait(() => one(".login-methods"), "second synthetic account");
    await settle();
    await tab("手机");
    fill("登录手机号", "13800138000");
    click(button("密码登录"));
    await wait(
      () => all('[aria-label="登录密码"]').length && status() === "ready" && !one(".tab-leave-active"),
      "active source phone password ready",
    );
    await settle();
    fill("登录密码", "fixture-password");
    if (!one('[aria-label="同意登录协议"]').checked)
      click(one('[aria-label="同意登录协议"]'));
    click(one(".login-submit"));
    await logged(source);
    await chooser();
    assert(eligibility() === "idle" && !test("active-server-switch-open"), "picker did not query eligibility on entry");
    await stage("active_switch_idle_light", { expectNickname: source });

    await mode("active_switch_verify");
    await check();
    await stage("active_switch_eligible_light", { expectNickname: source });
    await enterConsent();
    await stage("active_switch_consent_light", { expectNickname: source });
    click(test("active-server-switch-consent"));
    await wait(() => phase() === "verification_required", "unsupported switch verification");
    assert(test("active-server-switch-alternate") && !test("active-server-switch-reconcile"), "unsupported switch does not replay");
    await stage("active_switch_verification_light", { expectNickname: source });
    await exitAttempt();

    await mode("active_switch_hold");
    await check();
    await enterConsent();
    click(test("active-server-switch-consent"));
    await wait(() => phase() === "exchanging", "switch request held");
    await wait(async () => {
      await mode("inspect");
      return window.__accountLoginPending === true;
    }, "native switch reached synthetic backend");
    await stage("active_switch_held_light", { expectNickname: source });
    await exitAttempt();
    await mode("release");
    await stage("active_switch_cancelled_light", { expectNickname: source });

    await theme("dark");
    await resize(880, 670);
    await mode("active_switch_interrupt");
    await check();
    await enterConsent();
    click(test("active-server-switch-consent"));
    await wait(() => phase() === "identity_required", "switch candidate has isolated receipt");
    assert(!test("active-server-switch-consent"), "no switch replay after lost body");
    await stage("active_switch_recovery_dark", { expectNickname: source });
    click(test("active-server-switch-reconcile"));
    await logged(target);
    await stage("active_switch_adopted_dark", { expectNickname: target });
    await emit("complete", { ok: true, activeSwitch: true, stageCount: count, expectNickname: target });
  }
  async function quickLogin() {
    const expectNickname = "邮箱验证账号";
    const test = (id) => one(`[data-testid="${id}"]`);
    const eligibility = () => test("quick-login-eligibility")?.dataset.quickEligibilityState;
    const phase = () => test("server-quick-login-panel")?.dataset.serverQuickPhase;
    async function check() {
      click(test("quick-login-eligibility-check"));
      await wait(() => eligibility() === "ready", "explicit quick eligibility");
      assert(one('[data-quick-eligible="true"]'), "synthetic account eligible");
      await settle();
    }
    async function enterConsent() {
      click(test("server-quick-login-open"));
      await wait(() => phase() === "consent", "explicit quick login consent");
      await settle();
    }
    await open();
    await email();
    await logged(expectNickname);
    await mode("quick_eligibility");
    await chooser();
    assert(eligibility() === "idle", "authenticated picker did not check automatically");
    await check();
    assert(!test("server-quick-login-open"), "authenticated picker has no server switch");
    await stage("quick_authenticated_eligibility_light", { expectNickname });
    // Sign out a different synthetic account: local logout clears only the
    // active store, leaving the target as genuine dormant saved history.
    click(button("＋ 添加账号"));
    await wait(() => one(".login-methods"), "alternate account login");
    await settle();
    await tab("手机");
    fill("登录手机号", "13800138000");
    click(button("密码登录"));
    await wait(
      () => all('[aria-label="登录密码"]').length && status() === "ready" && !one(".tab-leave-active"),
      "alternate phone password ready",
    );
    await settle();
    fill("登录密码", "fixture-password");
    if (!one('[aria-label="同意登录协议"]').checked)
      click(one('[aria-label="同意登录协议"]'));
    click(one(".login-submit"));
    await logged("短信验证账号");
    await chooser();
    click(button("退出登录"));
    await wait(() => one(".logout-option input"), "local logout confirmation");
    await settle();
    click(one(".logout-option input"));
    click(button("确认退出"));
    await wait(() => !one("dialog") && one(".welcome") && ready(), "local logout preserved history");
    await settle();

    await open();
    click(one(".login-accounts"));
    await wait(() => one(".saved-account") && eligibility() === "idle", "guest picker ready");
    await stage("quick_guest_idle_light", { expectGuest: true });
    await check();
    await stage("quick_guest_eligible_light", { expectGuest: true });
    await enterConsent();
    await stage("quick_guest_consent_light", { expectGuest: true });
    await mode("quick_verify");
    click(test("server-quick-login-consent"));
    await wait(() => phase() === "verification_required", "unsupported quick verification");
    assert(test("server-quick-login-alternate") && !test("server-quick-login-reconcile"), "unsupported challenge has alternate login only");
    await stage("quick_guest_verification_light", { expectGuest: true });
    click(test("server-quick-login-cancel"));
    await wait(() => !phase() && eligibility() === "idle", "previous attempt cancelled");
    await settle();

    await theme("dark");
    await resize(880, 670);
    await mode("quick_interrupt");
    await check();
    await enterConsent();
    click(test("server-quick-login-consent"));
    await wait(() => phase() === "identity_required", "interrupted body retained fresh receipt");
    assert(!test("server-quick-login-consent"), "no replay after interrupted reply");
    await stage("quick_guest_recovery_dark", { expectGuest: true });
    click(test("server-quick-login-reconcile"));
    await logged(expectNickname);
    await stage("quick_guest_adopted_dark", { expectNickname });
    await emit("complete", { ok: true, quickLogin: true, stageCount: count, expectNickname });
  }
  async function deviceRemoval() {
    const expectNickname = "邮箱验证账号";
    const test = (id) => one(`[data-testid="${id}"]`);
    const phase = () => one(".login-device-removal-dialog")?.dataset.removalPhase;
    const cards = () => all(".login-device-card", one(".login-devices-page"));
    const closeRemoval = async () => {
      click(test("login-device-removal-close"));
      await wait(() => !one(".login-device-removal-dialog"), "device removal closed");
      await settle();
    };
    async function begin(expected) {
      click(cards().find((card) => card.textContent.includes("旧设备")));
      await wait(() => test("login-device-remove-open"), "old-device detail");
      await settle();
      click(test("login-device-remove-open"));
      await wait(() => phase() === "confirm", "removal confirmation");
      await settle();
      click(test("login-device-remove-confirm"));
      await wait(() => phase() === expected, `device removal ${expected}`);
    }
    await open();
    await email();
    await logged();
    click(button("打开设置"));
    await wait(() => test("account-devices-open"), "device settings entry");
    await settle();
    click(test("account-devices-open"));
    await wait(() => cards().length === 2, "device list");
    await settle();
    click(cards().find((card) => card.textContent.includes("测试桌面")));
    await wait(() => one(".login-device-detail"), "current device detail");
    assert(!test("login-device-remove-open"), "current device has no removal action");
    await stage("device_removal_current_light", { expectNickname });
    click(button("关闭设备详情"));
    await wait(() => !one(".login-device-detail"), "current detail closed");
    await settle();

    await mode("device_removal_unbound");
    await begin("needs_binding");
    await settle();
    click(test("login-device-removal-binding"));
    await wait(() => one(".account-binding-qr-dialog svg[data-account-binding-qr]"), "binding QR");
    await stage("device_removal_binding_light", { expectNickname });
    click(button("关闭扫码设置"));
    await wait(() => !one(".account-binding-qr-dialog"), "binding QR closed");
    await settle();
    await closeRemoval();
    await wait(() => cards().length === 2, "fresh device handles");
    await settle();

    await mode("device_removal_sms");
    await begin("needs_sms");
    await stage("device_removal_sms_offer_light", { expectNickname });
    click(test("login-device-removal-send"));
    await wait(() => phase() === "sms_ready", "SMS acknowledgement");
    await stage("device_removal_sms_ready_light", { expectNickname });

    await theme("dark");
    await resize(880, 670);
    await mode("device_removal_drop");
    const code = test("login-device-removal-code");
    code.focus();
    code.value = "001234";
    code.dispatchEvent(new Event("input", { bubbles: true }));
    await wait(() => !test("login-device-removal-submit")?.disabled, "SMS submit enabled");
    click(test("login-device-removal-submit"));
    await wait(() => phase() === "recovery_required", "lost kickoff acknowledgement");
    assert(code.value === "" && !code.isConnected && !test("login-device-removal-code"), "OTP input scrubbed before removal");
    assert(!one(".login-device-removal-dialog").textContent.includes("001234"), "OTP not displayed after submit");
    await stage("device_removal_recovery_dark", { expectNickname });
    await closeRemoval();
    click(test("login-device-removal-resume"));
    await wait(() => phase() === "recovery_required", "recover saved outcome");
    await settle();
    click(test("login-device-removal-reconcile"));
    await wait(() => phase() === "observed_absent", "read-only absence observation");
    await wait(() => cards().length === 1, "fresh device list after observation");
    await stage("device_removal_observed_dark", { expectNickname });
    await closeRemoval();
    await wait(() => cards().length === 1 && test("login-device-removal-result"), "updated device list and result");
    assert(!one(".login-devices-page").innerHTML.includes("fixture-private-key"), "no server device key");
    await emit("complete", { ok: true, deviceRemoval: true, stageCount: count, expectNickname });
  }
  try {
    await wait(() => ready() && one(".welcome"), "guest bootstrap");
    await theme("light");
    await resize(1120, 760);
    if (window.__activeSwitchProof) {
      await activeSwitch();
      return;
    }
    if (window.__quickLoginProof) {
      await quickLogin();
      return;
    }
    if (window.__accountAvatarProof) {
      await accountAvatar();
      return;
    }
    if (window.__deviceRemovalProof) {
      await deviceRemoval();
      return;
    }
    if (window.__profileEditProof) {
      await profileEdit();
      return;
    }
    if (window.__accountDetailProof) {
      await accountDetail();
      return;
    }
    await open();
    await wait(() => one('.qr-frame svg[role="img"]'), "QR created");
    await stage("qr_waiting_light", { expectGuest: true });
    await tab("手机");
    await country("HK");
    fill("登录手机号", "51234567");
    click(one('.login-options input[type="checkbox"]'));
    const passwordSwitch = button("密码登录");
    const outgoing = passwordSwitch?.closest(".login-panel");
    assert(outgoing, "SMS panel before method switch");
    click(passwordSwitch);
    // Capture the original panel, even if reduced motion or a slow frame
    // means the animation class is never observable on the next tick.
    const immediate = {
      inert: outgoing.inert,
      secretLength:
        outgoing.querySelector('input[aria-label="短信验证码"]')?.value
          .length ?? 0,
    };
    await wait(
      () =>
        all('[aria-label="登录密码"]').length &&
        status() === "ready" &&
        !one(".tab-leave-active"),
      "password ready",
    );
    await settle();
    const retained =
      one('[aria-label="登录手机号"]').value === "51234567" &&
      one('[aria-label="国际区号"]').textContent.includes("+852");
    if (window.__accountLoginBaseline) {
      await stage("baseline_phone_switch", {
        expectGuest: true,
        retained,
        immediate,
      });
      await emit("complete", {
        ok: true,
        baseline: true,
        retained,
        immediate,
        stageCount: count,
      });
      return;
    }
    assert(retained, "phone and country survive SMS/password switch");
    assert(
      immediate.inert && immediate.secretLength === 0,
      "outgoing SMS immediately inert and cleared: " +
        JSON.stringify(immediate),
    );
    assert(
      !one('.login-options input[type="checkbox"]').checked,
      "remember choice survives switch",
    );
    await stage("phone_password_light", { expectGuest: true });
    const motionKey = "cumulus.motion.v1";
    const savedMotion = localStorage.getItem(motionKey);
    const motion = (value) => {
      if (value === null) localStorage.removeItem(motionKey);
      else localStorage.setItem(motionKey, value);
      dispatchEvent(new StorageEvent("storage", {
        key: motionKey, newValue: value, storageArea: localStorage,
      }));
    };
    motion("disabled");
    await wait(() => document.documentElement.dataset.motion === "reduced", "reduce motion");
    fill("登录密码", "fixture-password");
    const smsSwitch = button("验证码登录");
    const passwordPanel = smsSwitch?.closest(".login-panel");
    const password = passwordPanel?.querySelector('[aria-label="登录密码"]');
    assert(password?.value.length, "filled password before method switch");
    click(smsSwitch);
    assert(
      passwordPanel.inert && password.value.length === 0,
      "outgoing password panel immediately inert and cleared",
    );
    await wait(
      () =>
        all('[aria-label="登录手机号"]').length &&
        status() === "ready" &&
        !one(".tab-leave-active"),
      "SMS mode",
    );
    assert(!passwordPanel.isConnected, "outgoing panel removed with reduced motion");
    motion(savedMotion);
    await settle();
    assert(
      one('[aria-label="登录手机号"]').value === "51234567" &&
        !one('[aria-label="短信验证码"]'),
      "identifier retained and OTP discarded",
    );
    await stage("phone_sms_light", { expectGuest: true });
    await tab("手机");
    assert(
      one('[aria-label="登录手机号"]').value === "51234567",
      "active method does not restart form",
    );
    await theme("dark");
    await resize(760, 540);
    await stage("phone_sms_small_dark", { expectGuest: true });
    await country("CN");
    fill("登录手机号", "13800138000");
    click(one('[aria-label="同意登录协议"]'));
    click(one(".login-submit"));
    await wait(() => one('[data-phone-phase="code_required"]'), "SMS sent once");
    await settle();
    await stage("sms_cooldown_small_dark", { expectGuest: true });
    fill("短信验证码", "123456");
    const outgoingCode = one('[aria-label="短信验证码"]');
    click(button("密码登录"));
    await sleep(0);
    assert(outgoingCode.value === "", "outgoing OTP cleared before animation");
    await wait(
      () =>
        all('[aria-label="登录密码"]').length &&
        status() === "ready" &&
        !one(".tab-leave-active"),
      "password mode",
    );
    await settle();
    click(button("验证码登录"));
    // A password panel also has the phone field and ready status. Yield its
    // Vue patch, then require the new SMS panel and native reply together.
    await sleep(0);
    await wait(
      () => {
        const panel = all(".phone-sms-login")[0];
        if (!panel || panel.dataset.phonePhase !== "number") return false;
        const input = all('[aria-label="登录手机号"]', panel)[0];
        const submit = all(".login-submit", panel)[0];
        return (
          status() === "ready" &&
          input &&
          !input.disabled &&
          submit &&
          !submit.disabled &&
          !one(
            ".tab-enter-active,.tab-leave-active,.reuse-step-enter-active,.reuse-step-leave-active",
          )
        );
      },
      "SMS return ready with enabled controls",
    );
    await settle();
    click(all(".phone-sms-login .login-submit")[0]);
    await wait(() => one('[data-phone-phase="code_required"]'), "same phone code page restored");
    await settle();
    assert(
      button("获取验证码") === undefined &&
        /秒后重发/.test(one(".login-code button").textContent) &&
        one(".login-code button").disabled,
      "native shared cooldown survives mode switch",
    );
    await stage("sms_cooldown_return_dark", { expectGuest: true });
    await email("alice@example.test", "wrong-password");
    await wait(() => status() === "rejected", "rejected password");
    await stage("email_rejected_small_dark", { expectGuest: true });
    assert(
      one('[aria-label="登录密码"]').value === "",
      "submitted password cleared",
    );
    await mode("login_hold");
    fill("登录密码", "fixture-password");
    if (!one('[aria-label="同意登录协议"]').checked)
      click(one('[aria-label="同意登录协议"]'));
    click(one(".login-submit"));
    await sleep(120);
    await stage("email_inflight_small_dark", { expectGuest: true });
    click(button("手机", one(".login-methods")));
    await mode("release");
    await wait(
      () => status() === "ready" && !one(".tab-leave-active"),
      "replacement login ready",
    );
    await settle();
    await sleep(250);
    assert(
      one(".login-methods") && status() === "ready",
      "late login cannot close replacement",
    );
    await stage("late_login_cancelled_dark", { expectGuest: true });
    await tab("邮箱");
    assert(
      one('[aria-label="登录邮箱"]').value === "alice@example.test" &&
        one('[aria-label="登录密码"]').value === "",
      "email retained only within dialog, secret cleared",
    );
    await closeLogin();
    await stage("closed_returns_focus_dark", { expectGuest: true });
    const qrError = () => one(".qr-frame .qr-error [role='alert']")?.textContent.trim() ===
      "服务端异常，请稍后再试" && button("点击重试")?.closest(".qr-error") &&
      !one('.qr-frame svg[role="img"]');
    await mode("qr_error");
    await open();
    await wait(qrError, "QR creation service error in the original QR frame");
    await stage("qr_error_small_dark", { expectGuest: true });
    await sleep(1150);
    await stage("qr_error_still_small_dark", { expectGuest: true });
    await mode("qr_poll_error");
    const createRetry = button("点击重试");
    click(createRetry);
    createRetry.click(); // Same event turn: the request guard rejects the duplicate.
    await wait(() => one('.qr-frame svg[role="img"]'), "explicit QR retry");
    await wait(qrError, "unknown QR status shows the same service error");
    assert(document.activeElement === button("点击重试"), "retry focus returns after the QR stops");
    await stage("qr_poll_error_small_dark", { expectGuest: true });
    await mode("normal");
    click(button("点击重试"));
    await wait(() => one('.qr-frame svg[role="img"]'), "QR retry");
    await stage("qr_retry_small_dark", { expectGuest: true });
    await mode("qr_scanned");
    await wait(
      () => one(".qr-confirmation")?.textContent.includes("手机上确认"),
      "QR scanned",
    );
    assert(
      !one('.qr-frame svg[role="img"]'),
      "scanned QR replaced by confirmation",
    );
    await stage("qr_scanned_small_dark", { expectGuest: true });
    await mode("qr_expired");
    await wait(
      () => one(".qr-expired [role='status']")?.textContent === "二维码已失效" &&
        button("点击重试")?.closest(".qr-frame") && !one('.qr-frame svg[role="img"]'),
      "QR expired",
    );
    await stage("qr_expired_small_dark", { expectGuest: true });
    await mode("normal");
    click(button("点击重试"));
    await wait(() => one('.qr-frame svg[role="img"]'), "QR regenerated");
    await theme("light");
    await stage("qr_retry_small_light", { expectGuest: true });
    await tab("邮箱");
    assert(
      one('[aria-label="登录邮箱"]').value === "",
      "closed dialog discards identifier",
    );
    await mode("bootstrap_slow");
    await email();
    await logged();
    assert(document.activeElement === one(".account-button"), "login focus restored after delayed resource initialization");
    await mode("normal");
    await stage("email_authenticated_light", {
      expectNickname: "邮箱验证账号",
    });
    await chooser();
    await stage("saved_current_small_light", {
      expectNickname: "邮箱验证账号",
    });
    click(button("退出登录"));
    await stage("logout_confirm_small_light", {
      expectNickname: "邮箱验证账号",
    });
    await mode("logout_error");
    click(button("确认退出"));
    await wait(
      () => one('.account-dialog [role="alert"]'),
      "remote logout error",
    );
    await stage("logout_failed_small_light", {
      expectNickname: "邮箱验证账号",
    });
    const checkbox = one(".logout-option input");
    assert(checkbox.checked, "failed remote logout keeps choice");
    click(checkbox);
    click(button("确认退出"));
    await wait(
      () => !one("dialog") && one(".welcome") && ready(),
      "local logout",
    );
    await settle();
    await stage("local_logout_light", { expectGuest: true });
    await mode("qr_identity_recovery");
    await open();
    await wait(
      () => one('.qr-frame svg[role="img"]'),
      "QR ready for authorization",
    );
    const checkQrIdentity = () => button("核对登录状态");
    await wait(() => one('[data-qr-recovery="identity"]') && checkQrIdentity(), "acknowledged QR awaiting identity");
    assert(!one(".qr-frame svg") && !button("点击重试") && !button("重新生成二维码"), "acknowledged QR cannot be replayed");
    checkQrIdentity().focus();
    await stage("qr_identity_pending_light", { expectGuest: true });
    click(checkQrIdentity());
    await wait(() => one('.login-status[role="alert"]') && checkQrIdentity(), "first identity retry failed");
    await stage("qr_identity_retry_light", { expectGuest: true });
    click(checkQrIdentity());
    await logged();
    await mode("normal");
    await stage("qr_authenticated_light", { expectNickname: "邮箱验证账号" });
    await theme("dark");
    await resize(1120, 760);
    await chooser();
    await stage("saved_current_dark", { expectNickname: "邮箱验证账号" });
    click(button("退出登录"));
    await settle();
    click(button("取消"));
    await settle();
    assert(
      document.activeElement === button("退出登录"),
      "confirmation restores opener",
    );
    await stage("logout_cancelled_dark", { expectNickname: "邮箱验证账号" });
    click(button("关闭账号窗口"));
    await wait(() => !one("dialog"), "chooser closed");
    await settle();
    await stage("account_closed_dark", { expectNickname: "邮箱验证账号" });
    await mode("qr_error");
    await chooser();
    click(button("＋ 添加账号"));
    await wait(qrError, "failed QR attempt never leaves the signed-in account pending");
    await stage("qr_error_keeps_current_dark", { expectNickname: "邮箱验证账号" });
    await mode("normal");
    await closeLogin();
    await chooser();
    click(button("＋ 添加账号"));
    await wait(() => one(".login-methods"), "add phone account");
    await settle();
    await tab("手机");
    fill("登录手机号", "13800138000");
    click(button("密码登录"));
    await wait(
      () =>
        all('[aria-label="登录密码"]').length &&
        status() === "ready" &&
        !one(".tab-leave-active"),
      "phone password ready",
    );
    await settle();
    fill("登录密码", "fixture-password");
    if (!one('[aria-label="同意登录协议"]').checked)
      click(one('[aria-label="同意登录协议"]'));
    click(one(".login-submit"));
    await logged("短信验证账号");
    await stage("phone_password_authenticated_dark", {
      expectNickname: "短信验证账号",
    });
    await chooser();
    await stage("saved_two_accounts_dark", { expectNickname: "短信验证账号" });
    click(
      all(".saved-account").find((node) =>
        node.textContent.includes("邮箱验证账号"),
      ),
    );
    await logged();
    await stage("saved_switched_dark", { expectNickname: "邮箱验证账号" });
    await chooser();
    click(button("＋ 添加账号"));
    await wait(() => one(".login-methods"), "add SMS account");
    await settle();
    await tab("手机");
    fill("登录手机号", "13800138000");
    click(one('[aria-label="同意登录协议"]'));
    click(one(".login-submit"));
    await wait(() => one('[data-phone-phase="code_required"]'), "SMS code page");
    await settle();
    fill("短信验证码", "123456");
    click(one('.login-options input[type="checkbox"]'));
    if (!one('[aria-label="同意登录协议"]').checked)
      click(one('[aria-label="同意登录协议"]'));
    click(one(".login-submit"));
    await logged("短信验证账号");
    await chooser();
    assert(
      all(".saved-account").some(
        (node) =>
          node.dataset.current === "true" &&
          node.textContent.includes("未开启自动登录"),
      ),
      "current account exposes disabled auto login",
    );
    await stage("sms_authenticated_dark", { expectNickname: "短信验证账号" });
    await emit("complete", {
      ok: true,
      accountLogin: true,
      stageCount: count,
      expectNickname: "短信验证账号",
    });
  } catch (error) {
    await emit("failed", {
      ok: false,
      last,
      error: error instanceof Error ? error.message : "unknown",
      stack: error instanceof Error ? error.stack?.slice(0, 4096) ?? null : null,
      status: status() ?? null,
      stageCount: count,
      body: document.body.innerText.slice(-1800),
    });
  }
})();
