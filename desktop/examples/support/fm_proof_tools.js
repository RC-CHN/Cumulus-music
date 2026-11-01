// Shared DOM/IPC driver for synthetic FM proofs; contains no business implementation.
window.createFmProof = () => {
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

  async function capture(label, extra = {}) {
    last = label;
    await sleep(250);
    await new Promise((resolve) =>
      requestAnimationFrame(() => requestAnimationFrame(resolve)),
    );
    const d = dialog(),
      panel = (d || one(".fm-page")).querySelector(".fm-state-recovery");
    const active = document.activeElement,
      samples = [];
    const root = one(".main-content");
    if (root) {
      const r = rect(root);
      samples.push({
        x: r.left + 4,
        y: r.top + 4,
        color: getComputedStyle(root).backgroundColor,
      });
    }
    assert(
      document.documentElement.scrollWidth <= innerWidth,
      "contained document",
    );
    assert(
      bar().dataset.state === "paused" &&
        bar().dataset.playId === playId &&
        Math.abs(Number(bar().dataset.position) - 10) < 0.2,
      "unchanged paused media",
    );
    if (d) {
      const r = rect(d);
      assert(
        r.left >= 8 &&
          r.right <= innerWidth - 8 &&
          r.top >= 8 &&
          r.bottom <= innerHeight - 8,
        "contained dialog",
      );
      assert(
        d.scrollWidth <= d.clientWidth + 1 && d.contains(active),
        "modal layout and focus",
      );
      for (const n of d.querySelectorAll("header button,footer button"))
        if (!n.disabled) assert(hit(n), "modal controls hit");
      samples.push({
        x: r.left + 6,
        y: r.top + r.height / 2,
        color: getComputedStyle(d).backgroundColor,
      });
    }
    if (panel) {
      const r = rect(panel),
        retry = button("重新读取状态", panel);
      if (retry && !retry.disabled)
        assert(hit(retry), "readonly recovery is reachable");
      if (d && retry) {
        const body = rect(d.querySelector(".fm-dialog-body")),
          target = rect(retry);
        assert(
          target.top >= body.top - 1 && target.bottom <= body.bottom + 1,
          "entire recovery button clears fixed footer",
        );
      }
      if (r.top >= 0 && r.bottom <= innerHeight)
        samples.push({
          x: r.left + 4,
          y: r.top + r.height / 2,
          color: getComputedStyle(panel).backgroundColor,
        });
    }
    const stage = `ux_${String(++count).padStart(2, "0")}_${label}`;
    window.__queueSelectionRecorded = null;
    await emit(stage, {
      captureHandshake: true,
      fmStateRecoveryUi: true,
      width: innerWidth,
      height: innerHeight,
      ratio: devicePixelRatio,
      visible: document.visibilityState === "visible",
      focused: document.hasFocus(),
      theme: document.documentElement.dataset.theme,
      playId,
      stateUnavailable: !!one(".fm-page").dataset.fmStateError,
      dialogOpen: !!d,
      submitDisabled: !!d?.querySelector("footer .primary")?.disabled,
      reasonDisabled: d
        ? [...d.querySelectorAll(".fm-reason")].every((n) => n.disabled)
        : null,
      review: !!d && !!button("查看反馈状态", d),
      panel: panel
        ? {
            text: panel.textContent,
            previous:
              panel.querySelector(".fm-state-previous")?.textContent ?? null,
            readIssue:
              panel.querySelector(".fm-state-read-issue")?.textContent ?? null,
            rect: rect(panel),
          }
        : null,
      active: {
        tag: active?.tagName,
        class: active?.className,
        text: active?.textContent.trim().slice(0, 80),
      },
      selected: Number(one(".fm-trash")?.dataset.selectedCount ?? 0),
      visibleSelected: all(".fm-trash input:checked").length,
      restoreDisabled: [
        ...document.querySelectorAll(
          ".fm-trash [data-song-id] button,.fm-trash-toolbar .primary",
        ),
      ].every((n) => n.disabled),
      paintSamples: samples,
      ...extra,
    });
    await wait(
      () => window.__queueSelectionRecorded === stage,
      "native capture " + label,
    );
  }
  return {
    one,
    all,
    sleep,
    invoke,
    emit,
    assert,
    wait,
    dialog,
    bar,
    button,
    click,
    rect,
    hit,
    fixture,
    prepare,
    show,
    close,
    theme,
    size,
    capture,
    setEpoch(value) {
      epoch = value;
    },
    get count() {
      return count;
    },
    get last() {
      return last;
    },
    get playId() {
      return playId;
    },
  };
};
