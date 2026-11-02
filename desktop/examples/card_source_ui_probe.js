// Production card flows, isolated APIs and real libmpv/null; only status/failure bits are injected.
(async () => {
  const invoke = (name, args = {}) =>
    window.__TAURI_INTERNALS__.invoke(name, args);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (value, label) => {
    if (!value) throw Error(label);
  };
  const all = (selector, root = document) => [
    ...root.querySelectorAll(selector),
  ];
  const one = (selector) =>
    all(selector).find(
      (node) =>
        !node.closest(".page-leave-active,.search-results-leave-active"),
    );
  const button = (label, root = document) =>
    all("button", root).find(
      (node) =>
        node.textContent.trim() === label ||
        node.getAttribute("aria-label") === label,
    );
  const emit = (stage, extra = {}) =>
    invoke("plugin:event|emit", {
      event: "ux-proof",
      payload: { stage, ...extra },
    });
  const source = Object.getOwnPropertyDescriptor(
    HTMLImageElement.prototype,
    "src",
  );
  Object.defineProperty(HTMLImageElement.prototype, "src", {
    ...source,
    set(value) {
      source.set.call(
        this,
        String(value).startsWith("https://")
          ? "data:image/svg+xml," +
              encodeURIComponent(
                '<svg xmlns="http://www.w3.org/2000/svg" width="320" height="320"><rect width="320" height="320" fill="#66528f"/><circle cx="160" cy="160" r="95" fill="#ce9aac"/></svg>',
              )
          : value,
      );
    },
  });
  window.__cardSourceAck = {};
  let epoch,
    count = 0,
    command = 0,
    last = "boot";
  async function wait(test, label) {
    for (let i = 0; i < 600; i++) {
      if (await test()) return;
      await sleep(40);
    }
    throw Error(label);
  }
  function click(node) {
    assert(node?.isConnected && !node.disabled, "enabled control");
    node.scrollIntoView({ block: "nearest" });
    node.focus({ preventScroll: true });
    node.click();
  }
  async function settle() {
    await wait(
      () =>
        !document.getAnimations().some((animation) => {
          const end = animation.effect?.getComputedTiming().endTime;
          return (
            typeof end === "number" &&
            Number.isFinite(end) &&
            (animation.pending || animation.playState === "running")
          );
        }),
      "finite animations",
    );
    await new Promise((resolve) =>
      requestAnimationFrame(() => requestAnimationFrame(resolve)),
    );
  }
  const snapshot = () => invoke("player_snapshot", { epoch });
  async function native(state, trigger) {
    await wait(async () => {
      const value = await snapshot();
      return (
        value.state === state &&
        one(".playerbar")?.dataset.state === state &&
        (!trigger || !trigger.disabled) &&
        (!["playing", "buffering"].includes(state) || value.position > 0.2)
      );
    }, "native " + state);
    await settle();
    return snapshot();
  }
  async function mode(value) {
    const id = ++command;
    await emit("card-source-fixture", { id, mode: value });
    await wait(
      () => window.__cardSourceAck[id],
      "audio fixture acknowledgement",
    );
    assert(!window.__cardSourceAck[id].error, "audio fixture accepted");
    return window.__cardSourceAck[id];
  }
  async function theme(value) {
    localStorage.setItem("cumulus.appearance.v1", value);
    dispatchEvent(
      new StorageEvent("storage", {
        key: "cumulus.appearance.v1",
        newValue: value,
      }),
    );
    await wait(() => document.documentElement.dataset.theme === value, "theme");
    await settle();
  }
  async function resize(width, height) {
    await emit("resize", { width, height, scale: devicePixelRatio });
    await wait(
      () => innerWidth === width && innerHeight === height,
      "CSS viewport",
    );
    await settle();
  }
  function libraryCard(id = "2") {
    return one(`[data-library-open="${id}"]`)?.closest("article");
  }
  const libraryPlay = (id) =>
    libraryCard(id)?.querySelector(".library-card-play,.library-index");
  function same(actual, expected) {
    for (const key of ["playId", "entryId", "queueRevision"])
      assert(actual[key] === expected[key], "retained " + key);
    assert(
      actual.track.id === expected.track.id &&
        actual.position + 0.1 >= expected.position,
      "retained song/position",
    );
  }
  function bounds(node) {
    if (!(node instanceof HTMLElement) || !node.isConnected) return false;
    let box = node.getBoundingClientRect(),
      left = Math.max(0, box.left),
      right = Math.min(innerWidth, box.right),
      top = Math.max(0, box.top),
      bottom = Math.min(innerHeight, box.bottom);
    for (let parent = node; parent; parent = parent.parentElement) {
      const style = getComputedStyle(parent),
        rect = parent.getBoundingClientRect();
      if (
        style.display === "none" ||
        style.visibility === "hidden" ||
        Number(style.opacity) === 0
      )
        return false;
      if (["hidden", "clip", "auto", "scroll"].includes(style.overflowX)) {
        left = Math.max(left, rect.left);
        right = Math.min(right, rect.right);
      }
      if (["hidden", "clip", "auto", "scroll"].includes(style.overflowY)) {
        top = Math.max(top, rect.top);
        bottom = Math.min(bottom, rect.bottom);
      }
    }
    return right - left > 1 && bottom - top > 1
      ? { left, right, top, bottom }
      : null;
  }
  const visible = (node) => !!bounds(node);
  async function capture(label, card, extra = {}) {
    last = label;
    await settle();
    const active = document.activeElement,
      focusValid = active !== document.body && visible(active);
    const root = one(".playlist-library-page") ?? one(".search-result-panel");
    const viewport = one(".library-scroll") ?? root;
    const main = one(".main-content");
    const current = await snapshot(),
      queue = await invoke("player_queue", { epoch });
    const focusSamples = [];
    if (active?.matches(".library-index:focus-visible")) {
      const rect = active.getBoundingClientRect();
      focusSamples.push({
        x: rect.left + 1,
        y: rect.top + rect.height / 2,
        color: getComputedStyle(active).outlineColor,
      });
    }
    const buttons = all("button", root)
      .filter(visible)
      .map((node) => {
        const b = bounds(node),
          hit = document.elementFromPoint(
            (b.left + b.right) / 2,
            (b.top + b.bottom) / 2,
          );
        return {
          label: node.getAttribute("aria-label") ?? node.textContent.trim(),
          hit: hit === node || node.contains(hit),
        };
      });
    const value = {
      width: innerWidth,
      height: innerHeight,
      ratio: devicePixelRatio,
      screenX,
      screenY,
      captureTitle: "Cumulus",
      visible: document.visibilityState === "visible",
      focused: document.hasFocus(),
      theme: document.documentElement.dataset.theme,
      focusValid,
      focusPath: [
        active,
        active?.parentElement,
        active?.parentElement?.parentElement,
        card,
      ]
        .filter(Boolean)
        .map((node) => {
          const box = node.getBoundingClientRect(),
            style = getComputedStyle(node);
          return {
            tag: node.tagName,
            classes: node.className,
            opacity: style.opacity,
            display: style.display,
            visibility: style.visibility,
            position: style.position,
            focusedWithin: node.matches(":focus-within"),
            rect: {
              left: box.left,
              right: box.right,
              top: box.top,
              bottom: box.bottom,
            },
          };
        }),
      paintSamples: [
        ...focusSamples,
        {
          x: innerWidth - 4,
          y: innerHeight - 4,
          color: getComputedStyle(one(".playerbar")).backgroundColor,
        },
      ],
      sourceFocus: card ? card.contains(active) : null,
      focusLabel:
        active?.getAttribute("aria-label") ??
        active?.textContent.trim().slice(0, 80),
      overflow:
        document.documentElement.scrollWidth > innerWidth ||
        main.scrollWidth > main.clientWidth + 1 ||
        viewport.scrollWidth > viewport.clientWidth + 1,
      widths: [
        "html",
        ".main-content",
        ".playlist-library-page",
        ".library-scroll",
        "#search-result-panel",
      ].map((selector) => {
        const node = document.querySelector(selector);
        return node
          ? {
              selector,
              client: node.clientWidth,
              scroll: node.scrollWidth,
              rect: node.getBoundingClientRect().width,
            }
          : null;
      }),
      buttons,
      player: current,
      queue,
      issue: one(".playback-message")?.textContent.trim(),
      controls: card
        ? all(".library-card-play,.library-index,.search-card-play", card).map(
            (node) => ({
              label: node.getAttribute("aria-label"),
              disabled: node.disabled,
            }),
          )
        : [],
      ...extra,
    };
    await emit(`ux_${String(++count).padStart(2, "0")}_${label}`, value);
    assert(value.visible && value.focused, "visible focused window " + label);
    assert(!value.overflow, "horizontal containment " + label);
    assert(focusValid, "visible focus " + label);
    assert(
      buttons.every((item) => item.hit),
      "visible control hit targets " + label,
    );
    if (card) assert(value.sourceFocus, "exact card focus " + label);
  }
  async function toggle(trigger, before, paused) {
    click(trigger);
    const next = await native(paused ? "paused" : "playing", trigger);
    same(next, before);
    assert(
      trigger.getAttribute("aria-label").startsWith(paused ? "播放" : "暂停"),
      "card follows pause state",
    );
    return next;
  }
  async function stop() {
    await invoke("player_action", { epoch, action: { kind: "stop" } });
    await native("idle");
  }
  const ready = () =>
    one("#search-result-panel")?.getAttribute("aria-busy") === "false";
  async function choose(category) {
    click(one(`[data-category="${category}"]`));
    await wait(
      () =>
        ready() &&
        one(`[data-category="${category}"]`)?.getAttribute("aria-selected") ===
          "true" &&
        one(".search-result-card"),
      "search category",
    );
    await settle();
    return one(".search-result-card");
  }
  try {
    await wait(() => one(".track-row"), "startup");
    epoch = (await invoke("account_snapshot")).epoch;
    await resize(1120, 760);
    await theme("light");
    click(button("我的歌单"));
    await wait(() => libraryCard(), "library");
    await settle();
    await capture("library_grid_light");
    let control = libraryPlay();
    click(control);
    let playing = await native("playing", control);
    await invoke("player_action", {
      epoch,
      action: { kind: "seek", seconds: 4 },
    });
    await wait(async () => (await snapshot()).position >= 4, "seek");
    playing = await snapshot();
    await capture("library_playing_light", libraryCard());
    await mode("buffer-on");
    playing = await native("buffering", control);
    assert(
      control.getAttribute("aria-label").startsWith("暂停"),
      "library buffering uses pause semantics",
    );
    await theme("dark");
    await capture("library_buffering_dark", libraryCard());
    let paused = await toggle(control, playing, true);
    await mode("buffer-off");
    await capture("library_buffer_paused_dark", libraryCard());
    await mode("fail-next-resume");
    click(control);
    await wait(
      () => one(".playback-message") && !control.disabled,
      "native failure feedback",
    );
    same(await snapshot(), paused);
    await capture("library_resume_failed_dark", libraryCard());
    await theme("light");
    click(button("重试", one(".playback-message")));
    playing = await native("playing", control);
    same(playing, paused);
    await capture("library_retry_light");
    await stop();
    control.focus({ preventScroll: true });
    await capture("library_stopped_light", libraryCard());
    click(control);
    let restarted = await native("playing", control);
    assert(
      restarted.playId !== playing.playId,
      "stopped source is selected again",
    );
    await capture("library_restarted_light", libraryCard());
    click(button("我创建的歌单：列表视图"));
    await settle();
    control = libraryPlay();
    control.focus({ preventScroll: true });
    await theme("dark");
    await capture("library_list_dark", libraryCard());
    paused = await toggle(control, restarted, true);
    await capture("library_list_paused_dark", libraryCard());
    await toggle(control, paused, false);
    await theme("light");
    await capture("library_list_resumed_light", libraryCard());
    control = libraryPlay("3");
    click(control);
    await native("playing", control);
    await capture("library_other_source_light", libraryCard("3"));
    await stop();
    const input = one('[aria-label="搜索音乐"]');
    input.value = "合成来源控制";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.form.dispatchEvent(
      new Event("submit", { bubbles: true, cancelable: true }),
    );
    await wait(() => one(".search-results-page") && ready(), "search ready");
    await settle();
    for (const [index, category] of [
      "playlists",
      "albums",
      "artists",
      "podcasts",
      "voices",
    ].entries()) {
      if (index === 2) await resize(760, 540);
      let card = await choose(category);
      control = card.querySelector(".search-card-play");
      await theme(index % 2 ? "dark" : "light");
      click(control);
      playing = await native("playing", control);
      await invoke("player_action", {
        epoch,
        action: { kind: "seek", seconds: 3 },
      });
      await wait(
        async () => (await snapshot()).position >= 3,
        "search source seek",
      );
      playing = await snapshot();
      await capture(`${category}_playing`, card);
      paused = await toggle(control, playing, true);
      await capture(`${category}_paused`, card);
      if (category === "voices") {
        click(card.querySelector(".menu-trigger"));
        await wait(() => one(".action-menu"), "voice menu");
        click(button("播放", one(".action-menu")));
        playing = await native("playing", control);
        same(playing, paused);
      } else playing = await toggle(control, paused, false);
      await capture(`${category}_resumed`, card);
      await stop();
      control.focus({ preventScroll: true });
      await capture(`${category}_stopped`, card);
      click(control);
      restarted = await native("playing", control);
      assert(
        restarted.playId !== playing.playId,
        "search stopped source reloads",
      );
      await capture(`${category}_restarted`, card);
      await stop();
    }
    await emit("complete", {
      ok: true,
      cardSources: true,
      stageCount: count,
      realAccountWrites: 0,
      realMusicRequests: 0,
      physicalAudioOutputVerified: false,
      syntheticBufferingStatus: true,
      syntheticResumeFailure: true,
      syntheticArtwork: true,
    });
  } catch (error) {
    await emit("failed", {
      last,
      stageCount: count,
      message: String(error?.message ?? error),
      stack: error?.stack,
      focus: document.activeElement?.outerHTML.slice(0, 500),
    });
  }
})();
