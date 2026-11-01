// Production WebView/IPC/core; synthetic cloud/media. Faults are typed-client injections.
(async () => {
  const invoke = (name, args = {}) =>
      window.__TAURI_INTERNALS__.invoke(name, args),
    sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const assert = (v, m) => {
      if (!v) throw Error(m);
    },
    wait = async (fn, label) => {
      for (let i = 0; i < 300; i++) {
        if (await fn()) return;
        await sleep(60);
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
    originalRead,
    readMode = "",
    release,
    downloadRoot;
  async function control(kind, data = {}) {
    const id = ++serial;
    await emit({ kind, id, ...data });
    await wait(() => id in window.__miniAck, kind);
    const v = window.__miniAck[id];
    delete window.__miniAck[id];
    assert(!v.error, v.error);
    return v.value ?? v;
  }
  const page = () => document.querySelector(".cloud-page"),
    batch = () =>
      document.querySelector(".cloud-batch-page:not(.page-leave-active)"),
    dialog = () =>
      document.querySelector("dialog[open]:not(.modal-leave-active)"),
    menu = () =>
      document.querySelector(
        '[role="menu"][aria-label="云盘更多操作"]:not(.menu-leave-active)',
      ),
    trigger = () =>
      page()?.querySelector("[data-cloud-toolbar-more] .menu-trigger");
  const settled = async () => {
    await wait(
      () =>
        !document.querySelector(
          ".page-enter-active,.page-leave-active,.modal-enter-active,.modal-leave-active,.menu-enter-active,.menu-leave-active",
        ),
      "transitions settled",
    );
    await new Promise((r) =>
      requestAnimationFrame(() => requestAnimationFrame(r)),
    );
  };
  const ready = () =>
    page()?.dataset.cloudLoading === "false" &&
    page()?.dataset.cloudReady === "true" &&
    page()?.dataset.cloudEpoch === epoch;
  function click(label, scope = dialog() ?? page()) {
    const b = [...scope.querySelectorAll("button")].find(
      (b) =>
        b.textContent.trim() === label ||
        b.getAttribute("aria-label") === label,
    );
    assert(b && !b.disabled, "enabled " + label);
    b.scrollIntoView({ block: "nearest" });
    b.focus({ preventScroll: true });
    b.click();
    return b;
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
    await wait(ready, "query read");
    await settled();
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
  async function open() {
    await settled();
    assert(trigger() && !trigger().disabled, "toolbar trigger available");
    trigger().focus({ preventScroll: true });
    trigger().click();
    await wait(
      () => menu()?.querySelector('[data-menu-key="batch"]'),
      "toolbar menu",
    );
    await settled();
  }
  async function choose(key) {
    const b = menu().querySelector(`[data-menu-key="${key}"]`);
    assert(b && !b.disabled, "enabled " + key);
    b.focus();
    b.click();
    await wait(() => !menu(), "toolbar closes");
  }
  async function close(label = "取消") {
    click(label);
    await wait(() => !document.querySelector("dialog[open]"), "dialog closed");
    await settled();
  }
  async function normal() {
    [...document.querySelectorAll("button")]
      .find((b) => b.textContent.trim() === "音乐云盘")
      .click();
    await wait(ready, "cloud page");
    await settled();
  }
  const rect = (n) => {
    const r = n.getBoundingClientRect();
    return {
      left: r.left,
      top: r.top,
      right: r.right,
      bottom: r.bottom,
      width: r.width,
      height: r.height,
    };
  };
  function facts() {
    const m = menu(),
      d = dialog(),
      b = batch(),
      active = m ?? d ?? b,
      buttons = [
        ...(m?.querySelectorAll("button") ??
          d?.querySelectorAll("header button,footer button") ??
          b?.querySelectorAll(".batch-toolbar button") ??
          page()?.querySelectorAll(".cloud-toolbar button") ??
          []),
      ];
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
      focusInside: active
        ? active.contains(document.activeElement)
        : !!page()?.contains(document.activeElement) ||
          !!document
            .querySelector(".playerbar")
            ?.contains(document.activeElement),
      active:
        document.activeElement?.getAttribute("aria-label") ||
        document.activeElement?.textContent.trim(),
      menu: m
        ? [...m.querySelectorAll("[data-menu-key]")].map((n) => ({
            key: n.dataset.menuKey,
            label: n.textContent.trim(),
            disabled: n.disabled,
          }))
        : [],
      menuRect: m ? rect(m) : null,
      dialog: d?.getAttribute("aria-labelledby"),
      dialogText: d?.textContent.trim(),
      batch: !!b,
      batchText: b?.textContent.trim(),
      keyword: page()?.querySelector('[aria-label="搜索云盘歌曲"]')?.value,
      rows: [...(page()?.querySelectorAll("[data-cloud-id]") ?? [])].map(
        (n) => ({ id: n.dataset.cloudId, height: rect(n).height }),
      ),
      toolbarBusy:
        page()
          ?.querySelector("[data-cloud-toolbar-more]")
          ?.getAttribute("aria-busy") === "true",
      feedback: [
        ...(page()?.querySelectorAll(".cloud-toolbar-feedback") ?? []),
      ].map((n) => n.textContent.trim()),
      toolbarButtons: [
        ...(page()?.querySelectorAll(".cloud-toolbar button") ?? []),
      ].map((n) => ({
        label: n.getAttribute("aria-label") || n.textContent.trim(),
        rect: rect(n),
        disabled: n.disabled,
      })),
      buttons: buttons.map((n) => {
        const r = rect(n),
          top = document.elementFromPoint(
            r.left + r.width / 2,
            r.top + r.height / 2,
          );
        return {
          label: n.textContent.trim() || n.getAttribute("aria-label"),
          disabled: n.disabled,
          hit: n === top || n.contains(top),
        };
      }),
      dialogs: d
        ? [
            {
              open: true,
              opacity: getComputedStyle(d).opacity,
              rect: rect(d),
              background: getComputedStyle(d).backgroundColor,
            },
          ]
        : [],
    };
  }
  async function capture(name, extra = {}) {
    last = name;
    await settled();
    await sleep(180);
    const v = facts();
    assert(!v.overflow && v.focusInside, "layout/focus " + name);
    assert(
      v.buttons.every((b) => b.hit),
      "toolbar/dialog hit targets " + name,
    );
    if (!v.batch) {
      const play = v.toolbarButtons.find((b) => b.label === "播放全部"),
        upload = v.toolbarButtons.find((b) => b.label === "上传音乐"),
        more = v.toolbarButtons.find((b) => b.label === "云盘更多操作");
      assert(
        play.rect.width === 114 &&
          play.rect.height === 48 &&
          upload.rect.width === 48 &&
          more.rect.width === 48,
        "official toolbar dimensions",
      );
    }
    if (v.menuRect)
      assert(
        v.menuRect.left >= 7 &&
          v.menuRect.right <= innerWidth - 7 &&
          v.menuRect.top >= 7 &&
          v.menuRect.bottom <= innerHeight - 7,
        "menu bounds",
      );
    window.__miniRecorded = null;
    const stage = `ux_${String(++count).padStart(2, "0")}_${name}`;
    await emit({
      kind: "snapshot",
      stage,
      cloudToolbarUi: true,
      ...v,
      ...extra,
    });
    await wait(() => window.__miniRecorded === stage, "screenshot");
  }
  function install() {
    const provides =
      document.querySelector("#app").__vue_app__._context.provides;
    client =
      provides[
        Reflect.ownKeys(provides).find(
          (k) => typeof k === "symbol" && k.description === "core-client",
        )
      ];
    originalRead = client.cloudPage;
    client.cloudPage = async (...args) => {
      const mode = readMode;
      if (mode === "reject") {
        readMode = "";
        throw { code: "io", message: "合成完整列表读取失败" };
      }
      if (mode === "hold") {
        readMode = "held";
        await new Promise((r) => (release = r));
      }
      const value = await originalRead(...args);
      if (mode === "limit-download") {
        readMode = "";
        return { ...value, total: 5001 };
      }
      if (mode === "limit-queue") {
        readMode = "";
        return { ...value, total: 10001 };
      }
      if (mode === "revision") {
        readMode = "";
        return { ...value, revision: "synthetic-changed" };
      }
      return value;
    };
  }
  const queued = async () => {
    await wait(
      () =>
        page()
          ?.querySelector(".cloud-toolbar-feedback")
          ?.textContent.includes("已添加至播放列表"),
      "queue accepted",
    );
    await wait(() => trigger() && !trigger().disabled, "toolbar released");
    await settled();
  };
  try {
    await wait(
      () => document.querySelector(".track-row .track-play:not(:disabled)"),
      "startup ready",
    );
    install();
    document
      .querySelector(".track-row")
      .dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    await wait(
      () => document.querySelector(".playerbar")?.dataset.state === "playing",
      "native audio",
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
    downloadRoot = (await control("cloud-download-root")).root;
    await control("report-resize", {
      width: 1120,
      height: 760,
      scale: devicePixelRatio,
    });
    await normal();
    await search("合成");
    await theme("light");
    await open();
    await capture("toolbar_menu_light");
    assert(
      menu().querySelectorAll("[data-menu-key]").length === 3,
      "official three actions",
    );
    await choose("download");
    await wait(dialog, "full download dialog");
    assert(
      dialog().textContent.includes("405 首云盘原文件"),
      "complete result not first 200",
    );
    await capture("download_all_light");
    await close();
    assert(
      (await control("cloud-state")).downloadAuths.length === 0,
      "cancel sends no download",
    );
    click("下一页", page());
    await wait(
      () =>
        page()
          .querySelector(".cloud-pagination")
          ?.textContent.includes("2 / 3"),
      "page two",
    );
    await open();
    const before = await invoke("player_snapshot", { epoch });
    await choose("queue");
    await queued();
    const after = await invoke("player_snapshot", { epoch }),
      q = await invoke("player_queue", { epoch });
    assert(
      q.tracks.length === 445 &&
        q.tracks[1].track.id === "1405" &&
        q.tracks[405].track.id === "1001" &&
        q.tracks.at(-1).track.id === "40",
      "all filtered sorted rows queued across pages",
    );
    assert(
      after.playId === before.playId &&
        after.state === "paused" &&
        Math.abs(after.position - 10) < 0.2,
      "queue keeps decoder paused seek",
    );
    await capture("queued_all_light", {
      queuedIds: q.tracks.map((t) => t.track.id),
    });
    await search("no-result");
    await open();
    assert(
      menu().querySelector('[data-menu-key="queue"]').disabled &&
        menu().querySelector('[data-menu-key="download"]').disabled &&
        !menu().querySelector('[data-menu-key="batch"]').disabled,
      "empty search still permits full batch",
    );
    await theme("dark");
    await capture("empty_search_menu_dark");
    await choose("batch");
    await wait(
      () =>
        batch()?.dataset.cloudBatchLoading === "false" &&
        batch()?.querySelector("[data-cloud-batch-id]"),
      "independent full batch",
    );
    assert(
      batch().textContent.includes("405 首") &&
        batch().querySelector("[data-cloud-batch-id]").dataset.cloudBatchId ===
          "1001",
      "batch is unfiltered natural full library",
    );
    await capture("batch_all_dark");
    click("完成", batch());
    await wait(() => !batch(), "batch returns");
    await settled();
    assert(
      page().querySelector('[aria-label="搜索云盘歌曲"]').value === "no-result",
      "query preserved",
    );
    assert(document.activeElement === trigger(), "batch returns focus to more");
    await capture("batch_returned_dark");
    await search("cloud-00");
    page().querySelector('[data-cloud-sort="title"]').click();
    await sleep(0);
    await wait(ready, "sorted nine");
    assert(
      page().querySelectorAll("[data-cloud-id]").length === 9,
      "nine results",
    );
    await open();
    await capture("filtered_menu_dark");
    await choose("download");
    await wait(dialog, "filtered download dialog");
    assert(
      dialog().textContent.includes("9 首云盘原文件"),
      "download uses complete current filter",
    );
    await capture("download_filter_dark");
    const dir = dialog().querySelector('[aria-label="下载目录"]');
    dir.value = downloadRoot;
    dir.dispatchEvent(new Event("input", { bubbles: true }));
    click("开始下载");
    await wait(
      () => !document.querySelector("dialog[open]"),
      "download accepted",
    );
    await wait(async () => {
      const v = await invoke("downloads", {
        epoch,
        query: {
          tab: "completed",
          keyword: "",
          category: "song",
          sort: "none",
          descending: false,
          offset: 0,
          limit: 200,
        },
      });
      return v.completedCount === 9;
    }, "nine real isolated files");
    await normal();
    await wait(
      () => page().querySelectorAll("[data-cloud-id]").length === 9,
      "filter retained",
    );
    trigger().focus();
    await capture("download_confirmed_dark");
    readMode = "hold";
    await open();
    await choose("queue");
    await wait(
      () =>
        readMode === "held" &&
        page()
          .querySelector("[data-cloud-toolbar-more]")
          .getAttribute("aria-busy") === "true",
      "held full read",
    );
    assert(
      page().querySelector(".cloud-upload-button").disabled,
      "pending toolbar blocks overlapping upload",
    );
    await capture("queue_reading_dark");
    const queueBefore = (await invoke("player_queue", { epoch })).revision;
    click("播放", document.querySelector(".playerbar"));
    await wait(
      () =>
        document.querySelector(
          '.playerbar button[aria-label="暂停"]:not(:disabled)',
        ),
      "resume acknowledgment",
    );
    click("暂停", document.querySelector(".playerbar"));
    await wait(
      () => document.querySelector(".playerbar")?.dataset.state === "paused",
      "pause again",
    );
    await wait(
      () =>
        document.activeElement ===
          document.querySelector(".playerbar .play-toggle") &&
        !document.activeElement.disabled,
      "transport restores focus",
    );
    release();
    readMode = "";
    await wait(
      () =>
        page()
          ?.querySelector(".cloud-toolbar-feedback.failed")
          ?.textContent.includes("播放状态已变化"),
      "late queue canceled",
    );
    assert(
      (await invoke("player_queue", { epoch })).revision === queueBefore,
      "late read does not update queue",
    );
    await capture("queue_intent_changed_dark");
    click("重新执行", page().querySelector(".cloud-toolbar-feedback.failed"));
    await queued();
    await capture("queue_retried_dark");
    readMode = "reject";
    await open();
    await choose("download");
    await wait(
      () =>
        page()
          ?.querySelector(".cloud-toolbar-feedback.failed")
          ?.textContent.includes("合成完整列表读取失败"),
      "read failure",
    );
    await capture("read_failed_dark");
    click("重新执行", page().querySelector(".cloud-toolbar-feedback.failed"));
    await wait(dialog, "explicit download retry");
    await capture("read_retried_dark");
    await close();
    readMode = "limit-download";
    await open();
    await choose("download");
    await wait(
      () =>
        page()
          ?.querySelector(".cloud-toolbar-feedback.failed")
          ?.textContent.includes("5000"),
      "download limit",
    );
    await capture("download_limit_dark");
    readMode = "limit-queue";
    await open();
    await choose("queue");
    await wait(
      () =>
        page()
          ?.querySelector(".cloud-toolbar-feedback.failed")
          ?.textContent.includes("10000"),
      "queue limit",
    );
    await capture("queue_limit_dark");
    readMode = "revision";
    await open();
    await choose("download");
    await wait(
      () =>
        page()
          ?.querySelector(".cloud-toolbar-feedback.failed")
          ?.textContent.includes("云盘已改变"),
      "revision mismatch",
    );
    await capture("snapshot_changed_dark");
    click(
      "刷新云盘列表",
      page().querySelector(".cloud-toolbar-feedback.failed"),
    );
    await sleep(0);
    await wait(() => ready() && !trigger().disabled, "refresh changed list");
    assert(
      !document.querySelector("dialog[open]"),
      "refresh does not automatically repeat download",
    );
    await settled();
    assert(
      document.activeElement === trigger(),
      "refresh restores toolbar focus",
    );
    await capture("snapshot_refreshed_dark");
    await control("report-resize", {
      width: 760,
      height: 540,
      scale: devicePixelRatio,
    });
    await wait(() => innerWidth === 760, "small");
    await theme("light");
    page().scrollTop = 0;
    await open();
    await capture("toolbar_small_light");
    await choose("download");
    await wait(dialog, "small download");
    await capture("download_small_light");
    await close();
    await theme("dark");
    await open();
    await capture("toolbar_small_dark");
    await choose("batch");
    await wait(
      () => batch()?.dataset.cloudBatchLoading === "false",
      "small batch",
    );
    click("完成", batch());
    await wait(() => !batch(), "small batch returned");
    await settled();
    await control("report-resize", {
      width: 1120,
      height: 760,
      scale: devicePixelRatio,
    });
    await wait(() => innerWidth === 1120, "normal");
    readMode = "hold";
    await open();
    await choose("download");
    await wait(() => readMode === "held", "held download list");
    await search("cloud-001");
    release();
    readMode = "";
    await sleep(150);
    assert(
      !document.querySelector("dialog[open]") &&
        !page().querySelector(".cloud-toolbar-feedback"),
      "query change discards delayed download",
    );
    page().querySelector("input").focus();
    await capture("query_replaced_dark");
    readMode = "hold";
    await open();
    await choose("download");
    await wait(() => readMode === "held", "held old account");
    await control("account");
    release();
    readMode = "";
    epoch = (await invoke("account_snapshot")).epoch;
    await wait(
      () => ready() && document.querySelector(".account-button:not(:disabled)"),
      "new account displayed",
    );
    await settled();
    assert(
      !document.querySelector("dialog[open]"),
      "old account cannot open download",
    );
    page().querySelector("input").focus();
    await theme("light");
    await capture("account_replaced_light");
    await control("cloud-config", { rows: 0 });
    click("刷新云盘", page());
    await wait(
      () => ready() && !page().querySelector("[data-cloud-id]"),
      "empty library",
    );
    assert(trigger().disabled, "empty library more disabled");
    page().querySelector("input").focus();
    await capture("empty_library_light");
    client.cloudPage = originalRead;
    await emit({
      kind: "complete",
      stage: "complete",
      ok: true,
      cloudToolbarUi: true,
      stageCount: count,
      realMusicRequests: 0,
      realAccountWrites: 0,
      typedFaultInjection: true,
    });
  } catch (e) {
    if (client) client.cloudPage = originalRead;
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
