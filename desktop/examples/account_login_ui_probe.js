// Production DOM actions, native IPC and synthetic HTTP. No credential/session dumps.
(async () => {
  const invoke = (command, args = {}) =>
    window.__TAURI_INTERNALS__.invoke(command, args);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (value, message) => {
    if (!value) throw Error(message);
  };
  const one = (selector) => document.querySelector(selector);
  const all = (selector) =>
    [...document.querySelectorAll(selector)].filter(
      (node) => !node.closest(".tab-leave-active,.modal-leave-active"),
    );
  const button = (text, root = document) =>
    [...root.querySelectorAll("button")].find(
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
    const dialog = one("dialog[open]"),
      focus = document.activeElement;
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
  async function email(
    address = "alice@example.test",
    password = "fixture-password",
  ) {
    await tab("邮箱");
    fill("登录邮箱", address);
    fill("登录密码", password);
    click(one(".login-submit"));
  }
  try {
    await wait(() => ready() && one(".welcome"), "guest bootstrap");
    await theme("light");
    await resize(1120, 760);
    await open();
    await wait(() => one('.qr-frame svg[role="img"]'), "QR created");
    await stage("qr_waiting_light", { expectGuest: true });
    await tab("手机");
    fill("国际区号", "852");
    fill("登录手机号", "51234567");
    fill("短信验证码", "123456");
    click(one('.login-options input[type="checkbox"]'));
    click(button("密码登录"));
    await sleep(0);
    const outgoing = one(".tab-leave-active");
    const immediate = {
      inert: outgoing?.inert ?? null,
      secretLength:
        outgoing?.querySelector('input[aria-label="短信验证码"]')?.value
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
      one('[aria-label="国际区号"]').value === "852";
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
    fill("登录密码", "fixture-password");
    click(button("验证码登录"));
    await sleep(0);
    assert(
      one(".tab-leave-active")?.querySelector('[aria-label="登录密码"]')?.value
        .length === 0,
      "outgoing password cleared before animation",
    );
    await wait(
      () =>
        all('[aria-label="短信验证码"]').length &&
        status() === "ready" &&
        !one(".tab-leave-active"),
      "SMS mode",
    );
    await settle();
    assert(
      one('[aria-label="登录手机号"]').value === "51234567" &&
        one('[aria-label="短信验证码"]').value === "",
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
    fill("国际区号", "86");
    fill("登录手机号", "13800138000");
    click(button("获取验证码"));
    await wait(() => status() === "code_sent", "SMS sent once");
    await stage("sms_cooldown_small_dark", { expectGuest: true });
    click(button("密码登录"));
    await wait(
      () =>
        all('[aria-label="登录密码"]').length &&
        status() === "ready" &&
        !one(".tab-leave-active"),
      "password mode",
    );
    await settle();
    click(button("验证码登录"));
    await wait(
      () =>
        all('[aria-label="短信验证码"]').length &&
        status() === "ready" &&
        !one(".tab-leave-active"),
      "SMS return",
    );
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
    await mode("qr_error");
    await open();
    await wait(
      () => one(".login-status")?.textContent.includes("失败"),
      "QR creation failure",
    );
    await stage("qr_error_small_dark", { expectGuest: true });
    await mode("normal");
    click(button("重新生成二维码"));
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
      () => one(".login-status")?.textContent.includes("已过期"),
      "QR expired",
    );
    await stage("qr_expired_small_dark", { expectGuest: true });
    await mode("normal");
    click(button("重新生成二维码"));
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
    await mode("qr_authorized");
    await open();
    await wait(
      () => one('.qr-frame svg[role="img"]'),
      "QR ready for authorization",
    );
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
    fill("短信验证码", "123456");
    click(one('.login-options input[type="checkbox"]'));
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
      status: status() ?? null,
      stageCount: count,
      body: document.body.innerText.slice(-1800),
    });
  }
})();
