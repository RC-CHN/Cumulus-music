// Production WebView/IPC/core/task persistence. Only service endpoints, API
// timing/failure and the OS directory launcher are explicitly substituted.
(async () => {
  const invoke = (name, args = {}) =>
      window.__TAURI_INTERNALS__.invoke(name, args),
    sleep = (ms) => new Promise((r) => setTimeout(r, ms)),
    emit = (payload) =>
      invoke("plugin:event|emit", { event: "mini-proof", payload });
  const assert = (v, m) => {
      if (!v) throw Error(m);
    },
    wait = async (fn, label) => {
      for (let i = 0; i < 1200; i++) {
        if (await fn()) return;
        await sleep(50);
      }
      throw Error(label);
    };
  window.__miniAck = {};
  let serial = 0,
    count = 0,
    last = "startup",
    epoch,
    client,
    directoryFault = false,
    directoryChecks = 0;
  async function control(kind, data = {}) {
    const id = ++serial;
    await emit({ kind, id, ...data });
    await wait(() => id in window.__miniAck, kind);
    const value = window.__miniAck[id];
    delete window.__miniAck[id];
    assert(!value.error, value.error);
    return value.value ?? value;
  }
  const page = () => document.querySelector(".cloud-page"),
    dialog = () =>
      document.querySelector(
        "dialog.cloud-task-remove-dialog[open]:not(.modal-leave-active)",
      ),
    rows = () => [
      ...(page()?.querySelectorAll(".cloud-task-table tbody tr") ?? []),
    ],
    menu = () =>
      document.querySelector(
        '[role=menu][aria-label^="上传任务更多操作"]:not(.menu-leave-active)',
      );
  const ready = () =>
    page()?.dataset.cloudEpoch === epoch &&
    page()?.dataset.cloudLoading === "false" &&
    page()?.dataset.cloudReady === "true";
  async function settled() {
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
  function click(label, scope = dialog() ?? page()) {
    const button = [...scope.querySelectorAll("button")].find(
      (b) =>
        b.textContent.trim() === label ||
        b.getAttribute("aria-label") === label,
    );
    assert(button && !button.disabled, "enabled " + label);
    button.scrollIntoView({ block: "nearest" });
    button.focus({ preventScroll: true });
    button.click();
    return button;
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
  async function snapshot(name, extra = {}) {
    last = name;
    await settled();
    await sleep(180);
    const d = dialog(),
      m = menu(),
      r = d ? rect(d) : null,
      scope = d ?? m,
      buttons = [
        ...(d?.querySelectorAll("header button,footer button") ??
          m?.querySelectorAll("button") ??
          page()?.querySelectorAll(".cloud-task-toolbar button") ??
          []),
      ];
    const value = {
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
      dialogText: d?.textContent,
      dialogRect: r,
      taskRows: rows().map((row) => ({
        id: row.dataset.uploadId,
        phase: row.dataset.uploadPhase,
        rect: rect(row),
        text: row.textContent,
      })),
      columns: [
        ...(page()?.querySelectorAll(".cloud-task-table thead th") ?? []),
      ].map((n) => ({ label: n.textContent, rect: rect(n) })),
      toolbar: [
        ...(page()?.querySelectorAll(".cloud-task-toolbar>button") ?? []),
      ].map((b) => ({
        label: b.textContent.trim(),
        disabled: b.disabled,
        rect: rect(b),
      })),
      menu: m
        ? [...m.querySelectorAll("button[data-menu-key]")].map((b) => ({
            key: b.dataset.menuKey,
            label: b.textContent.trim(),
            disabled: b.disabled,
          }))
        : null,
      buttons: buttons.map((b) => {
        const r = rect(b),
          hit = document.elementFromPoint(
            r.left + r.width / 2,
            r.top + r.height / 2,
          );
        return {
          label: b.getAttribute("aria-label") || b.textContent.trim(),
          disabled: b.disabled,
          rect: r,
          hit: b.disabled || hit === b || b.contains(hit),
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
    assert(!value.overflow && value.focusInside, "layout/focus " + name);
    assert(
      value.buttons.every((b) => b.hit),
      "button hit " + name,
    );
    assert(
      value.taskRows.length <= 200 &&
        value.taskRows.every((row) => row.rect.height === 64),
      "bounded64px rows " + name,
    );
    if (value.columns.length) {
      assert(
        Math.abs(value.columns[0].rect.width - 44) < 1,
        "number column44px " + name,
      );
      assert(
        Math.abs(
          value.columns[1].rect.width - value.columns[2].rect.width - 148,
        ) < 1,
        "official title/progress width difference " + name,
      );
    }
    if (name.startsWith("tasks_small_") && value.taskRows.length) {
      const player = document
        .querySelector(".playerbar")
        .getBoundingClientRect();
      assert(
        value.taskRows[0].rect.bottom <= player.top + 1,
        "first small-window task row visible " + name,
      );
    }
    if (["upload_complete_dark", "encoding_rechecked_dark"].includes(name)) {
      assert(
        document.activeElement?.classList.contains("cloud-task-history-entry"),
        "completed task returns to available history control " + name,
      );
    }
    window.__miniRecorded = null;
    const stage = `ux_${String(++count).padStart(2, "0")}_${name}`;
    await emit({
      kind: "snapshot",
      stage,
      cloudTaskUi: true,
      ...value,
      ...extra,
    });
    await wait(() => window.__miniRecorded === stage, "capture " + name);
  }
  async function refresh(total) {
    await wait(
      () => page()?.querySelector('[aria-label="刷新云盘"]:not(:disabled)'),
      "refresh available",
    );
    click("刷新云盘", page());
    await sleep(0);
    await wait(ready, "cloud read");
    await wait(
      () => rows().length === Math.min(total, 200),
      "visible task count " + total,
    );
    await settled();
  }
  async function pending() {
    return client.cloudUploadSelection(epoch, false);
  }
  async function history() {
    return client.cloudUploadSelection(epoch, true);
  }
  async function confirmClear() {
    click("全部清空", page());
    await wait(dialog, "clear confirmation");
    await settled();
  }
  async function close(label = "取消") {
    click(label);
    await wait(() => !dialog(), "confirmation closed");
    await settled();
  }
  async function rowMenu(row = rows()[0], pointer = false) {
    const trigger = row.querySelector(".menu-trigger");
    trigger.focus();
    if (pointer) {
      const r = row.getBoundingClientRect();
      row.dispatchEvent(
        new MouseEvent("contextmenu", {
          bubbles: true,
          cancelable: true,
          clientX: r.left + 70,
          clientY: r.top + 30,
        }),
      );
    } else
      trigger.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "ArrowDown",
          bubbles: true,
          cancelable: true,
        }),
      );
    await wait(menu, "task menu");
    await settled();
  }
  try {
    await wait(
      () =>
        [...document.querySelectorAll("button")].some(
          (b) => b.textContent.trim() === "音乐云盘" && !b.disabled,
        ),
      "cloud navigation ready",
    );
    const provides =
      document.querySelector("#app").__vue_app__._context.provides;
    client =
      provides[
        Reflect.ownKeys(provides).find(
          (key) => typeof key === "symbol" && key.description === "core-client",
        )
      ];
    client.openCloudUploadDirectory = async (owner, id) => {
      if (directoryFault) {
        directoryFault = false;
        throw { code: "io", message: "合成：目录打开暂时失败，请重试" };
      }
      const value = await control("upload-directory", {
        epoch: owner,
        taskId: id,
      });
      assert(
        value.verified && value.osLauncherInjected,
        "native source directory validation",
      );
      directoryChecks++;
    };
    epoch = (await invoke("account_snapshot")).epoch;
    await resize(1120, 760);
    await theme("light");
    [...document.querySelectorAll("button")]
      .find((b) => b.textContent.trim() === "音乐云盘")
      .click();
    await wait(ready, "cloud");
    click("正在上传 0", page());
    await wait(
      () => page().textContent.includes("还没有上传任务"),
      "empty tasks",
    );
    await snapshot("empty_light");
    const seeded = (await control("upload-task-seed", { count: 205 })).ids;
    await refresh(205);
    assert(
      (await pending()).total === 205,
      "native complete pending selection",
    );
    await snapshot("first_page_light");
    click("下一页", page());
    await sleep(0);
    await wait(() => rows().length === 5, "last task page");
    await theme("dark");
    await snapshot("last_page_dark");
    await control("upload-config", {
      hold: true,
      holdPath: "/api/cloud/upload/check",
    });
    click("全部上传", page());
    await wait(async () => {
      const p = await pending();
      return (
        p.uploads.length === 205 && p.uploads.every((task) => task.enabled)
      );
    }, "all205 scheduled");
    await wait(
      async () => (await control("upload-state")).holdEntered === 1,
      "check held",
    );
    await snapshot("all_queued_dark");
    await wait(
      () =>
        [...page().querySelectorAll(".cloud-task-toolbar button")].some(
          (b) => b.textContent.trim() === "全部暂停" && !b.disabled,
        ),
      "pause all available",
    );
    click("全部暂停", page());
    await wait(
      async () => (await pending()).uploads.every((task) => !task.enabled),
      "all205 paused",
    );
    await control("upload-config", { hold: false });
    await wait(
      async () => (await pending()).uploads.every((task) => !task.running),
      "worker stopped",
    );
    await refresh(5);
    await snapshot("all_paused_dark");
    await confirmClear();
    assert(
      dialog().textContent.includes("205 个任务"),
      "all205 confirmation independent of page",
    );
    await snapshot("clear_confirm_dark");
    await close();
    assert((await pending()).total === 205, "cancel does not remove");
    await theme("light");
    const first = rows()[0];
    click(`删除上传任务 ${first.querySelector("strong").textContent}`, first);
    await wait(dialog, "single confirmation");
    await snapshot("single_remove_light");
    await close();
    assert((await pending()).total === 205, "single cancel preserves jobs");
    click(`删除上传任务 ${first.querySelector("strong").textContent}`, first);
    await wait(dialog, "single confirmation second");
    click("删除任务");
    await wait(() => !dialog(), "single removed");
    await wait(async () => (await pending()).total === 204, "204 remain");
    await wait(ready, "page current");
    await snapshot("single_removed_light");
    await confirmClear();
    assert(dialog().textContent.includes("204 个任务"), "captured204");
    await snapshot("clear_snapshot_light");
    const late = (await control("upload-task-seed", { count: 1 })).ids[0];
    click("清空上传任务");
    await wait(() => !dialog(), "all captured removed");
    await wait(async () => {
      const p = await pending();
      return p.total === 1 && p.uploads[0].id === late;
    }, "new task survives captured clear");
    await wait(() => rows().length === 1, "late task visible");
    assert(
      (await control("upload-state")).seededInputsIntact,
      "source files retained",
    );
    await snapshot("new_task_retained_light");
    // Model the DTO from the native journal without dispatching a second
    // cancellation or publication in this test's real synthetic workflow.
    const readCloudPage = client.cloudPage;
    try {
      for (const [phase, label, running] of [
        ["audio_uploading", "取消待确认", false],
        ["publication_pending", "正在保存", true],
      ]) {
        client.cloudPage = async (...args) => {
          const value = await readCloudPage.call(client, ...args);
          return {
            ...value,
            uploads: value.uploads.map((task) =>
              task.id === late
                ? { ...task, phase, label, running, canResume: false }
                : task,
            ),
          };
        };
        await refresh(1);
        const progress = rows()[0].querySelector(".cloud-task-progress");
        assert(
          progress.textContent.trim() === label && !progress.querySelector("progress"),
          label + " is not replaced with a fallback status or transfer meter",
        );
      }
    } finally {
      client.cloudPage = readCloudPage;
    }
    await refresh(1);
    click(
      `开始上传 ${rows()[0].querySelector("strong").textContent}`,
      rows()[0],
    );
    await wait(
      async () =>
        (await history()).uploads.some(
          (task) => task.id === late && task.phase === "complete",
        ),
      "actual upload complete",
    );
    await wait(
      () =>
        page().textContent.includes("还没有上传任务") && rows().length === 0,
      "completed leaves pending list",
    );
    await theme("dark");
    await snapshot("upload_complete_dark");
    click("上传记录 1", page());
    await sleep(0);
    await wait(() => rows().length === 1, "history one");
    await snapshot("history_dark");
    await rowMenu(rows()[0], true);
    assert(
      menu().querySelector("[data-menu-key=play]").disabled &&
        menu().querySelector("[data-menu-key=next]").disabled,
      "official upload menu disables playback",
    );
    await snapshot("history_menu_dark");
    menu().dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    );
    await wait(() => !menu(), "context menu closes");
    await settled();
    await rowMenu();
    await snapshot("history_keyboard_menu_dark");
    directoryFault = true;
    click("打开文件所在目录", menu());
    await wait(
      () => page().textContent.includes("目录打开暂时失败"),
      "directory failure",
    );
    await settled();
    await snapshot("directory_error_dark");
    await rowMenu();
    click("打开文件所在目录", menu());
    await wait(() => directoryChecks === 1, "directory retry verified");
    await settled();
    assert(
      document.activeElement === rows()[0].querySelector(".menu-trigger"),
      "directory focus returns to task",
    );
    await snapshot("directory_retried_dark");
    click(
      `删除上传任务 ${rows()[0].querySelector("strong").textContent}`,
      rows()[0],
    );
    await wait(dialog, "history remove");
    await snapshot("history_remove_dark");
    await close();
    assert((await history()).total === 1, "history cancel");
    click(
      `删除上传任务 ${rows()[0].querySelector("strong").textContent}`,
      rows()[0],
    );
    await wait(dialog, "history remove second");
    click("清除记录");
    await wait(() => !dialog(), "history hidden");
    await wait(
      () =>
        rows().length === 0 && page().textContent.includes("还没有上传记录"),
      "history empty",
    );
    assert(
      (await control("upload-state")).publishedIds.length === 1,
      "published song survives history removal",
    );
    await snapshot("history_cleared_dark");
    click("返回上传", page());
    await sleep(0);
    await wait(ready, "back to tasks");
    await control("upload-task-seed", { count: 1 });
    await refresh(1);
    await control("upload-config", { encodingMode: 1 });
    click(
      `开始上传 ${rows()[0].querySelector("strong").textContent}`,
      rows()[0],
    );
    await wait(async () => {
      const p = await pending();
      return p.uploads[0]?.canRetryEncoding && !p.uploads[0].running;
    }, "encoding failed needs status check");
    await refresh(1);
    await snapshot("encoding_failed_dark");
    const encodeBefore = await control("upload-state");
    await control("upload-config", { encodingMode: 2 });
    click("重新检查", rows()[0]);
    await wait(
      async () =>
        (await history()).total === 1 && (await pending()).total === 0,
      "encoding check completes",
    );
    await wait(() => rows().length === 0, "completed retry leaves tasks");
    const encodeAfter = await control("upload-state");
    for (const endpoint of [
      "/api/cloud/upload/check",
      "/api/whale/token/alloc",
      "/api/upload/cloud/info/v2",
    ])
      assert(
        encodeBefore.calls[endpoint] === encodeAfter.calls[endpoint],
        "encoding retry does not repeat " + endpoint,
      );
    await snapshot("encoding_rechecked_dark");
    click("上传记录 1", page());
    await sleep(0);
    await wait(() => rows().length === 1, "encoding record visible");
    click("清除记录", page().querySelector(".cloud-task-toolbar"));
    await wait(dialog, "clear encoding record");
    click("清除记录");
    await wait(
      () => !dialog() && rows().length === 0,
      "encoding history hidden",
    );
    click("返回上传", page());
    await sleep(0);
    await wait(ready, "back after encoding record");
    await control("upload-config", { encodingMode: 0 });

    await control("upload-task-seed", { count: 1 });
    await refresh(1);
    await theme("light");
    await control("upload-config", {
      hold: true,
      holdPath: "/api/whale/token/alloc",
      loseAllocation: true,
    });
    click(
      `开始上传 ${rows()[0].querySelector("strong").textContent}`,
      rows()[0],
    );
    await wait(
      async () => (await control("upload-state")).holdEntered === 1,
      "allocation held",
    );
    await refresh(1);
    assert(
      rows()[0].querySelector('[aria-label^="删除上传任务"]').disabled,
      "in-flight unknown allocation removal disabled",
    );
    await snapshot("allocation_inflight_light");
    await confirmClear();
    click("清空上传任务");
    await wait(
      () =>
        dialog()?.dataset.taskRemoveSent === "true" &&
        dialog().getAttribute("aria-busy") === "false",
      "clear explicitly rejected",
    );
    await snapshot("clear_rejected_light");
    await control("upload-config", { hold: false });
    await wait(async () => {
      const p = await pending();
      return p.uploads[0]?.label === "等待核对" && !p.uploads[0].running;
    }, "unknown allocation preserved");
    click("只读核对");
    await sleep(0);
    await wait(
      () => dialog().getAttribute("aria-busy") === "false",
      "read-only cancellation inspection",
    );
    await close("返回任务列表");
    await refresh(1);
    const before = (await control("upload-state")).calls[
      "/api/whale/token/alloc"
    ];
    await refresh(1);
    assert(
      (await control("upload-state")).calls["/api/whale/token/alloc"] ===
        before,
      "refresh does not repeat allocation",
    );
    await snapshot("allocation_unknown_light");
    await resize(760, 540);
    await snapshot("tasks_small_light");
    await theme("dark");
    await snapshot("tasks_small_dark");
    await confirmClear();
    await snapshot("confirm_small_dark");
    await control("account");
    epoch = (await invoke("account_snapshot")).epoch;
    await wait(
      () =>
        !dialog() &&
        ready() &&
        document.querySelector(".account-button:not(:disabled)"),
      "account replaced",
    );
    assert(
      (await pending()).total === 0 && (await history()).total === 0,
      "old task records do not enter new account",
    );
    await snapshot("account_replaced_dark");
    const final = await control("upload-state");
    assert(
      final.seededInputs === 208 && final.seededInputsIntact,
      "all208 input files retained",
    );
    assert(
      final.publishedIds.length === 2 &&
        final.objects.length === 2 &&
        final.objects.every((object) => object.matchesFixture),
      "two actual completed synthetic objects retained",
    );
    assert(
      final.calls["/api/cloud/upload/check"] === 4 &&
        final.calls["/api/whale/token/alloc"] === 3 &&
        final.calls["/api/cloud/pub/v2"] === 2,
      "no hidden write retries",
    );
    assert(seeded.length === 205, "initial complete selection");
    await emit({
      kind: "complete",
      stage: "complete",
      ok: true,
      cloudTaskUi: true,
      stageCount: count,
      actualInitialTaskCount: 205,
      sourceFilesIntact: true,
      nativeDirectoryChecks: directoryChecks,
      osLauncherInjected: true,
      realMusicRequests: 0,
      realAccountWrites: 0,
      physicalAudioOutputVerified: false,
    });
  } catch (e) {
    await emit({
      kind: "failed",
      stage: "failed",
      ok: false,
      last,
      error: String(e?.message || e),
      stack: e?.stack,
      html: page()?.outerHTML.slice(0, 18000),
      dialogText: dialog()?.textContent,
    });
  }
})();
