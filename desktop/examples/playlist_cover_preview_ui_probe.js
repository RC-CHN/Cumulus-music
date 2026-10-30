// Real persisted-image failures and production UI/IPC. Only response timing is held.
(async () => {
  const one = (s) => document.querySelector(s);
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const assert = (v, message) => {
    if (!v) throw Error(message);
  };
  const wait = async (f, message) => {
    for (let n = 0; n < 180; n++) {
      if (f()) return;
      await sleep(100);
    }
    throw Error(message);
  };
  const button = (label, root = document) =>
    [...root.querySelectorAll("button")].find(
      (n) =>
        n.getAttribute("aria-label") === label ||
        n.textContent.trim() === label,
    );
  const click = (n) => {
    assert(n && !n.disabled, "unavailable action");
    n.focus({ preventScroll: true });
    n.click();
  };
  const emit = (stage, data = {}) =>
    window.__TAURI_INTERNALS__.invoke("plugin:event|emit", {
      event: "ux-proof",
      payload: { stage, ...data },
    });
  const dialog = () => one(".cover-dialog");
  const hit = (n) => {
    const r = n.getBoundingClientRect(),
      at = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
    return at === n || n.contains(at);
  };
  const write = (n, value) => {
    n.value = value;
    n.dispatchEvent(new Event("input", { bubbles: true }));
  };
  let count = 0,
    reads = 0,
    gate,
    release,
    held = false,
    originalName;
  const checks = [];
  const originalFetch = window.fetch;
  window.fetch = async function (input, options) {
    const url = typeof input === "string" ? input : input.url;
    if (new URL(url, location.href).pathname !== "/playlist_cover_image")
      return originalFetch.call(window, input, options);
    reads++;
    const delay = gate;
    // Preserve the actual native response and headers; delay only its delivery.
    const response = await originalFetch.call(window, input, options);
    if (delay) {
      held = true;
      await delay;
    }
    return response;
  };
  function hold() {
    held = false;
    gate = new Promise((resolve) => {
      const timer = setTimeout(resolve, 10000);
      release = () => {
        clearTimeout(timer);
        gate = undefined;
        resolve();
      };
    });
  }
  async function fixture(kind) {
    window.__coverTransferAck = null;
    await emit("cover-transfer-fixture", { kind });
    await wait(() => window.__coverTransferAck, "fixture acknowledgment");
    assert(
      window.__coverTransferAck.ok,
      JSON.stringify(window.__coverTransferAck),
    );
  }
  async function capture(label) {
    await wait(
      () =>
        !dialog() ||
        (dialog()
          .getAnimations()
          .every((a) => a.playState !== "running" && !a.pending) &&
          getComputedStyle(dialog()).transform === "none"),
      "animation settled",
    );
    await sleep(150);
    const d = dialog(),
      body = d?.querySelector(".cover-body"),
      r = d?.getBoundingClientRect();
    const state = {
      label,
      contained:
        !d || (r.width === 442 && r.y >= 11 && r.bottom <= innerHeight - 11),
      noOverflow:
        document.documentElement.scrollWidth <= innerWidth &&
        (!body || body.scrollWidth <= body.clientWidth),
      footerHit: !d || [...d.querySelectorAll("footer button")].every(hit),
      focus: document.activeElement?.getAttribute("aria-label"),
    };
    assert(
      state.contained && state.noOverflow && state.footerHit,
      JSON.stringify(state),
    );
    const placeholder = d?.querySelector(".cover-empty");
    if (placeholder) {
      const p = placeholder.getBoundingClientRect(),
        b = body.getBoundingClientRect();
      assert(
        p.top >= b.top && p.bottom <= b.bottom,
        "whole placeholder visible with recovery controls",
      );
    }
    if (d && innerHeight <= 600) {
      const footer = d.querySelector("footer").getBoundingClientRect();
      assert(
        Math.abs(footer.bottom - (r.bottom - 40)) < 0.01,
        "footer remains at its padded bottom during retry",
      );
    }
    checks.push(state);
    await emit(`ux_${String(++count).padStart(2, "0")}_${label}`, {
      checks: state,
      text: body?.textContent,
      reads,
    });
    await sleep(800);
  }
  async function openBroken() {
    await fixture("seed");
    await fixture("break_preview");
    click(button("替换歌单封面"));
    await wait(
      () => dialog()?.querySelector(".error-state"),
      "visible preview failure",
    );
    assert(!one(".cover-crop-image"), "no invalid image");
    assert(
      dialog().textContent.includes("暂时无法显示所选图片"),
      "existing image failure wording",
    );
    assert(button("保存", dialog()).disabled, "no save with invalid preview");
    assert(
      hit(button("重新读取预览", dialog())),
      "retry visible in short window",
    );
  }
  async function pending() {
    hold();
    const before = reads;
    click(button("重新读取预览", dialog()));
    await wait(
      () => held && button("正在读取预览…", dialog())?.disabled,
      "visible loading",
    );
    assert(
      !one(".cover-dialog .error-state"),
      "previous preview error cleared during retry",
    );
    assert(
      button("重新选取图片", dialog()).disabled &&
        button("保存", dialog()).disabled,
      "conflicting actions disabled",
    );
    button("正在读取预览…", dialog()).click();
    await sleep(100);
    assert(reads === before + 1, "one retry per click sequence");
  }
  try {
    await wait(() => one(".track-row"), "startup");
    for (const [width, height, theme] of [
      [1120, 760, "light"],
      [760, 540, "light"],
      [760, 540, "dark"],
      [1120, 760, "dark"],
    ]) {
      const suffix = width + "_" + theme;
      await emit("resize", { width, height });
      await wait(
        () => innerWidth === width && innerHeight === height,
        "resize",
      );
      click(button("外观主题"));
      await wait(() => one(".action-menu"), "theme menu");
      click(button(theme === "dark" ? "深色" : "浅色"));
      await wait(
        () =>
          !one(".action-menu") &&
          document.documentElement.dataset.theme === theme,
        "theme applied",
      );
      click(button("我的歌单"));
      await wait(() => button("管理歌单：合成歌单 3"), "library");
      click(button("管理歌单：合成歌单 3"));
      await wait(() => one(".action-menu"), "playlist menu");
      click(button("编辑歌单", one(".action-menu")));
      await wait(() => one('[aria-label="歌单名称"]')?.value, "editor");
      originalName = one('[aria-label="歌单名称"]').value;
      write(one('[aria-label="歌单名称"]'), "预览重试期间保留的草稿");
      await openBroken();
      await capture("failed_" + suffix);
      await pending();
      await capture("retry_failure_loading_" + suffix);
      release();
      await wait(
        () =>
          one(".cover-dialog .error-state") &&
          !button("重新读取预览", dialog())?.disabled,
        "repeat failure",
      );
      await capture("failed_again_" + suffix);
      await fixture("restore_preview");
      await pending();
      await capture("retry_success_loading_" + suffix);
      release();
      await wait(
        () =>
          one(".cover-crop-image")?.naturalWidth > 0 &&
          !button("保存", dialog())?.disabled,
        "preview recovered",
      );
      assert(
        !one(".cover-dialog .error-state"),
        "retry success removes old error",
      );
      assert(
        document.activeElement === one(".cover-crop-area"),
        "retry returns keyboard focus to crop",
      );
      assert(one(".cover-body").scrollTop === 0, "crop visible after recovery");
      await capture("recovered_" + suffix);
      click(button("关闭封面编辑", dialog()));
      await wait(() => !dialog(), "close");
      await openBroken();
      await fixture("restore_preview");
      await pending();
      click(button("关闭封面编辑", dialog()));
      await wait(() => !dialog(), "close while preview waits");
      await capture("closed_pending_" + suffix);
      release();
      await sleep(250);
      assert(
        !dialog() && document.activeElement === button("替换歌单封面"),
        "late preview does not reopen or steal focus",
      );
      assert(
        one('[aria-label="歌单名称"]').value === "预览重试期间保留的草稿",
        "text draft preserved",
      );
      await capture("closed_after_late_" + suffix);
      write(one('[aria-label="歌单名称"]'), originalName);
      click(button("取消编辑歌单"));
      await wait(() => !one(".playlist-editor-page"), "leave editor");
    }
    await emit("complete", {
      ok: true,
      stageCount: count,
      checks,
      imageReads: reads,
      responseTimingInjected: true,
      realMusicRequests: 0,
      realAccountWrites: 0,
      officialPixelMatchVerified: false,
    });
  } catch (error) {
    release?.();
    await emit("ux_failed", {
      message: String(error),
      text: dialog()?.textContent,
    });
    await sleep(850);
    await emit("failed", {
      message: String(error),
      stack: error?.stack,
      stageCount: count,
    });
  } finally {
    window.fetch = originalFetch;
  }
})();
