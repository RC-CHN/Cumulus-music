// Production WebView/IPC/Rust/SQLite; only the service and legacy DTO loss are synthetic.
(async () => {
  const invoke = (name, args = {}) =>
      window.__TAURI_INTERNALS__.invoke(name, args),
    sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const assert = (v, m) => {
      if (!v) throw Error(m);
    },
    wait = async (fn, label) => {
      for (let i = 0; i < 600; i++) {
        if (await fn()) return;
        await sleep(50);
      }
      throw Error(label);
    };
  const emit = (v) =>
    invoke("plugin:event|emit", { event: "mini-proof", payload: v });
  window.__miniAck = {};
  let serial = 0,
    count = 0,
    last = "startup",
    epoch,
    legacy = false,
    client,
    originalRead;
  async function control(kind, data = {}) {
    const id = ++serial;
    await emit({ kind, id, ...data });
    await wait(() => id in window.__miniAck, kind);
    const result = window.__miniAck[id];
    delete window.__miniAck[id];
    assert(!result.error, result.error);
    return result.value ?? result;
  }
  const page = () => document.querySelector(".cloud-page"),
    batch = () =>
      document.querySelector(".cloud-batch-page:not(.page-leave-active)"),
    dialog = () =>
      document.querySelector("dialog[open]:not(.modal-leave-active)");
  const rect = (n) => {
    const r = n.getBoundingClientRect();
    return {
      left: r.left,
      right: r.right,
      top: r.top,
      bottom: r.bottom,
      width: r.width,
      height: r.height,
    };
  };
  const ready = () =>
    page()?.dataset.cloudLoading === "false" &&
    page()?.dataset.cloudReady === "true";
  async function settle() {
    await wait(
      () =>
        !document.querySelector(
          ".page-enter-active,.page-leave-active,.modal-enter-active,.modal-leave-active,.menu-enter-active,.menu-leave-active",
        ),
      "transitions",
    );
    await new Promise((r) =>
      requestAnimationFrame(() => requestAnimationFrame(r)),
    );
  }
  function button(label, scope = dialog() ?? batch() ?? page()) {
    return [...scope.querySelectorAll("button")].find(
      (n) =>
        n.textContent.trim() === label ||
        n.getAttribute("aria-label") === label,
    );
  }
  function click(label, scope) {
    const n = button(label, scope);
    assert(n && !n.disabled, "enabled " + label);
    n.scrollIntoView({ block: "nearest" });
    n.focus({ preventScroll: true });
    n.click();
    return n;
  }
  async function theme(value) {
    localStorage.setItem("cumulus.appearance.v1", value);
    window.dispatchEvent(
      new StorageEvent("storage", {
        key: "cumulus.appearance.v1",
        newValue: value,
      }),
    );
    await wait(() => document.documentElement.dataset.theme === value, "theme");
  }
  async function resize(width, height) {
    await control("report-resize", { width, height, scale: devicePixelRatio });
    await wait(() => innerWidth === width && innerHeight === height, "resize");
  }
  async function search(value) {
    const n = page().querySelector('[aria-label="搜索云盘歌曲"]');
    n.focus();
    n.value = value;
    n.dispatchEvent(new Event("input", { bubbles: true }));
    n.form.dispatchEvent(
      new Event("submit", { bubbles: true, cancelable: true }),
    );
    await sleep(0);
    await wait(ready, "query");
    await settle();
  }
  async function openRows(ids) {
    for (const id of ids) {
      const n = page().querySelector(`[data-cloud-id="${id}"] input`);
      assert(n, "select " + id);
      n.click();
    }
    await sleep(0);
    click("删除所选");
    await wait(dialog, "confirmation");
    await settle();
  }
  async function close() {
    click(dialog().dataset.cloudDeletePhase ? "保留记录并返回" : "保留歌曲");
    await wait(() => !document.querySelector("dialog[open]"), "closed");
    await settle();
  }
  function facts() {
    const d = dialog(),
      b = batch(),
      host = d ?? b ?? page(),
      r = d && rect(d),
      active = document.activeElement;
    return {
      captureTitle: "Cumulus",
      windowKind: "main",
      width: innerWidth,
      height: innerHeight,
      screenX,
      screenY,
      ratio: devicePixelRatio,
      visible: document.visibilityState === "visible",
      focused: document.hasFocus(),
      theme: document.documentElement.dataset.theme,
      overflow: document.documentElement.scrollWidth > innerWidth,
      focusInside: !!host?.contains(active),
      active: active?.getAttribute("aria-label") || active?.textContent.trim(),
      warning: d?.querySelector(".cloud-delete-warning")?.textContent.trim(),
      result: d?.querySelector(".cloud-delete-result")?.textContent.trim(),
      localNote: d
        ?.querySelector(".cloud-delete-local-note")
        ?.textContent.trim(),
      titles: [...(d?.querySelectorAll(".cloud-delete-targets li") ?? [])].map(
        (n) => n.textContent.trim(),
      ),
      scope: d?.querySelector(".cloud-delete-review>p")?.textContent.trim(),
      pagination: d
        ?.querySelector(".cloud-delete-pagination")
        ?.textContent.trim(),
      phase: d?.dataset.cloudDeletePhase,
      dialogRect: r,
      legacyDtoInjected: legacy,
      selection: b?.querySelector(".batch-select-label")?.textContent.trim(),
      status: b?.querySelector(".batch-status")?.textContent.trim(),
      buttons: [
        ...(d?.querySelectorAll(
          "header button,footer button,.cloud-delete-pagination button",
        ) ?? []),
      ].map((n) => {
        const box = rect(n),
          top = document.elementFromPoint(
            box.left + box.width / 2,
            box.top + box.height / 2,
          );
        return {
          label: n.textContent.trim() || n.getAttribute("aria-label"),
          disabled: n.disabled,
          rect: box,
          hit: n === top || n.contains(top),
        };
      }),
      dialogs: d
        ? [
            {
              open: true,
              opacity: getComputedStyle(d).opacity,
              rect: r,
              background: getComputedStyle(d).backgroundColor,
            },
          ]
        : [],
    };
  }
  async function capture(name) {
    last = name;
    await settle();
    await sleep(180);
    const v = facts();
    assert(!v.overflow && v.focusInside, "layout/focus " + name);
    assert(
      v.buttons.every((b) => b.hit),
      "hit targets " + name,
    );
    assert(v.titles.length <= 20, "bounded review " + name);
    if (v.dialogRect)
      assert(
        v.dialogRect.left >= 8 &&
          v.dialogRect.top >= 8 &&
          v.dialogRect.right <= innerWidth - 8 &&
          v.dialogRect.bottom <= innerHeight - 8,
        "dialog bounds " + name,
      );
    const stage = `ux_${String(++count).padStart(2, "0")}_${name}`;
    window.__miniRecorded = null;
    await emit({ kind: "snapshot", stage, cloudDeleteContextUi: true, ...v });
    await wait(() => window.__miniRecorded === stage, "snapshot");
  }
  try {
    await wait(
      () => document.querySelector(".track-row .track-play:not(:disabled)"),
      "startup",
    );
    const provides =
      document.querySelector("#app").__vue_app__._context.provides;
    client =
      provides[
        Reflect.ownKeys(provides).find(
          (k) => typeof k === "symbol" && k.description === "core-client",
        )
      ];
    originalRead = client.cloudPage.bind(client);
    client.cloudPage = async (...args) => {
      const v = await originalRead(...args);
      return legacy
        ? {
            ...v,
            tracks: v.tracks.map((row) => {
              const copy = { ...row };
              delete copy.deletionKind;
              return copy;
            }),
          }
        : v;
    };
    document
      .querySelector(".track-row")
      .dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    await wait(
      () => document.querySelector(".playerbar")?.dataset.state === "playing",
      "actual audio",
    );
    epoch = (await invoke("account_snapshot")).epoch;
    await invoke("player_action", {
      epoch,
      action: { kind: "pause", paused: true },
    });
    await invoke("player_action", {
      epoch,
      action: { kind: "seek", seconds: 10 },
    });
    await control("cloud-config", {
      deletionKinds: { 1001: "private", 1002: "off_store", 1003: "ordinary" },
    });
    await resize(1120, 760);
    [...document.querySelectorAll("button")]
      .find((n) => n.textContent.trim() === "音乐云盘")
      .click();
    await wait(ready, "cloud");
    await search("cloud-003");
    await theme("light");
    await openRows(["1003"]);
    assert(
      facts().warning === "确定将选中的歌曲从网盘删除？",
      "ordinary warning",
    );
    await capture("ordinary_light");
    await close();
    await search("cloud-002");
    await theme("dark");
    await openRows(["1002"]);
    assert(facts().warning.includes("无法再试听下载"), "off-store warning");
    await capture("off_store_dark");
    await close();
    await search("cloud-001");
    await theme("light");
    await openRows(["1001"]);
    assert(facts().warning.includes("其他端将无法同步"), "private warning");
    await capture("private_light");
    await close();
    await search("cloud-00");
    await theme("dark");
    await openRows(["1002", "1001"]);
    assert(
      facts().warning.includes("其他端将无法同步") &&
        facts().titles.length === 2,
      "mixed priority",
    );
    await capture("mixed_priority_dark");
    await close();
    legacy = true;
    await search("cloud-003");
    await theme("light");
    await openRows(["1003"]);
    assert(facts().warning.includes("类型尚未核实"), "missing type is unknown");
    await capture("unknown_light");
    await close();
    legacy = false;
    await search("");
    const trigger = page().querySelector(
      "[data-cloud-toolbar-more] .menu-trigger",
    );
    trigger.focus();
    trigger.click();
    await wait(
      () =>
        document.querySelector(
          '[role="menu"][aria-label="云盘更多操作"] [data-menu-key="batch"]',
        ),
      "batch menu",
    );
    document
      .querySelector(
        '[role="menu"][aria-label="云盘更多操作"] [data-menu-key="batch"]',
      )
      .click();
    await wait(
      () =>
        batch()?.dataset.cloudBatchLoading === "false" &&
        batch()?.querySelector("[data-cloud-batch-id]"),
      "batch ready",
    );
    await settle();
    batch().querySelector(".batch-select-label").focus();
    batch().querySelector(".batch-select-label").click();
    await wait(
      () =>
        batch()
          .querySelector(".batch-select-label")
          .textContent.includes("405"),
      "all405",
    );
    click("删除");
    await wait(dialog, "bulk");
    assert(
      facts().scope.includes("405") && facts().titles.length === 20,
      "full scope with bounded review",
    );
    const firstTitles = facts().titles.slice();
    await capture("bulk_first_light");
    click("下一页");
    await sleep(0);
    await wait(
      () => facts().pagination.includes("21–40"),
      "second review page",
    );
    await capture("bulk_second_light");
    for (let i = 0; i < 19; i++) {
      click("下一页");
      await sleep(0);
    }
    await wait(
      () =>
        facts().titles.length === 5 && facts().pagination.includes("401–405"),
      "last review page",
    );
    assert(
      document.activeElement === button("上一页"),
      "last page retains usable focus",
    );
    await theme("dark");
    await capture("bulk_last_dark");
    await resize(760, 540);
    await capture("bulk_small_dark");
    await theme("light");
    await capture("bulk_small_light");
    await control("cloud-config", { mode: 2, hold: true });
    click("删除歌曲");
    await wait(
      () =>
        dialog()?.dataset.cloudDeleteWorking === "true" &&
        dialog()?.querySelector("h2")?.textContent.includes("正在删除"),
      "pending deletion title",
    );
    await capture("bulk_pending_light");
    await control("cloud-config", { hold: false });
    await wait(
      () => dialog()?.dataset.cloudDeletePhase === "partial",
      "partial deletion",
    );
    assert(
      facts().scope.includes("405") && facts().titles.length === 5,
      "original405 preserved after native404",
    );
    assert(
      !facts().warning &&
        facts().result.includes("已删除 1 首") &&
        facts().result.includes("404"),
      "result replaces confirmation prompt",
    );
    await capture("bulk_partial_light");
    await theme("dark");
    await capture("bulk_partial_dark");
    await close();
    await wait(
      () =>
        batch()?.querySelector(".batch-status")?.textContent.includes("404") &&
        batch()
          ?.querySelector(".batch-select-label")
          ?.textContent.includes("404"),
      "remaining batch",
    );
    await capture("remaining_dark");
    click("删除");
    await wait(dialog, "new remaining confirmation");
    assert(
      facts().scope.includes("404") &&
        facts().titles.length === 20 &&
        facts().titles[0] !== firstTitles[0],
      "new confirmation captures remaining snapshot",
    );
    await capture("remaining_confirm_dark");
    await control("account");
    await wait(
      () => !document.querySelector("dialog[open]") && !batch(),
      "old account cleared",
    );
    epoch = (await invoke("account_snapshot")).epoch;
    await wait(ready, "new account cloud");
    await theme("light");
    page().querySelector('[aria-label="搜索云盘歌曲"]').focus();
    await capture("account_replaced_light");
    const saved = await control("cloud-state");
    assert(
      saved.writes.length === 1 && saved.writes[0].length === 405,
      "one complete fixed target",
    );
    assert(
      saved.writes[0].join(",") ===
        Array.from({ length: 405 }, (_, i) => String(1001 + i)).join(","),
      "exact original natural targets",
    );
    client.cloudPage = originalRead;
    await emit({
      kind: "complete",
      stage: "complete",
      ok: true,
      cloudDeleteContextUi: true,
      stageCount: count,
      legacyDtoInjected: true,
      realMusicRequests: 0,
      realAccountWrites: 0,
    });
  } catch (e) {
    if (client && originalRead) client.cloudPage = originalRead;
    await emit({
      kind: "failed",
      stage: "failed",
      message: String(e?.message || e),
      stack: e?.stack,
      last,
      stageCount: count,
      ...facts(),
    });
  }
})();
