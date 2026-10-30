// Production WebView/IPC, isolated Rust account fixture; no real account writes.
(async () => {
  const one = (s) => document.querySelector(s),
    all = (s) => [...document.querySelectorAll(s)];
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const assert = (v, m) => {
    if (!v) throw Error(m);
  };
  const wait = async (fn, m) => {
    for (let i = 0; i < 200; i++) {
      if (fn()) return;
      await sleep(50);
    }
    throw Error(m);
  };
  const emit = (stage, data = {}) =>
    window.__TAURI_INTERNALS__.invoke("plugin:event|emit", {
      event: "ux-proof",
      payload: { stage, ...data },
    });
  const button = (label, root = document) =>
    [...root.querySelectorAll("button")].find(
      (n) =>
        n.getAttribute("aria-label") === label ||
        n.textContent.trim() === label,
    );
  const click = (n) => {
    assert(n && !n.disabled, "enabled control");
    n.focus({ preventScroll: true });
    n.click();
  };
  const key = (n, k) =>
    n.dispatchEvent(
      new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true }),
    );
  const fixture = async (mode) => {
    window.__collectFixtureAck = null;
    await emit("collect-fixture", { mode });
    await wait(() => window.__collectFixtureAck, "fixture");
    return window.__collectFixtureAck;
  };
  const dialog = () => one(".playlist-collect-dialog[open]");
  const list = () => one(".collect-list");
  let step = "startup",
    count = 0,
    maxRows = 0,
    listCalls = 0;
  const originalFetch = window.fetch;
  window.fetch = async function (url, options) {
    if (String(url).endsWith("/account_playlists")) listCalls++;
    return originalFetch.call(window, url, options);
  };
  function input(value) {
    const n = one('[aria-label="歌单名称"]');
    n.value = value;
    n.dispatchEvent(new Event("input", { bubbles: true }));
  }
  async function capture(label) {
    step = label;
    await sleep(200);
    const d = dialog(),
      r = d?.getBoundingClientRect();
    assert(
      document.documentElement.scrollWidth <= innerWidth,
      "page containment",
    );
    if (d) {
      assert(
        r.left >= 0 &&
          r.right <= innerWidth &&
          r.top >= 0 &&
          r.bottom <= innerHeight,
        "dialog containment",
      );
      assert(d.scrollWidth <= d.clientWidth, "dialog horizontal containment");
      const close = button("关闭歌单操作", d).getBoundingClientRect();
      assert(close.top >= r.top && close.bottom <= r.bottom, "close visible");
      if (list()) {
        assert(Math.round(r.width) === 442, "official dialog width");
        assert(all(".collect-row").length <= 23, "bounded rows");
        maxRows = Math.max(maxRows, all(".collect-row").length);
        for (const row of all(".collect-row")) {
          assert(
            Math.round(row.getBoundingClientRect().height) === 64,
            "official row height",
          );
          assert(
            Math.round(
              row.querySelector(".collect-cover").getBoundingClientRect().width,
            ) === 48,
            "official cover width",
          );
        }
      }
    }
    await emit(`ux_${String(++count).padStart(2, "0")}_${label}`, {
      width: innerWidth,
      height: innerHeight,
      theme: document.documentElement.dataset.theme,
      rows: all(".collect-row").length,
      dialog: !!d,
    });
    await sleep(420);
  }
  async function open(index = 0) {
    const trigger = all(".track-row .track-more")[index];
    trigger.scrollIntoView({ block: "nearest" });
    await sleep(80);
    click(trigger);
    await wait(
      () => one(".song-action-menu")?.style.visibility === "visible",
      "menu",
    );
    click(button("收藏", one(".song-action-menu")));
    await wait(() => dialog() && list(), "collector");
    await sleep(220);
    return trigger;
  }
  async function close(trigger) {
    click(button("关闭歌单操作", dialog()));
    await wait(() => !one(".playlist-collect-dialog"), "closed");
    assert(
      document.activeElement === trigger,
      "focus returns to invoking song",
    );
  }
  async function create() {
    list().scrollTop = 0;
    await sleep(90);
    click(button("创建新歌单", dialog()));
    await wait(
      () =>
        one(".collect-create") &&
        !one(".collect-step-enter-active,.collect-step-leave-active"),
      "create form",
    );
    assert(
      document.activeElement === one('[aria-label="歌单名称"]'),
      "create focuses input",
    );
  }
  try {
    await wait(() => one(".track-more"), "daily");
    for (const [w, h, theme] of [
      [1120, 760, "light"],
      [1120, 760, "dark"],
      [760, 540, "light"],
      [760, 540, "dark"],
    ]) {
      step = `setup_${w}_${theme}`;
      await emit("resize", { width: w, height: h });
      await wait(() => innerWidth === w && innerHeight === h, "resize");
      click(button("外观主题"));
      await wait(() => one(".action-menu"), "theme menu");
      click(button(theme === "dark" ? "深色" : "浅色"));
      await wait(() => !one(".action-menu"), "theme applied");
      const trigger = await open();
      await wait(() => !one(".collect-load"), "list read");
      await capture(`list_${w}_${theme}`);
      key(list(), "End");
      await wait(
        () => document.activeElement?.dataset.playlistId === "19",
        "End reaches final owned list",
      );
      await capture(`bottom_${w}_${theme}`);
      key(document.activeElement, "Home");
      await wait(
        () =>
          document.activeElement?.getAttribute("aria-label") === "创建新歌单",
        "Home create",
      );
      await create();
      assert(button("创建并收藏", dialog()).disabled, "empty name disabled");
      input("用来检查名称长度的合成歌单名称不能超出当前上限");
      assert(button("创建并收藏", dialog()).disabled, "long name disabled");
      await capture(`create_${w}_${theme}`);
      input("保留的草稿");
      click(button("返回收藏歌单列表", dialog()));
      await wait(
        () =>
          list() &&
          !one(".collect-step-enter-active,.collect-step-leave-active"),
        "back list",
      );
      await create();
      assert(
        one('[aria-label="歌单名称"]').value === "保留的草稿",
        "draft preserved",
      );
      assert(one('[aria-label="设为私密歌单"]').checked, "private default");
      await capture(`draft_${w}_${theme}`);
      await close(trigger);
    }
    await fixture("many");
    let trigger = await open();
    await wait(() => !one(".collect-load"), "many list");
    key(list(), "End");
    await wait(
      () => document.activeElement?.dataset.playlistId === "260",
      "virtual keyboard end",
    );
    await capture("many_bottom");
    key(document.activeElement, "Home");
    await wait(
      () => document.activeElement?.getAttribute("aria-label") === "创建新歌单",
      "virtual keyboard home",
    );
    await capture("many_top");
    await close(trigger);
    await fixture("empty");
    trigger = await open();
    await wait(() => one(".collect-empty"), "empty");
    await capture("empty");
    await close(trigger);
    await fixture("fail_list");
    trigger = await open();
    await wait(() => button("重试", dialog()), "read failure");
    await capture("load_error");
    await fixture("hold_list");
    const before = listCalls;
    const retry = button("重试", dialog());
    click(retry);
    click(retry);
    await capture("load_retry_wait");
    assert(listCalls === before + 1, "one retry while pending");
    await fixture("success");
    await wait(() => !one(".collect-load"), "read recovered");
    assert(document.activeElement === list(), "retry success retains focus");
    await capture("load_recovered");
    await close(trigger);
    await fixture("hold_add");
    trigger = await open();
    const target = () => one('[data-playlist-id="2"]');
    click(target());
    click(target());
    await wait(
      () => dialog()?.getAttribute("aria-busy") === "true",
      "collect pending",
    );
    dialog().dispatchEvent(new Event("cancel", { cancelable: true }));
    assert(dialog(), "busy Escape retained");
    await capture("collect_pending");
    await fixture("success");
    await wait(() => !dialog(), "collect saved");
    await capture("collect_saved");
    let facts = await fixture("success");
    assert(facts.writes === 1, "single add");
    trigger = await open();
    await create();
    input("界面验证 · 创建后重试");
    await fixture("reject_add");
    click(button("创建并收藏", dialog()));
    await wait(
      () =>
        button("继续收藏", dialog()) &&
        dialog()?.getAttribute("aria-busy") === "false",
      "create succeeded add failed",
    );
    assert(
      one(".daily-heading-copy"),
      "create collector stays on original page",
    );
    facts = await fixture("success");
    assert(facts.creates === 1, "one creation");
    assert(
      document.activeElement === button("继续收藏", dialog()),
      "failed add retains retry focus",
    );
    await capture("created_add_failed");
    click(button("继续收藏", dialog()));
    await wait(() => !dialog(), "existing creation retry saved");
    facts = await fixture("success");
    assert(
      facts.creates === 1 && facts.writes === 3,
      "retry adds without second creation",
    );
    await capture("created_add_recovered");
    trigger = await open(1);
    await fixture("uncertain");
    click(target());
    await wait(
      () =>
        button("核对结果", dialog()) &&
        dialog()?.getAttribute("aria-busy") === "false",
      "uncertain visible",
    );
    assert(
      target().getAttribute("aria-disabled") === "true",
      "uncertain prevents another write",
    );
    await capture("uncertain");
    facts = await fixture("success");
    click(button("核对结果", dialog()));
    await wait(() => !dialog(), "reconcile saved");
    const after = await fixture("success");
    assert(after.writes === facts.writes, "reconcile read only");
    await capture("reconciled");
    await emit("complete", {
      ok: true,
      collectionUi: true,
      stageCount: count,
      maxRows,
      realMusicRequests: 0,
      realAccountWrites: 0,
      creationRetryVerified: true,
      uncertainReadbackVerified: true,
    });
  } catch (error) {
    await emit("failed", {
      step,
      message: String(error),
      count,
      active: document.activeElement?.outerHTML?.slice(0, 250),
      dialog: dialog()?.textContent?.slice(0, 800),
    });
  }
})();
