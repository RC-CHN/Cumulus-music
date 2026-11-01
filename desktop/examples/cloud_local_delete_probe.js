// Real production UI/IPC/core/files, synthetic service, FLAC and response-loss injection.
(async () => {
  const invoke = (name, args = {}) =>
      window.__TAURI_INTERNALS__.invoke(name, args),
    sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const assert = (v, m) => {
    if (!v) throw Error(m);
  };
  async function wait(fn, label) {
    for (let i = 0; i < 500; i++) {
      if (await fn()) return;
      await sleep(50);
    }
    throw Error(label);
  }
  const emit = (value) =>
    invoke("plugin:event|emit", { event: "mini-proof", payload: value });
  window.__miniAck = {};
  let serial = 0,
    count = 0,
    last = "startup",
    client,
    originalExecute;
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
      document.querySelector("dialog[open]:not(.modal-leave-active)");
  const file = (songId, action = "observe") =>
    control("cloud-removal-file", { songId, action });
  function click(label, scope = dialog() ?? page()) {
    const n = [...scope.querySelectorAll("button")].find(
      (b) =>
        b.textContent.trim() === label ||
        b.getAttribute("aria-label") === label,
    );
    assert(n && !n.disabled, "enabled " + label);
    n.scrollIntoView({ block: "nearest" });
    n.focus({ preventScroll: true });
    n.click();
  }
  async function settle() {
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
  async function resize(width, height) {
    await control("report-resize", { width, height, scale: devicePixelRatio });
    await wait(
      () => innerWidth === width && innerHeight === height,
      "window size",
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
  async function search(value) {
    const n = page().querySelector('[aria-label="搜索云盘歌曲"]');
    n.value = value;
    n.dispatchEvent(new Event("input", { bubbles: true }));
    n.form.dispatchEvent(
      new Event("submit", { bubbles: true, cancelable: true }),
    );
    await sleep(0);
    await wait(
      () =>
        page()?.dataset.cloudLoading === "false" &&
        page()?.dataset.cloudReady === "true",
      "cloud query",
    );
  }
  async function open(ids) {
    for (const c of page().querySelectorAll("[data-cloud-id] input:checked"))
      c.click();
    for (const id of ids) {
      const c = page().querySelector(`[data-cloud-id="${id}"] input`);
      assert(c, "cloud row " + id);
      c.click();
    }
    await sleep(0);
    click("删除所选");
    await wait(
      () => dialog()?.dataset.cloudDeleteReady === "true",
      "native preview",
    );
    await settle();
  }
  async function close() {
    click(
      dialog().textContent.includes("保留记录并返回")
        ? "保留记录并返回"
        : "保留歌曲",
    );
    await wait(() => !document.querySelector("dialog[open]"), "closed");
    await settle();
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
      box = d ? rect(d) : null,
      option = d?.querySelector("[data-cloud-delete-local]");
    return {
      captureTitle: "Cumulus",
      windowKind: "main",
      width: innerWidth,
      height: innerHeight,
      ratio: devicePixelRatio,
      screenX,
      screenY,
      visible: document.visibilityState === "visible",
      focused: document.hasFocus(),
      theme: document.documentElement.dataset.theme,
      overflow: document.documentElement.scrollWidth > innerWidth,
      focusInside: !!d?.contains(document.activeElement),
      phase: d?.dataset.cloudDeletePhase,
      heading: d?.querySelector("h2")?.textContent.trim(),
      text: d?.textContent.trim(),
      checked: option?.checked ?? null,
      optionDisabled: option?.disabled ?? null,
      optionInline:
        !option ||
        (Math.abs(
          (rect(option).top + rect(option).bottom) / 2 -
            (rect(option.nextElementSibling).top +
              rect(option.nextElementSibling).bottom) /
              2,
        ) < 4 &&
          rect(option.nextElementSibling).left >= rect(option).right),
      cue: d?.querySelector("[data-cloud-delete-cue]")?.textContent.trim(),
      summary: d
        ?.querySelector(".cloud-delete-local-result summary")
        ?.textContent.trim(),
      localRows:
        d?.querySelectorAll(".cloud-delete-local-result li").length ?? 0,
      buttons: [
        ...(d?.querySelectorAll("header button,footer button") ?? []),
      ].map((n) => {
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
              rect: box,
              opacity: getComputedStyle(d).opacity,
              background: getComputedStyle(d).backgroundColor,
            },
          ]
        : [],
    };
  }
  async function capture(label, extra = {}) {
    last = label;
    await settle();
    const f = facts();
    assert(!f.overflow && f.focusInside, "layout/focus " + label);
    assert(
      f.buttons.every((b) => b.hit),
      "hit targets " + label,
    );
    assert(f.localRows <= 20, "bounded local receipts");
    const box = f.dialogs[0].rect;
    assert(
      box.left >= 8 &&
        box.top >= 8 &&
        box.right <= innerWidth - 8 &&
        box.bottom <= innerHeight - 8,
      "dialog bounds " + label,
    );
    const stage = `ux_${String(++count).padStart(2, "0")}_${label}`;
    window.__miniRecorded = null;
    await emit({
      kind: "snapshot",
      stage,
      cloudLocalDeleteUi: true,
      ...f,
      ...extra,
    });
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
    document
      .querySelector(".track-row")
      .dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    await wait(
      () => document.querySelector(".playerbar")?.dataset.state === "playing",
      "native player",
    );
    const epoch = (await invoke("account_snapshot")).epoch;
    await invoke("player_action", {
      epoch,
      action: { kind: "pause", paused: true },
    });
    await control("report-resize", {
      width: 1120,
      height: 760,
      scale: devicePixelRatio,
    });
    for (const songId of ["1004", "1005", "1006"]) {
      await control("cloud-menu-seed", { songId });
      assert((await file(songId)).exists, "real seeded file " + songId);
    }
    [...document.querySelectorAll("button")]
      .find((n) => n.textContent.trim() === "音乐云盘")
      .click();
    await wait(() => page()?.dataset.cloudReady === "true", "cloud");
    await search("cloud-00");
    await theme("light");
    await open(["1004"]);
    assert(!facts().checked, "default keeps local");
    await capture("local_keep_light");
    dialog().querySelector("[data-cloud-delete-local]").click();
    await resize(900, 620);
    await theme("dark");
    assert(facts().checked, "checked");
    await capture("local_checked_dark");
    await close();
    assert((await file("1004")).exists, "cancel kept file");
    await theme("light");
    await open(["1006"]);
    dialog().querySelector("[data-cloud-delete-local]").click();
    await control("cloud-config", { size: 123456790 });
    click("删除歌曲");
    await wait(
      () =>
        dialog()?.dataset.cloudDeleteWorking === "false" &&
        dialog()?.textContent.includes("重新确认"),
      "preflight changed result",
    );
    assert(
      !dialog().querySelector(".cloud-delete-warning"),
      "no confirmation question after rejection",
    );
    assert(
      facts().summary.includes("未执行 1 项"),
      "local work not left waiting after preflight rejection",
    );
    await capture("preflight_changed_light");
    assert((await file("1006")).exists, "preflight kept local file");
    await close();
    await file("1004", "cue");
    await theme("light");
    await open(["1004"]);
    assert(
      !dialog().querySelector("[data-cloud-delete-local]"),
      "CUE cannot be unlinked",
    );
    assert(facts().cue.includes("均来自"), "all CUE");
    await capture("cue_all_light");
    await close();
    await theme("dark");
    await open(["1004", "1003"]);
    assert(facts().cue.includes("其中 1 首"), "mixed CUE");
    await capture("cue_mixed_dark");
    await close();
    await open(["1005"]);
    dialog().querySelector("[data-cloud-delete-local]").click();
    await file("1005", "change");
    originalExecute = client.cloudDeleteExecute.bind(client);
    let lost = true;
    client.cloudDeleteExecute = async (...args) => {
      const value = await originalExecute(...args);
      if (lost) {
        lost = false;
        throw { code: "transport", message: "合成回执丢失，请核对" };
      }
      return value;
    };
    click("删除歌曲");
    await wait(
      () =>
        dialog()?.dataset.cloudDeleteWorking === "false" &&
        dialog()?.textContent.includes("只读核对"),
      "lost acknowledgement",
    );
    assert(facts().optionDisabled, "choice stays frozen");
    await capture("write_unknown_dark", { injectedReplyLoss: true });
    click("只读核对");
    await wait(
      () => dialog()?.dataset.cloudDeletePhase === "confirmed",
      "read-only result",
    );
    dialog().querySelector("details summary").click();
    await capture("source_retained_dark");
    assert(facts().summary.includes("保留 1 项"), "replacement retained");
    assert((await file("1005")).exists, "modified file retained");
    client.cloudDeleteExecute = originalExecute;
    await close();
    await control("cloud-config", { mode: 2 });
    await search("cloud-00");
    await theme("light");
    await open(["1006", "1007"]);
    dialog().querySelector("[data-cloud-delete-local]").click();
    await file("1006", "hold-index");
    click("删除歌曲");
    await wait(
      () =>
        dialog()?.dataset.cloudDeletePhase === "partial" &&
        dialog()?.textContent.includes("重试本地清理"),
      "local failure with remote result",
    );
    dialog().querySelector("details summary").click();
    await capture("local_failed_light");
    assert((await file("1006")).exists, "failed cleanup retains file");
    await file("1006", "release-index");
    await theme("dark");
    click("重试本地清理");
    await wait(
      () =>
        dialog()?.dataset.cloudDeleteWorking === "false" &&
        dialog()?.textContent.includes("已清理 1 项"),
      "explicit local retry",
    );
    await capture("local_retry_dark");
    const removed = await file("1006");
    assert(
      !removed.exists && !removed.downloadPresent,
      "file and metadata removed",
    );
    await close();
    const history = page().querySelector(".cloud-delete-history");
    history.open = true;
    click("查看结果", history);
    await wait(
      () => dialog()?.dataset.cloudDeleteReady === "true",
      "reopen persisted receipt",
    );
    await theme("light");
    await capture("receipt_reopened_light");
    assert(facts().summary.includes("已清理 1 项"), "reopened local receipt");

    const cloud = await control("cloud-state");
    assert(
      cloud.writes.length === 2,
      "two intentional remote requests; no repeats",
    );
    assert(cloud.realRequests === 0, "no real requests");
    assert((await file("1004")).exists, "CUE audio retained");
    await emit({
      kind: "complete",
      stage: "complete",
      ok: true,
      cloudLocalDeleteUi: true,
      stageCount: count,
      syntheticWrites: cloud.writes,
      realAccountWrites: 0,
      realMusicRequests: 0,
    });
  } catch (e) {
    if (client && originalExecute) client.cloudDeleteExecute = originalExecute;
    await emit({
      kind: "failed",
      stage: "failed",
      message: String(e?.message ?? e),
      stack: e?.stack,
      last,
    });
  }
})();
