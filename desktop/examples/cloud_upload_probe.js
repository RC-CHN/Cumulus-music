// Native WebView/IPC/SQLite/ffprobe/NOS state machine. OS selections and
// unavailable service endpoints are explicitly synthetic; no account writes.
(async () => {
  const invoke = (name, args = {}) =>
      window.__TAURI_INTERNALS__.invoke(name, args),
    sleep = (ms) => new Promise((r) => setTimeout(r, ms)),
    emit = (payload) =>
      invoke("plugin:event|emit", { event: "mini-proof", payload }),
    assert = (v, m) => {
      if (!v) throw Error(m);
    },
    wait = async (fn, label) => {
      for (let i = 0; i < 400; i++) {
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
    selection = "cancel",
    pickMode = "",
    releasePick,
    lostReply = false;
  const calls = [],
    receipts = [];
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
    dialog = () =>
      document.querySelector(
        "dialog.cloud-upload-dialog[open]:not(.modal-leave-active)",
      ),
    files = () => [...(dialog()?.querySelectorAll("[data-upload-file]") ?? [])],
    ready = () =>
      page()?.dataset.cloudReady === "true" &&
      page()?.dataset.cloudLoading === "false" &&
      page()?.dataset.cloudEpoch === epoch;
  async function settled() {
    await wait(
      () =>
        !document.querySelector(
          ".page-enter-active,.page-leave-active,.modal-enter-active,.modal-leave-active",
        ),
      "transitions",
    );
    await new Promise((r) =>
      requestAnimationFrame(() => requestAnimationFrame(r)),
    );
  }
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
  async function open() {
    await settled();
    click("上传音乐", page());
    await wait(dialog, "upload modal");
    await settled();
  }
  async function close(label = "取消") {
    click(label);
    await wait(() => !dialog(), "upload closed");
    await settled();
  }
  async function choose(name, label = "选择音频文件") {
    selection = name;
    click(label);
    await sleep(0);
    await wait(
      () => dialog()?.getAttribute("aria-busy") === "false",
      "picker ready",
    );
    await sleep(50);
  }
  function rect(n) {
    const r = n.getBoundingClientRect();
    return {
      left: r.left,
      top: r.top,
      right: r.right,
      bottom: r.bottom,
      width: r.width,
      height: r.height,
    };
  }
  function facts() {
    const d = dialog(),
      r = d && rect(d),
      buttons = [...(d?.querySelectorAll("header button,footer button") ?? [])];
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
      overflow:
        document.documentElement.scrollWidth > innerWidth + 1 ||
        (r &&
          (r.left < 7 ||
            r.right > innerWidth - 7 ||
            r.top < 7 ||
            r.bottom > innerHeight - 7)),
      dialogRect: r,
      dialogText: d?.textContent,
      focus:
        document.activeElement?.getAttribute("aria-label") ||
        document.activeElement?.textContent?.trim(),
      focusInside: !d || d.contains(document.activeElement),
      busy: d?.getAttribute("aria-busy"),
      uncertain: d?.dataset.uploadUncertain,
      files: files().map((n) => ({
        path: n.dataset.uploadPath,
        name: n.querySelector("strong")?.textContent,
        error: n.querySelector(".cloud-file-error")?.textContent,
        details: n.querySelector("details")?.open,
      })),
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
          hit: hit === b || b.contains(hit),
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
      calls: calls.map((c) => ({
        kind: c.kind,
        files: c.files?.map((f) => ({
          requestId: f.request_id,
          path: f.input.path,
        })),
      })),
      receipts,
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
      "button hit " + name,
    );
    window.__miniRecorded = null;
    const stage = `ux_${String(++count).padStart(2, "0")}_${name}`;
    await emit({
      kind: "snapshot",
      stage,
      cloudUploadUi: true,
      ...v,
      ...extra,
    });
    await wait(() => window.__miniRecorded === stage, "capture " + name);
  }
  const nativeQuery = {
    tab: "history",
    keyword: "",
    sort: "natural",
    descending: false,
    offset: 0,
    limit: 200,
  };
  async function completed(total) {
    let result;
    await wait(async () => {
      result = await client.cloudPage(epoch, nativeQuery, false);
      return (
        result.uploads.length === total &&
        result.uploads.every((v) => v.phase === "complete")
      );
    }, "actual upload completion " + total);
    return result;
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
    const originalCommand = client.cloudCommand;
    client.cloudCommand = async (...args) => {
      if (args[1].kind === "upload_planned")
        calls.push(structuredClone(args[1]));
      const result = await originalCommand(...args);
      if (args[1].kind === "upload_planned") {
        receipts.push(result.accepted);
        if (lostReply) {
          lostReply = false;
          throw { code: "io", message: "合成：原生任务已登记，回执未抵达界面" };
        }
      }
      return result;
    };
    client.pickCloudFiles = async (_epoch, kind) => {
      const name = selection,
        mode = pickMode;
      pickMode = "";
      if (mode === "error")
        throw { code: "io", message: "合成：文件选择暂时不可用，请重试" };
      const result = await control("upload-select", {
        selection: name,
        pickKind: kind,
      });
      if (mode === "hold") {
        pickMode = "held";
        await new Promise((r) => (releasePick = r));
      }
      return result.paths;
    };
    document
      .querySelector(".track-row")
      .dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    await wait(
      () => document.querySelector(".playerbar")?.dataset.state === "playing",
      "native player",
    );
    epoch = (await invoke("account_snapshot")).epoch;
    await invoke("player_action", {
      epoch,
      action: { kind: "pause", paused: true },
    });
    await resize(1120, 760);
    await theme("light");
    [...document.querySelectorAll("button")]
      .find((b) => b.textContent.trim() === "音乐云盘")
      .click();
    await wait(ready, "cloud ready");
    await open();
    await capture("empty_light");
    selection = "pair";
    pickMode = "hold";
    const origin = click("选择音频文件");
    await wait(() => pickMode === "held", "picker held");
    await capture("choosing_light");
    assert(dialog().getAttribute("aria-busy") === "true", "busy picker");
    releasePick();
    await wait(
      () =>
        files().length === 2 && dialog().getAttribute("aria-busy") === "false",
      "pair added",
    );
    await wait(
      () => document.activeElement === origin,
      "picker focus restored",
    );
    await capture("same_names_light");
    const [first, second] = files();
    first.querySelector("summary").click();
    second.querySelector("summary").click();
    for (const kind of ["cover", "lyric", "cue"]) {
      selection = kind;
      click(
        kind === "cue"
          ? "选择CUE：same.flac"
          : `选择${kind === "cover" ? "封面" : "歌词"}：same.flac`,
        first,
      );
      await sleep(0);
      await wait(
        () => dialog().getAttribute("aria-busy") === "false",
        "attachment " + kind,
      );
    }
    selection = "invalid";
    click("选择封面：same.flac", second);
    await sleep(0);
    await wait(
      () => dialog().getAttribute("aria-busy") === "false",
      "invalid cover selected",
    );
    await capture("attachments_light");
    click("开始上传 (2)");
    await wait(
      () =>
        files().length === 1 &&
        files()[0].querySelector(".cloud-file-error") &&
        dialog().getAttribute("aria-busy") === "false",
      "partial upload receipt",
    );
    const partial = await completed(1);
    assert(
      files()[0].dataset.uploadPath.includes("second"),
      "failure belongs to second full path",
    );
    assert(
      calls.length === 1 && calls[0].files.length === 2,
      "one registration command",
    );
    await theme("dark");
    assert(
      document.activeElement.classList.contains("cloud-file-error"),
      "partial error receives focus",
    );
    assert(
      files()[0].querySelector(".cloud-file-error").textContent.includes("JPG"),
      "specific cover validation feedback",
    );
    await capture("partial_error_dark", { actualUploads: partial.uploads });
    const failed = files()[0],
      oldRequest = calls[0].files.find(
        (f) => f.input.path === failed.dataset.uploadPath,
      ).request_id;
    failed.querySelector("details").open = true;
    selection = "cover";
    click("选择封面：same.flac", failed);
    await sleep(0);
    await wait(
      () => dialog().getAttribute("aria-busy") === "false",
      "correct cover",
    );
    await capture("corrected_attachment_dark");
    click("开始上传 (1)");
    await wait(() => !dialog(), "partial retry closed");
    const two = await completed(2);
    assert(
      calls[1].files.length === 1 &&
        calls[1].files[0].request_id !== oldRequest,
      "retry changed only rejected file",
    );
    await wait(
      () =>
        !page().querySelector(".cloud-upload-row") &&
        page().textContent.includes("还没有上传任务"),
      "completed task leaves current list",
    );
    await capture("completed_tasks_dark", { actualUploads: two.uploads });
    await theme("light");
    await open();
    await choose("single");
    lostReply = true;
    click("开始上传 (1)");
    await wait(
      () =>
        dialog()?.dataset.uploadUncertain === "true" &&
        dialog().getAttribute("aria-busy") === "false",
      "lost receipt",
    );
    const three = await completed(3);
    await capture("unknown_receipt_light", { actualUploads: three.uploads });
    const before = await control("upload-state");
    click("核对并补全任务");
    await wait(() => !dialog(), "receipt recovered");
    const after = await control("upload-state");
    assert(
      JSON.stringify(calls[2]) === JSON.stringify(calls[3]),
      "same request after lost reply",
    );
    assert(
      JSON.stringify(receipts[2]) === JSON.stringify(receipts[3]),
      "same task after lost reply",
    );
    for (const endpoint of [
      "/api/cloud/upload/check",
      "/api/whale/token/alloc",
      "/api/upload/cloud/info/v2",
      "/api/cloud/pub/v2",
    ])
      assert(
        before.calls[endpoint] === after.calls[endpoint],
        "no repeated write " + endpoint,
      );
    await capture("receipt_recovered_light", {
      actualUploads: (await completed(3)).uploads,
    });
    await resize(760, 540);
    await theme("dark");
    await open();
    await choose("many", "选择文件夹");
    assert(files().length === 20, "bounded rendered files");
    await capture("many_first_small_dark");
    for (let i = 0; i < 24; i++) click("下一页");
    await sleep(50);
    assert(
      files().length === 20 && files()[0].dataset.uploadPath.includes("480-"),
      "last bounded page",
    );
    await capture("many_last_small_dark");
    await choose("extra");
    assert(
      files().length === 20 &&
        dialog().textContent.includes("单次最多添加 500"),
      "aggregate selection limit",
    );
    await capture("limit_small_dark");
    await close();
    await theme("light");
    await open();
    pickMode = "error";
    click("选择音频文件");
    await wait(
      () => dialog().textContent.includes("文件选择暂时不可用"),
      "picker error",
    );
    await capture("picker_error_small_light");
    await choose("cancel");
    assert(
      files().length === 0 && !dialog().querySelector("[role=alert]"),
      "cancel clears picker error",
    );
    await capture("cancelled_picker_small_light");
    await choose("pair");
    files()[0].querySelector("summary").click();
    selection = "cover";
    click("选择封面：same.flac", files()[0]);
    await sleep(0);
    await wait(
      () => dialog().getAttribute("aria-busy") === "false",
      "small attachment",
    );
    await capture("attachments_small_light");
    await close();
    await resize(1120, 760);
    await theme("dark");
    await open();
    selection = "single";
    pickMode = "hold";
    click("选择音频文件");
    await wait(() => pickMode === "held", "held account picker");
    await control("account");
    await invoke("account_snapshot");
    epoch = (await invoke("account_snapshot")).epoch;
    releasePick();
    await wait(
      () =>
        !dialog() &&
        ready() &&
        document.querySelector(".account-button:not(:disabled)"),
      "account replaced",
    );
    assert(
      !document.body.textContent.includes("upload-inputs"),
      "private paths cleared",
    );
    await capture("account_replaced_dark");
    const final = await control("upload-state");
    assert(
      final.publishedIds.length === 3 && final.objects.length === 7,
      "three real synthetic upload jobs and four attachments",
    );
    assert(
      final.objects.every((o) => o.matchesFixture && o.bytes > 0),
      "all stored bytes match actual fixtures",
    );
    assert(
      final.calls["/api/cloud/upload/check"] === 3 &&
        final.calls["/api/cloud/pub/v2"] === 3,
      "one remote creation/publication per job",
    );
    assert(
      final.attachmentMetadata.some(
        (keys) => keys.join(",") === "coverid,lyricid,cueid",
      ),
      "all attachment IDs reach metadata",
    );
    await emit({
      kind: "complete",
      stage: "complete",
      ok: true,
      cloudUploadUi: true,
      stageCount: count,
      realMusicRequests: 0,
      realAccountWrites: 0,
      registrationCommands: calls.length,
      actualUploadTasks: 3,
      actualStoredObjects: 7,
      typedPickerInjected: true,
      lostReplyInjected: true,
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
      ...facts(),
    });
  }
})();
