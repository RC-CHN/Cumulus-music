// Production FM feedback UI, synthetic account/service/audio; no live mutation.
(async () => {
  const one = (s) => document.querySelector(s),
    all = (s) => [...document.querySelectorAll(s)],
    sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const invoke = (...a) =>
    (window.__CUMULUS_PROOF_INVOKE__ ?? window.__TAURI_INTERNALS__.invoke)(
      ...a,
    );
  const emit = (stage, value = {}) =>
    invoke("plugin:event|emit", {
      event: "ux-proof",
      payload: { stage, ...value },
    });
  const assert = (v, m) => {
    if (!v) throw Error(m);
  };
  const wait = async (f, m) => {
    for (let i = 0; i < 300; i++) {
      if (await f()) return;
      await sleep(50);
    }
    throw Error(m);
  };
  const dialog = () => one(".fm-dialog[open]:not(.modal-leave-active)"),
    bar = () => one(".playerbar");
  const button = (text, root = document) =>
    [...root.querySelectorAll("button")].find(
      (n) =>
        n.getAttribute("aria-label") === text || n.textContent.trim() === text,
    );
  const click = (n) => {
    assert(n && !n.disabled, "enabled " + n?.outerHTML.slice(0, 160));
    n.scrollIntoView({ block: "center" });
    n.focus({ preventScroll: true });
    n.click();
  };
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
  const hit = (n) => {
    const r = rect(n),
      h = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
    return h === n || n.contains(h);
  };
  let epoch,
    count = 0,
    last = "boot",
    playId,
    reasonReads = 0;
  async function fixture(kind) {
    window.__fmFeedbackFixtureAck = null;
    await emit("fm-feedback-fixture", { kind });
    await wait(() => window.__fmFeedbackFixtureAck === kind, "fixture " + kind);
  }
  async function prepare() {
    await wait(
      () => ["playing", "paused"].includes(bar()?.dataset.state),
      "native audio",
    );
    await invoke("player_action", {
      epoch,
      action: { kind: "pause", paused: true },
    });
    await invoke("player_action", {
      epoch,
      action: { kind: "seek", seconds: 10 },
    });
    await wait(
      () =>
        bar().dataset.state === "paused" &&
        Math.abs(Number(bar().dataset.position) - 10) < 0.2,
      "native paused 10",
    );
    playId = bar().dataset.playId;
    await wait(
      () => one(".fm-copy h2")?.textContent === `song-${bar().dataset.trackId}`,
      "FM view current",
    );
  }
  async function show(ready = true) {
    reasonReads++;
    click(button("减少推荐", one(".fm-tools")));
    await wait(() => dialog(), "dialog opened");
    if (ready)
      await wait(
        () =>
          dialog().querySelector(".fm-reason") &&
          !dialog().querySelector('[role="alert"]'),
        "reason loaded",
      );
  }
  async function close() {
    click(button("取消", dialog()));
    await wait(() => !one(".fm-dialog[open]"), "dialog leave");
    await wait(
      () => document.activeElement === button("减少推荐", one(".fm-tools")),
      "feedback focus returned",
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
  async function size(width, height) {
    await emit("resize", { width, height });
    await sleep(240);
    if (innerWidth !== width || innerHeight !== height)
      await emit("resize", {
        width: (width * width) / innerWidth,
        height: (height * height) / innerHeight,
      });
    await wait(() => innerWidth === width && innerHeight === height, "size");
  }
  async function capture(label) {
    last = label;
    await sleep(250);
    await new Promise((resolve) =>
      requestAnimationFrame(() => requestAnimationFrame(resolve)),
    );
    const d = dialog(),
      r = d && rect(d),
      active = document.activeElement;
    assert(
      document.documentElement.scrollWidth <= innerWidth,
      "no horizontal overflow",
    );
    assert(
      all(".fm-trash [data-song-id] label").every(
        (n) =>
          n.textContent.trim().length > 0 &&
          n.querySelector("input").getAttribute("aria-label").trim() !== "选择",
      ),
      "trash identities remain readable with sparse metadata",
    );
    assert(
      bar().dataset.state === "paused" &&
        bar().dataset.playId === playId &&
        Math.abs(Number(bar().dataset.position) - 10) < 0.2,
      "same paused media",
    );
    if (d) {
      assert(
        r.left >= 8 &&
          r.right <= innerWidth - 8 &&
          r.top >= 8 &&
          r.bottom <= innerHeight - 8,
        "dialog contained",
      );
      assert(d.scrollWidth <= d.clientWidth + 1, "dialog text contained");
      let color = getComputedStyle(document.documentElement)
        .getPropertyValue("--surface")
        .trim()
        .slice(1);
      if (color.length === 3) color = [...color].map((v) => v + v).join("");
      const surface = `rgb(${[0, 2, 4].map((i) => parseInt(color.slice(i, i + 2), 16)).join(", ")})`;
      assert(getComputedStyle(d).backgroundColor === surface, "theme surface");
      for (const n of d.querySelectorAll("header button,footer button"))
        if (!n.disabled) assert(hit(n), "essential modal controls hit");
      assert(d.contains(active), "modal owns keyboard focus");
      if (label === "artist_partial_light")
        assert(
          d.textContent.includes("歌曲已减少推荐") &&
            d.textContent.includes("歌手屏蔽失败"),
          "explicit partial success explanation",
        );
    }
    const paintSamples = [];
    if (d) {
      paintSamples.push({
        x: r.left + 6,
        y: r.top + r.height / 2,
        color: getComputedStyle(d).backgroundColor,
      });
      for (const notice of d.querySelectorAll(".form-warning")) {
        const nr = rect(notice);
        if (nr.top >= r.top && nr.bottom <= r.bottom)
          paintSamples.push({
            x: nr.left + 4,
            y: nr.top + nr.height / 2,
            color: getComputedStyle(notice).backgroundColor,
          });
      }
      assert(
        [...d.querySelectorAll(".fm-reason")].every(
          (n) => rect(n).height <= 45,
        ),
        "reason rows keep official bounded height",
      );
    }
    const stage = `ux_${String(++count).padStart(2, "0")}_${label}`;
    window.__queueSelectionRecorded = null;
    await emit(stage, {
      captureHandshake: true,
      fmFeedbackUi: true,
      width: innerWidth,
      height: innerHeight,
      ratio: devicePixelRatio,
      visible: document.visibilityState === "visible",
      focused: document.hasFocus(),
      theme: document.documentElement.dataset.theme,
      playId,
      expectedReasonReads: reasonReads,
      dialog: r
        ? {
            ...r,
            text: d.textContent,
            busy: d.getAttribute("aria-busy") === "true",
            reasonCount: d.querySelectorAll(".fm-reason").length,
            canSubmit: !d.querySelector("footer .primary").disabled,
            issue: [...d.querySelectorAll('[role="alert"]')].map(
              (n) => n.textContent,
            ),
            review: !!button("查看反馈状态", d),
            background: getComputedStyle(d).backgroundColor,
          }
        : null,
      active: {
        tag: active?.tagName,
        class: active?.className,
        text: active?.textContent.trim().slice(0, 80),
      },
      selected: all(".fm-trash input:checked").length,
      trashRows: all(".fm-trash [data-song-id]").map((n) => n.dataset.songId),
      paintSamples,
    });
    await wait(
      () => window.__queueSelectionRecorded === stage,
      "native snapshot " + label,
    );
  }
  try {
    await wait(
      () =>
        one(".account-button") &&
        !one(".account-button").disabled &&
        !one(".welcome"),
      "account",
    );
    epoch = (await invoke("account_snapshot")).epoch;
    await theme("light");
    click(button("私人漫游"));
    await wait(() => button("开始漫游"), "FM");
    click(button("开始漫游"));
    await prepare();
    await capture("initial_light");
    await fixture("hold-reasons");
    await show(false);
    await wait(
      () => dialog().textContent.includes("正在读取反馈选项"),
      "loading",
    );
    await capture("reason_loading_light");
    await close();
    await fixture("release-reasons");
    await sleep(250);
    assert(!dialog(), "closed late reasons");
    await capture("reason_cancelled_light");
    await show();
    await capture("ready_light");
    await theme("dark");
    await capture("ready_dark");
    await size(760, 540);
    await capture("ready_narrow_dark");
    await theme("light");
    await capture("ready_narrow_light");
    await close();
    await fixture("reason-error");
    await show(false);
    await wait(() => button("重新加载选项", dialog()), "reason error");
    await capture("reason_error_light");
    reasonReads++;
    click(button("重新加载选项", dialog()));
    await wait(() => dialog().querySelector(".fm-reason"), "reason retry");
    await capture("reason_retry_light");
    await close();
    await fixture("reason-empty");
    await show(false);
    await wait(
      () => dialog().textContent.includes("暂无更多反馈选项"),
      "empty reason",
    );
    await capture("reason_empty_light");
    await close();
    await fixture("reason-normal");
    await show();
    await fixture("trash-rejected");
    click(button("仅减少此歌曲", dialog()));
    await wait(
      () =>
        dialog()?.querySelector('[role="alert"]') &&
        dialog().getAttribute("aria-busy") === "false",
      "rejected stays",
    );
    await capture("rejected_light");
    await theme("dark");
    await capture("rejected_dark");
    await close();
    await capture("rejected_cancelled_dark");
    await show();
    await fixture("hold-write");
    click(button("仅减少此歌曲", dialog()));
    await wait(
      () => dialog()?.getAttribute("aria-busy") === "true",
      "write pending",
    );
    dialog().dispatchEvent(new Event("cancel", { cancelable: true }));
    assert(!!dialog(), "pending cancel blocked");
    document.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: " ",
        bubbles: true,
        cancelable: true,
      }),
    );
    await capture("pending_dark");
    await fixture("release-write");
    await wait(() => !one(".fm-dialog[open]"), "success closes");
    await prepare();
    await capture("submitted_dark");
    await show();
    await invoke("player_action", { epoch, action: { kind: "next" } });
    await prepare();
    await wait(
      () => dialog()?.textContent.includes("当前歌曲或频道已切换"),
      "stale dialog",
    );
    assert(button("仅减少此歌曲", dialog()).disabled, "stale action disabled");
    await capture("changed_dark");
    await close();
    await capture("changed_closed_dark");
    await theme("light");
    await size(1120, 760);
    await show();
    await fixture("artist-rejected");
    click(dialog().querySelector(".fm-reason[data-artist-id]"));
    await wait(
      () =>
        dialog()?.querySelector('[role="alert"]') &&
        button("查看反馈状态", dialog()),
      "partial artist error",
    );
    await prepare();
    await capture("artist_partial_light");
    click(button("查看反馈状态", dialog()));
    await wait(
      () =>
        !one(".fm-dialog[open]") &&
        document.activeElement === one(".fm-recovery"),
      "recovery focus",
    );
    await capture("recovery_light");
    click(button("重试此操作", one(".fm-recovery")));
    await wait(
      () => !button("重试此操作", one(".fm-recovery") ?? document),
      "artist retried",
    );
    await prepare();
    await capture("artist_retried_light");
    await show();
    click(button("音乐黑名单", dialog()));
    await wait(
      () =>
        !one(".fm-dialog[open]") &&
        one(".fm-trash") &&
        document.activeElement === one(".fm-header h1"),
      "blacklist navigation",
    );
    await wait(() => all(".fm-trash input").length === 2, "trash list");
    await capture("blacklist_light");
    for (const n of all(".fm-trash input")) n.click();
    await capture("restore_selected_light");
    await fixture("restore-rejected");
    const restore = button("恢复推荐（2）", one(".fm-trash-toolbar"));
    click(restore);
    await wait(
      () => one(".error-state") && restore && !restore.disabled,
      "restore rejected",
    );
    await wait(
      () => document.activeElement === restore,
      "restore failure focus",
    );
    assert(
      all(".fm-trash input:checked").length === 2,
      "failure keeps checked songs",
    );
    await capture("restore_rejected_light");
    await theme("dark");
    await capture("restore_rejected_dark");
    await fixture("restore-lost");
    click(restore);
    await wait(
      () =>
        button("核对恢复状态", one(".fm-recovery") ?? document) &&
        restore.disabled,
      "restore uncertainty",
    );
    assert(
      all(".fm-trash input:checked").length === 2,
      "unknown retains checked songs",
    );
    await capture("restore_uncertain_dark");
    restore.click();
    click(button("核对恢复状态", one(".fm-recovery")));
    await wait(
      () =>
        !all(".fm-trash [data-song-id]").length &&
        !one(".fm-trash").getAttribute("aria-busy").includes("true"),
      "restore confirmed",
    );
    await capture("restore_success_dark");
    click(button("取消屏蔽", one(".fm-artists")));
    await wait(() => !one(".fm-artists"), "artist restored");
    await capture("artist_unblocked_dark");
    click(button("返回私人漫游"));
    await prepare();
    await capture("returned_fm_dark");
    await show();
    await fixture("trash-lost");
    click(button("仅减少此歌曲", dialog()));
    await wait(
      () =>
        dialog()?.querySelector('[role="alert"]') &&
        button("查看反馈状态", dialog()),
      "uncertain feedback",
    );
    assert(button("仅减少此歌曲", dialog()).disabled, "unknown cannot resend");
    await capture("uncertain_dark");
    click(button("查看反馈状态", dialog()));
    await wait(
      () =>
        !one(".fm-dialog[open]") &&
        document.activeElement === one(".fm-recovery"),
      "unknown recovery focus",
    );
    await capture("uncertain_review_dark");
    click(button("核对并继续", one(".fm-recovery")));
    await wait(() => !one(".fm-recovery"), "reconciled");
    await prepare();
    await capture("reconciled_dark");
    await emit("complete", {
      ok: true,
      fmFeedbackUi: true,
      stageCount: count,
      realMusicRequests: 0,
      realAccountWrites: 0,
    });
  } catch (error) {
    await emit("failed", {
      ok: false,
      last,
      count,
      error: String(error),
      active: document.activeElement?.outerHTML.slice(0, 300),
      body: document.body.textContent.slice(-2200),
    });
  }
})();
