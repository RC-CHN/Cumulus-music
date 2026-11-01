// Actual production WebView/IPC/Rust/local mapping/SQLite. Service and the OS
// browser/file-manager launchers are explicit synthetic boundaries.
(async () => {
  const invoke = (name, args = {}) =>
      window.__TAURI_INTERNALS__.invoke(name, args),
    sleep = (ms) => new Promise((r) => setTimeout(r, ms)),
    assert = (v, m) => {
      if (!v) throw Error(m);
    };
  const wait = async (fn, label) => {
    for (let i = 0; i < 1200; i++) {
      if (await fn()) return;
      await sleep(50);
    }
    throw Error(label);
  };
  const emit = (value) =>
    invoke("plugin:event|emit", { event: "mini-proof", payload: value });
  window.__miniAck = {};
  let serial = 0,
    count = 0,
    last = "startup",
    epoch,
    client,
    browserCalls = 0,
    directoryCalls = 0,
    releaseBrowser;
  async function control(kind, data = {}) {
    const id = ++serial;
    await emit({ kind, id, ...data });
    await wait(() => id in window.__miniAck, kind);
    const r = window.__miniAck[id];
    delete window.__miniAck[id];
    assert(!r.error, r.error);
    return r.value ?? r;
  }
  const page = () => document.querySelector(".cloud-page"),
    rows = () => [...page().querySelectorAll("[data-cloud-id]")],
    menu = () =>
      document.querySelector(
        '[role=menu][aria-label="云盘歌曲操作"]:not(.menu-leave-active)',
      ),
    dialog = () =>
      document.querySelector("dialog[open]:not(.modal-leave-active)");
  const ready = () =>
    page()?.dataset.cloudEpoch === epoch &&
    page()?.dataset.cloudReady === "true" &&
    page()?.dataset.cloudLoading === "false";
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
  const key = (n, key, extra = {}) =>
    n.dispatchEvent(
      new KeyboardEvent("keydown", {
        key,
        bubbles: true,
        cancelable: true,
        ...extra,
      }),
    );
  const button = (label, scope = dialog() ?? menu() ?? page()) =>
    [...scope.querySelectorAll("button")].find(
      (n) =>
        n.textContent.trim() === label ||
        n.getAttribute("aria-label") === label,
    );
  function click(label, scope) {
    const b = button(label, scope);
    assert(b && !b.disabled, "enabled " + label);
    b.scrollIntoView({ block: "nearest" });
    b.focus({ preventScroll: true });
    b.click();
    return b;
  }
  async function settle() {
    await wait(
      () =>
        !document.querySelector(
          ".modal-enter-active,.modal-leave-active,.menu-enter-active,.menu-leave-active,.page-enter-active,.page-leave-active",
        ),
      "transitions",
    );
    await new Promise((r) =>
      requestAnimationFrame(() => requestAnimationFrame(r)),
    );
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
  const selected = () =>
    rows()
      .filter((n) => n.getAttribute("aria-selected") === "true")
      .map((n) => n.dataset.cloudId);
  const widths = () =>
    [...page().querySelectorAll(".cloud-table th")].map((n) => rect(n).width);
  async function snapshot(name) {
    last = name;
    await settle();
    await sleep(180);
    const d = dialog(),
      m = menu(),
      scope = d ?? m,
      r = d ? rect(d) : null;
    const buttons = [
      ...(scope?.querySelectorAll(
        "header button,footer button,button[data-menu-key]",
      ) ?? []),
    ];
    const state = {
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
      overflow:
        document.documentElement.scrollWidth > innerWidth + 1 ||
        (r &&
          (r.left < 7 ||
            r.right > innerWidth - 7 ||
            r.top < 7 ||
            r.bottom > innerHeight - 7)),
      focusInside: !scope || scope.contains(document.activeElement),
      focus:
        document.activeElement?.getAttribute("aria-label") ||
        document.activeElement?.textContent?.trim(),
      columns: widths(),
      rows: rows().map((n) => ({
        id: n.dataset.cloudId,
        rect: rect(n),
        selected: n.getAttribute("aria-selected") === "true",
      })),
      selected: selected(),
      sort: page()
        .querySelector("[data-cloud-sort=title]")
        ?.closest("th")
        .getAttribute("aria-sort"),
      capacity: page().querySelector(".cloud-capacity")?.textContent,
      capacityRect: rect(page().querySelector(".cloud-capacity")),
      progress: page().querySelector(".cloud-capacity progress")?.value,
      browserCalls,
      directoryCalls,
      dialog: !!d,
      dialogText: d?.textContent,
      dialogRect: r,
      menu: m
        ? [...m.querySelectorAll("[data-menu-key]")].map((n) => ({
            key: n.dataset.menuKey,
            label: n.textContent.trim(),
            disabled: n.disabled,
          }))
        : null,
      buttons: buttons.map((n) => {
        const r = rect(n),
          hit = document.elementFromPoint(
            r.left + r.width / 2,
            r.top + r.height / 2,
          );
        return {
          label: n.getAttribute("aria-label") || n.textContent.trim(),
          disabled: n.disabled,
          hit: n.disabled || hit === n || n.contains(hit),
        };
      }),
      paintSamples: d
        ? [
            {
              x: r.left + 6,
              y: r.top + r.height / 2,
              color: getComputedStyle(d).backgroundColor,
            },
          ]
        : [],
    };
    assert(!state.overflow && state.focusInside, "layout/focus " + name);
    assert(
      state.rows.length <= 200 &&
        state.rows.every((row) => Math.abs(row.rect.height - 64) < 1),
      "bounded64px " + name,
    );
    assert(
      state.buttons.every((b) => b.hit),
      "button hit " + name,
    );
    if (state.columns.length) {
      assert(
        Math.abs(state.columns[0] - 28) < 1 &&
          Math.abs(state.columns[1] - 45) < 1 &&
          Math.abs(state.columns[4] - 64) < 1,
        "fixed columns " + name,
      );
    }
    if (name === "keyboard_played_dark") {
      const focused = document.activeElement.closest("[data-cloud-id]");
      assert(
        focused &&
          rect(focused).bottom <=
            rect(document.querySelector(".playerbar")).top + 1,
        "playing focused row remains above player",
      );
    }
    window.__miniRecorded = null;
    const stage = `ux_${String(++count).padStart(2, "0")}_${name}`;
    await emit({ kind: "snapshot", stage, cloudSongUi: true, ...state });
    await wait(() => window.__miniRecorded === stage, "capture " + name);
  }
  async function refresh() {
    await wait(
      () => button("刷新云盘", page()) && !button("刷新云盘", page()).disabled,
      "refresh control",
    );
    click("刷新云盘", page());
    await sleep(0);
    await wait(ready, "cloud refresh");
    await settle();
  }
  async function search(text) {
    const n = page().querySelector('[aria-label="搜索云盘歌曲"]');
    n.value = text;
    n.dispatchEvent(new Event("input", { bubbles: true }));
    n.form.dispatchEvent(
      new Event("submit", { bubbles: true, cancelable: true }),
    );
    await sleep(0);
    await wait(ready, "filtered");
    await settle();
  }
  async function rowMenu() {
    const row = rows()[0];
    row.querySelector(".cloud-more").focus();
    key(row, "ContextMenu");
    await wait(
      () => menu()?.querySelector("[data-menu-key=play]:not(:disabled)"),
      "resource menu",
    );
    await settle();
  }
  async function closeMenu() {
    key(menu(), "Escape");
    await wait(() => !menu(), "menu closed");
    await settle();
  }
  function pointer(index, extra = {}) {
    const row = rows()[index];
    row.dispatchEvent(
      new PointerEvent("pointerdown", {
        pointerId: 1,
        button: 0,
        bubbles: true,
        cancelable: true,
        ...extra,
      }),
    );
    row.dispatchEvent(
      new PointerEvent("pointerup", {
        pointerId: 1,
        button: 0,
        bubbles: true,
        cancelable: true,
        ...extra,
      }),
    );
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
    client.openCloudDirectory = async (owner, id, revision) => {
      assert(id === "1001", "directory target");
      const r = await control("cloud-local-file", {
        action: "verify",
        epoch: owner,
        revision,
      });
      assert(r.verified && r.osLauncherInjected, "native directory check");
      directoryCalls++;
    };
    client.openCloudCapacity = async (owner) => {
      assert(
        owner === (await invoke("account_snapshot")).epoch,
        "browser owner",
      );
      browserCalls++;
      if (browserCalls === 1)
        throw { code: "io", message: "合成：浏览器暂时无法打开" };
      if (browserCalls === 3)
        await new Promise((r) => {
          releaseBrowser = r;
        });
    };
    document
      .querySelector(".track-row")
      .dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    await wait(
      () => document.querySelector(".playerbar")?.dataset.state === "playing",
      "native playback",
    );
    epoch = (await invoke("account_snapshot")).epoch;
    await invoke("player_action", {
      epoch,
      action: { kind: "pause", paused: true },
    });
    await resize(1120, 760);
    [...document.querySelectorAll("button")]
      .find((n) => n.textContent.trim() === "音乐云盘")
      .click();
    await wait(ready, "cloud");
    await theme("light");
    await snapshot("catalog_light");
    await theme("dark");
    await snapshot("catalog_dark");
    const resizeTitle = () =>
      page().querySelector('[aria-label="调整标题列宽"]');
    let before = widths();
    resizeTitle().focus();
    key(resizeTitle(), "ArrowRight");
    await wait(() => widths()[2] > before[2] + 7, "keyboard resize");
    assert(
      Math.abs(widths()[3] - before[3] + 8) < 1,
      "album gives title surplus",
    );
    await snapshot("title_resized_dark");
    let handle = resizeTitle();
    handle.dispatchEvent(
      new PointerEvent("pointerdown", {
        pointerId: 63,
        button: 0,
        clientX: 200,
        bubbles: true,
      }),
    );
    document.dispatchEvent(
      new PointerEvent("pointermove", {
        pointerId: 63,
        clientX: 222,
        bubbles: true,
      }),
    );
    document.dispatchEvent(
      new PointerEvent("pointerup", {
        pointerId: 63,
        clientX: 222,
        bubbles: true,
      }),
    );
    await settle();
    await snapshot("pointer_resized_dark");
    before = widths();
    handle = resizeTitle();
    handle.dispatchEvent(
      new PointerEvent("pointerdown", {
        pointerId: 64,
        button: 0,
        clientX: 200,
        bubbles: true,
      }),
    );
    document.dispatchEvent(
      new PointerEvent("pointermove", {
        pointerId: 64,
        clientX: 250,
        bubbles: true,
      }),
    );
    await settle();
    key(document, "Escape");
    await settle();
    widths().forEach((v, i) =>
      assert(Math.abs(v - before[i]) < 1, "escape restores column " + i),
    );
    await snapshot("resize_cancelled_dark");
    page().querySelector("[data-cloud-sort=title]").click();
    await sleep(0);
    await wait(ready, "sort");
    await settle();
    widths().forEach((v, i) =>
      assert(Math.abs(v - before[i]) < 1, "sort retains column " + i),
    );
    await snapshot("sorted_resized_dark");
    click("正在上传 0", page());
    await wait(
      () => page().textContent.includes("还没有上传任务"),
      "tasks empty",
    );
    click("已上传单曲 405", page());
    await sleep(0);
    await wait(ready, "return songs");
    await settle();
    widths().forEach((v, i) =>
      assert(Math.abs(v - before[i]) < 1, "tabs retain columns " + i),
    );
    await snapshot("tabs_restored_dark");
    await theme("light");
    pointer(2);
    await sleep(0);
    pointer(4, { ctrlKey: true });
    await sleep(0);
    assert(
      selected().join(",") ===
        [rows()[2].dataset.cloudId, rows()[4].dataset.cloudId].join(","),
      "control selection",
    );
    await snapshot("pointer_selected_light");
    pointer(6, { shiftKey: true });
    await sleep(0);
    assert(
      selected().join(",") ===
        rows()
          .slice(4, 7)
          .map((n) => n.dataset.cloudId)
          .join(","),
      "shift range",
    );
    await snapshot("range_selected_light");
    pointer(5, { ctrlKey: true, shiftKey: true });
    await sleep(0);
    assert(
      selected().join(",") === rows()[6].dataset.cloudId,
      "control shift subtracts",
    );
    await snapshot("range_subtracted_light");
    await theme("dark");
    rows()[6].querySelector("input").focus();
    key(rows()[6].querySelector("input"), "a", { ctrlKey: true });
    await sleep(0);
    assert(selected().length === 200, "only current page selected");
    await snapshot("all_page_selected_dark");
    key(rows()[6], "ArrowDown");
    await wait(
      () =>
        selected().length === 1 &&
        selected()[0] === rows()[199].dataset.cloudId,
      "arrow collapses from selection edge",
    );
    await snapshot("edge_keyboard_dark");
    key(rows()[199], "Delete", { repeat: true });
    await sleep(100);
    assert(!dialog(), "repeat delete ignored");
    key(rows()[199], "Delete");
    await wait(dialog, "keyboard delete confirmation");
    await snapshot("delete_keyboard_dark");
    click("保留歌曲");
    await wait(() => !dialog(), "delete cancel");
    await settle();
    assert(
      document.activeElement?.dataset.cloudId === rows()[199].dataset.cloudId,
      "delete returns to row",
    );
    await snapshot("keyboard_cancel_dark");
    key(rows()[199], "Enter");
    await wait(
      () => document.querySelector(".playerbar")?.dataset.state === "playing",
      "keyboard playback",
    );
    await invoke("player_action", {
      epoch,
      action: { kind: "pause", paused: true },
    });
    await snapshot("keyboard_played_dark");
    await search("001");
    assert(
      !page().querySelector(".cloud-notice"),
      "query clears previous playback acknowledgement",
    );
    await wait(
      () => rows().length === 1 && rows()[0].dataset.cloudId === "1001",
      "single mapped target",
    );
    await theme("light");
    await rowMenu();
    assert(
      !menu().querySelector("[data-menu-key=open]"),
      "unmapped has no directory",
    );
    await snapshot("public_menu_light");
    await closeMenu();
    await control("cloud-local-file", { action: "map" });
    await rowMenu();
    assert(
      menu().querySelector("[data-menu-key=open]"),
      "mapped local directory",
    );
    await snapshot("mapped_menu_light");
    click("打开文件所在目录");
    await wait(
      () =>
        document
          .querySelector(".track-action-feedback")
          ?.textContent.includes("已请求打开"),
      "directory accepted",
    );
    await snapshot("mapped_opened_light");
    click("关闭", document.querySelector(".track-action-feedback"));
    await control("cloud-local-file", { action: "change" });
    await rowMenu();
    assert(
      !menu().querySelector("[data-menu-key=open]"),
      "changed local file hidden",
    );
    await snapshot("changed_file_menu_light");
    await closeMenu();
    await control("cloud-local-file", { action: "corrupt" });
    await theme("dark");
    await rowMenu();
    assert(
      menu().querySelector("[data-menu-key=local-issue]")?.disabled &&
        !menu().querySelector("[data-menu-key=open]"),
      "local fault degrades only directory",
    );
    await snapshot("damaged_local_menu_dark");
    click("播放");
    await wait(
      () => document.querySelector(".playerbar")?.dataset.state === "playing",
      "play despite local fault",
    );
    await invoke("player_action", {
      epoch,
      action: { kind: "pause", paused: true },
    });
    await snapshot("damaged_local_played_dark");
    click("关闭", document.querySelector(".track-action-feedback"));
    await control("cloud-local-file", { action: "repair" });
    await control("cloud-config", { size: 1073741824, maxSize: 1073741824 });
    await refresh();
    assert(
      button("上传音乐", page()).disabled &&
        page().textContent.includes("网盘已满"),
      "full capacity state",
    );
    await snapshot("capacity_full_dark");
    await control("cloud-config", { size: null, maxSize: null });
    await refresh();
    await theme("light");
    assert(
      page().querySelector(".cloud-capacity-numbers").textContent.trim() ===
        "—/—",
      "unknown capacity never fabricated",
    );
    await snapshot("capacity_unknown_light");
    await control("cloud-config", { size: 2 ** 40, maxSize: 2 ** 41 });
    await refresh();
    assert(
      page().querySelector(".cloud-capacity-numbers").textContent.trim() ===
        "1.0TB/2.0TB",
      "official TB units",
    );
    await snapshot("capacity_terabyte_light");
    await theme("dark");
    click("扩容", page());
    await wait(
      () => page().querySelector(".cloud-capacity-feedback.failed"),
      "browser failure",
    );
    await snapshot("capacity_open_failed_dark");
    click("重试", page().querySelector(".cloud-capacity-feedback"));
    await wait(
      () =>
        page()
          .querySelector(".cloud-capacity-feedback")
          ?.textContent.includes("已请求在浏览器打开"),
      "browser accepted",
    );
    await snapshot("capacity_opened_dark");
    click("关闭", page().querySelector(".cloud-capacity-feedback"));
    await search("");
    await control("cloud-config", { rows: 0, size: 0, maxSize: 1073741824 });
    await refresh();
    await theme("light");
    assert(
      !page().querySelector(".cloud-capacity-tip"),
      "empty library has no permanent-storage claim",
    );
    await snapshot("empty_library_light");
    await control("cloud-config", {
      rows: 405,
      size: 123456789,
      maxSize: 1073741824,
    });
    await refresh();
    await resize(760, 540);
    await snapshot("catalog_small_light");
    await theme("dark");
    await snapshot("catalog_small_dark");
    await search("001");
    await rowMenu();
    await snapshot("menu_small_dark");
    await closeMenu();
    click("扩容", page());
    await wait(() => !!releaseBrowser, "held browser launcher");
    await snapshot("capacity_pending_dark");
    await control("account");
    epoch = (await invoke("account_snapshot")).epoch;
    releaseBrowser();
    await wait(ready, "new account");
    await theme("light");
    assert(
      !page().querySelector(".cloud-capacity-feedback"),
      "late browser feedback isolated",
    );
    await snapshot("account_replaced_light");
    const stats = await control("cloud-state");
    assert(
      stats.writes.length === 0 && stats.collectWrites.length === 0,
      "no cloud/playlist writes",
    );
    assert(browserCalls === 3 && directoryCalls === 1, "launcher counts");
    await emit({
      kind: "complete",
      stage: "complete",
      ok: true,
      cloudSongUi: true,
      stageCount: count,
      browserCalls,
      directoryCalls,
      serviceSynthetic: true,
      osBrowserInjected: true,
      osFileManagerInjected: true,
      realMusicRequests: 0,
      realAccountWrites: 0,
    });
  } catch (error) {
    let diagnostic = {};
    try {
      diagnostic = {
        visible: document.visibilityState,
        focused: document.hasFocus(),
        selected: page() ? selected() : [],
        columns: page() ? widths() : [],
        tableWidth: page()
          ?.querySelector(".cloud-table")
          ?.getBoundingClientRect().width,
        tableMinimum: page()?.querySelector(".cloud-table")
          ? getComputedStyle(page().querySelector(".cloud-table")).minWidth
          : null,
        colWidths: [...(page()?.querySelectorAll("col") ?? [])].map((n) => ({
          style: n.style.width,
          width: n.getBoundingClientRect().width,
        })),
      };
    } catch {}
    await emit({
      kind: "failed",
      stage: "failed",
      message: String(error),
      stack: error?.stack,
      last,
      stageCount: count,
      ...diagnostic,
    });
  }
})();
