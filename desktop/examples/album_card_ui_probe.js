// Production album cards, synthetic APIs/artwork and native libmpv/null audio.
(async () => {
  const invoke = (name, args = {}) =>
    window.__TAURI_INTERNALS__.invoke(name, args);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (value, label) => {
    if (!value) throw Error(label);
  };
  const all = (selector, root = document) =>
    [...root.querySelectorAll(selector)].filter(
      (node) => !node.closest(".page-leave-active,.catalog-tab-leave-active"),
    );
  const one = (selector) => all(selector)[0];
  const button = (label, root = document) =>
    all("button", root).find(
      (node) =>
        node.textContent.trim() === label ||
        node.getAttribute("aria-label") === label,
    );
  const emit = (stage, value = {}) =>
    invoke("plugin:event|emit", {
      event: "ux-proof",
      payload: { stage, ...value },
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
          ? String(value).includes("synthetic-missing")
            ? "data:image/png;base64,AAAA"
            : "data:image/svg+xml," +
              encodeURIComponent(
                '<svg xmlns="http://www.w3.org/2000/svg" width="320" height="320"><rect width="320" height="320" fill="#66528f"/><circle cx="160" cy="160" r="95" fill="#ce9aac"/></svg>',
              )
          : value,
      );
    },
  });
  window.__albumCardAck = {};
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
  async function settle() {
    await wait(async () => {
      await new Promise((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(resolve)),
      );
      return !document.getAnimations().some((animation) => {
        const end = animation.effect?.getComputedTiming().endTime;
        return (
          Number.isFinite(end) &&
          (animation.pending || animation.playState === "running")
        );
      });
    }, "finite transitions");
  }
  function bounds(node) {
    if (!(node instanceof HTMLElement) || !node.isConnected) return null;
    const rect = node.getBoundingClientRect();
    let left = Math.max(0, rect.left),
      right = Math.min(innerWidth, rect.right),
      top = Math.max(0, rect.top),
      bottom = Math.min(innerHeight, rect.bottom);
    for (let parent = node; parent; parent = parent.parentElement) {
      const style = getComputedStyle(parent),
        box = parent.getBoundingClientRect();
      if (
        style.display === "none" ||
        style.visibility === "hidden" ||
        Number(style.opacity) === 0
      )
        return null;
      if (["auto", "scroll", "hidden", "clip"].includes(style.overflowX)) {
        left = Math.max(left, box.left);
        right = Math.min(right, box.right);
      }
      if (["auto", "scroll", "hidden", "clip"].includes(style.overflowY)) {
        top = Math.max(top, box.top);
        bottom = Math.min(bottom, box.bottom);
      }
    }
    return right - left > 1 && bottom - top > 1
      ? { left, right, top, bottom }
      : null;
  }
  function click(node) {
    assert(node?.isConnected && !node.disabled, "enabled control");
    if (!bounds(node)) node.scrollIntoView({ block: "nearest" });
    node.focus({ preventScroll: true });
    node.click();
  }
  async function mode(value) {
    const id = ++command;
    await emit("album-card-fixture", { id, mode: value });
    await wait(() => window.__albumCardAck[id], "fixture ACK");
    return window.__albumCardAck[id];
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
  const card = (id = 7) =>
    one(`[data-catalog-key="album:${id}"]`)?.closest(".catalog-album-card");
  const play = (id = 7) => card(id)?.querySelector(".catalog-album-play");
  const opener = (id = 7) => card(id)?.querySelector(".catalog-card");
  const snapshot = () => invoke("player_snapshot", { epoch });
  async function native(state, id) {
    await wait(async () => {
      const current = await snapshot();
      return (
        current.state === state &&
        one(".playerbar")?.dataset.state === state &&
        (!id || current.origin?.id === String(id)) &&
        (!play(id) || !play(id).disabled) &&
        (state !== "playing" || current.position > 0.2)
      );
    }, "native " + state);
    await settle();
    return snapshot();
  }
  function same(actual, expected, queue = true) {
    for (const key of [
      "playId",
      "entryId",
      ...(queue ? ["queueRevision"] : []),
    ])
      assert(actual[key] === expected[key], "retained " + key);
    assert(
      actual.track.id === expected.track.id &&
        actual.position + 0.15 >= expected.position,
      "retained song/position",
    );
  }
  async function menu(id = 7, keyboard = false) {
    const anchor = keyboard
      ? opener(id)
      : card(id)?.querySelector(".menu-trigger");
    assert(anchor, "menu origin");
    if (keyboard) {
      anchor.focus({ preventScroll: true });
      anchor.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "F10",
          shiftKey: true,
          bubbles: true,
          cancelable: true,
        }),
      );
    } else click(anchor);
    await wait(() => one('[role="menu"]'), "menu");
    await settle();
  }
  async function action(key) {
    click(one(`[role="menu"] [data-menu-key="${key}"]`));
    await settle();
  }
  async function albums() {
    await wait(
      () => all(".catalog-album-card").length === 3,
      "three album cards",
    );
    await settle();
  }
  async function membership() {
    await wait(
      () =>
        one(".album-membership-dialog[open]") &&
        !one('.album-membership-dialog [role="status"]'),
      "membership read",
    );
    await settle();
  }
  async function closeMembership() {
    click(button("关闭专辑收藏确认"));
    await wait(() => !one(".album-membership-dialog"), "membership closed");
    await settle();
  }
  async function capture(label, extra = {}) {
    last = label;
    await settle();
    const dialog = one("dialog[open]"),
      menu = one('[role="menu"]');
    const root =
      dialog ?? menu ?? one(".collections-page") ?? one(".catalog-page");
    const active = document.activeElement;
    const controls = all("button", root)
      .filter((node) => !node.disabled && bounds(node))
      .map((node) => {
        const box = bounds(node),
          hit = document.elementFromPoint(
            (box.left + box.right) / 2,
            (box.top + box.bottom) / 2,
          );
        return {
          label: node.getAttribute("aria-label") ?? node.textContent.trim(),
          hit: node === hit || node.contains(hit),
        };
      });
    const current = await snapshot(),
      queue = await invoke("player_queue", { epoch });
    const paint = dialog ?? menu ?? one(".playerbar"),
      box = paint.getBoundingClientRect();
    const value = {
      width: innerWidth,
      height: innerHeight,
      ratio: devicePixelRatio,
      screenX,
      screenY,
      captureTitle: "Cumulus",
      visible: document.visibilityState === "visible",
      focused: document.hasFocus(),
      focusValid: active !== document.body && !!bounds(active),
      focusKey: active?.dataset?.catalogKey ?? null,
      focusLabel:
        active?.getAttribute("aria-label") ??
        active?.textContent.trim().slice(0, 80),
      theme: document.documentElement.dataset.theme,
      overflow:
        document.documentElement.scrollWidth > innerWidth ||
        (!!one(".catalog-card-scroll") &&
          one(".catalog-card-scroll").scrollWidth >
            one(".catalog-card-scroll").clientWidth + 1),
      cards: all(".catalog-album-card").length,
      controls,
      menuKeys: menu
        ? all("[data-menu-key]", menu).map((node) => node.dataset.menuKey)
        : [],
      metadata: all(".catalog-album-card").map((node) => ({
        name: node.querySelector("strong")?.textContent.trim(),
        credits:
          node.querySelector(".catalog-album-credits")?.textContent.trim() ??
          "",
        date:
          node.querySelector(".catalog-album-date")?.textContent.trim() ?? "",
        fallback: !!node.querySelector(".catalog-card-placeholder"),
        explicit: !!node.querySelector(".album-explicit"),
      })),
      playback: {
        state: current.state,
        track: current.track?.id,
        playId: current.playId,
        entryId: current.entryId,
        queueRevision: current.queueRevision,
        position: current.position,
        origin: current.origin,
        queueLen: current.queueLen,
      },
      queue: queue.tracks.map((entry) => ({
        entryId: entry.entryId,
        track: entry.track.id,
      })),
      error:
        one(".album-action-feedback.failed")?.textContent.trim() ??
        one(".album-membership-dialog .catalog-error")?.textContent.trim() ??
        "",
      empty: one(".collections-page .empty-state")?.textContent.trim() ?? "",
      keyword: one('[aria-label="筛选收藏列表"]')?.value ?? null,
      paintSamples: [
        {
          x: box.left + 4,
          y: box.top + box.height / 2,
          color: getComputedStyle(paint).backgroundColor,
        },
      ],
      ...extra,
    };
    if (!extra.diagnostic) {
      assert(
        value.visible && value.focused && value.focusValid && !value.overflow,
        "visible/focused layout",
      );
      assert(
        controls.every((control) => control.hit),
        "actual controls hit",
      );
      assert(value.cards <= 36, "bounded cards");
    }
    const stage = `ux_${String(++count).padStart(2, "0")}_${label}`;
    window.__CUMULUS_ALBUM_CAPTURE_ACK__ = null;
    await emit(stage, value);
    await wait(
      () => window.__CUMULUS_ALBUM_CAPTURE_ACK__ === stage,
      "capture ACK",
    );
  }
  try {
    await wait(() => one(".track-row"), "startup");
    epoch = (await invoke("account_snapshot")).epoch;
    await resize(1120, 760);
    await theme("light");
    click(one(".track-credit-links button"));
    await wait(() => one(".artist-header"), "artist");
    click(one('.artist-tabs [data-catalog-tab="albums"]'));
    await albums();
    opener().focus({ preventScroll: true });
    assert(
      card().querySelector(".catalog-album-credits").textContent.trim() ===
        "主要署名 / 测试歌手",
      "official primary credit order",
    );
    assert(
      card()
        .querySelector(".catalog-album-date")
        .textContent.includes("2023-11-"),
      "official date",
    );
    await wait(
      () => card(9).querySelector(".catalog-card-placeholder"),
      "decode failure fallback",
    );
    await capture("artist_cards_light");
    await theme("dark");
    opener(8).focus({ preventScroll: true });
    await capture("long_card_dark");
    click(card().querySelector(".catalog-album-credits button"));
    await wait(
      () => one(".artist-header h1")?.textContent.includes("相似歌手 13"),
      "credit artist",
    );
    await capture("credit_artist_dark");
    click(button("返回上一页"));
    await albums();
    await wait(
      () => document.activeElement?.dataset?.catalogKey === "album:7:artist:0",
      "credit return focus",
    );
    await capture("credit_return_dark");
    await menu(7, true);
    await capture("keyboard_menu_dark");
    await action("share");
    await wait(() => one(".track-share-dialog[open]"), "share");
    await capture("share_dark");
    click(button("关闭分享"));
    await wait(() => !one(".track-share-dialog"), "share close");
    await settle();
    await capture("share_closed_dark");
    await menu();
    await action("copy");
    await wait(
      () => one(".album-action-feedback")?.textContent.includes("链接已复制"),
      "copy",
    );
    await capture("copy_dark");
    click(button("关闭", one(".album-action-feedback")));
    click(play());
    let playing = await native("playing", 7);
    await invoke("player_action", {
      epoch,
      action: { kind: "seek", seconds: 3 },
    });
    await wait(async () => (await snapshot()).position >= 3, "seek");
    playing = await snapshot();
    await capture("playing_dark");
    click(play());
    let paused = await native("paused", 7);
    same(paused, playing);
    await capture("paused_dark");
    click(play());
    playing = await native("playing", 7);
    same(playing, paused);
    await capture("resumed_dark");
    await invoke("player_action", { epoch, action: { kind: "stop" } });
    await native("idle");
    await capture("stopped_dark");
    click(play());
    const restarted = await native("playing", 7);
    assert(restarted.playId !== playing.playId, "new playback after Stop");
    await capture("restarted_dark");
    click(play());
    paused = await native("paused", 7);
    await menu(8);
    await action("next");
    await wait(async () => (await snapshot()).queueLen === 6, "album inserted");
    const inserted = await snapshot();
    same(inserted, paused, false);
    assert(inserted.state === "paused", "queue keeps pause");
    await capture("next_album_dark");
    await invoke("player_action", { epoch, action: { kind: "next" } });
    await native("playing", 8);
    await capture("next_track_dark");
    click(play(8));
    paused = await native("paused", 8);
    await mode("album-offline");
    await menu();
    await action("next");
    await wait(() => one(".album-action-feedback.failed"), "queue failure");
    same(await snapshot(), paused);
    await capture("next_failed_dark");
    await mode("none");
    click(button("重试", one(".album-action-feedback")));
    await wait(
      () => one(".album-action-feedback")?.textContent.includes("已设为下一首"),
      "queue retry",
    );
    same(await snapshot(), paused, false);
    await capture("next_retry_dark");
    click(button("关闭", one(".album-action-feedback")));
    await menu();
    await action("download");
    await wait(
      () =>
        one("dialog.download-dialog[open],dialog.download-start-dialog[open]"),
      "download dialog",
    );
    await capture("download_dark");
    click(button("取消", one("dialog[open]")));
    await wait(() => !one("dialog[open]"), "download canceled");
    await capture("download_closed_dark");
    await menu(8);
    await action("collect");
    await membership();
    await capture("collect_confirm_dark");
    await closeMembership();
    await capture("collect_canceled_dark");
    await mode("album-offline");
    await menu();
    await action("collect");
    await membership();
    await wait(
      () => one(".album-membership-dialog .catalog-error"),
      "membership read failure",
    );
    await capture("collect_read_failed_dark");
    await mode("none");
    click(button("重新读取", one(".album-membership-dialog")));
    await membership();
    await wait(
      () => !one(".album-membership-dialog .catalog-error"),
      "membership read recovered",
    );
    await capture("collect_read_recovered_dark");
    await closeMembership();
    await mode("album-sub-reject");
    await menu();
    await action("collect");
    await membership();
    click(button("确认", one(".album-membership-dialog")));
    await wait(
      () => one(".album-membership-dialog .catalog-error"),
      "collect rejection",
    );
    await settle();
    await capture("collect_rejected_dark");
    await mode("none");
    click(button("确认", one(".album-membership-dialog")));
    await wait(() => !one(".album-membership-dialog"), "collect retry");
    await capture("collect_retried_dark");
    click(button("专辑与关注"));
    await wait(
      () =>
        card() &&
        one(".collections-page")?.getAttribute("aria-busy") === "false",
      "saved album",
    );
    await settle();
    await capture("saved_light_before_theme");
    await theme("light");
    const savedFilter = one('[aria-label="筛选收藏列表"]');
    savedFilter.focus({ preventScroll: true });
    savedFilter.value = "测试专辑";
    savedFilter.dispatchEvent(new Event("input", { bubbles: true }));
    await settle();
    await menu();
    assert(
      one('[data-menu-key="remove"]') && !one('[data-menu-key="collect"]'),
      "saved menu",
    );
    await capture("saved_menu_light");
    await action("remove");
    await membership();
    await capture("remove_confirm_light");
    await closeMembership();
    await capture("remove_canceled_light");
    await menu();
    await action("remove");
    await membership();
    click(button("确认", one(".album-membership-dialog")));
    await wait(
      () =>
        !one(".album-membership-dialog") &&
        one(".collections-page .empty-state"),
      "removed",
    );
    assert(
      one('[aria-label="筛选收藏列表"]').value === "测试专辑",
      "membership keeps latest filter",
    );
    await capture("removed_empty_light");
    click(button("返回上一页"));
    await albums();
    await menu();
    await mode("album-sub-lost");
    await action("collect");
    await membership();
    click(button("确认", one(".album-membership-dialog")));
    await wait(
      () =>
        button("核对结果", one(".album-membership-dialog")) &&
        !button("核对结果", one(".album-membership-dialog")).disabled,
      "uncertain journal",
    );
    await capture("lost_write_light");
    await closeMembership();
    await menu();
    await action("collect");
    await membership();
    await capture("pending_reopened_light");
    await mode("none");
    const writes = (await mode("read")).calls.filter(
      (call) => call.path === "/api/album/sub",
    ).length;
    click(button("核对结果", one(".album-membership-dialog")));
    await wait(() => !one(".album-membership-dialog"), "reconciled");
    assert(
      (await mode("read")).calls.filter(
        (call) => call.path === "/api/album/sub",
      ).length === writes,
      "reconcile must not resend write",
    );
    await capture("reconciled_light");
    click(button("专辑与关注"));
    await wait(() => card(), "reconciled collection");
    await settle();
    click(opener());
    await wait(() => one(".album-header"), "album detail");
    await settle();
    await capture("collected_detail_light");
    click(button("已收藏", one(".album-actions")));
    await wait(() => button("收藏", one(".album-actions")), "detail uncollect");
    click(button("返回上一页"));
    await wait(
      () => one(".collections-page .empty-state"),
      "detail invalidates history",
    );
    await capture("detail_removed_return_light");
    click(button("返回上一页"));
    await albums();
    await resize(760, 540);
    await theme("dark");
    opener().focus({ preventScroll: true });
    await capture("small_cards_dark");
    await menu();
    await capture("small_menu_dark");
    await action("share");
    await wait(() => one(".track-share-dialog[open]"), "small share");
    await capture("small_share_dark");
    click(button("关闭分享"));
    await wait(() => !one(".track-share-dialog"), "small share close");
    await capture("small_share_closed_dark");
    await invoke("player_action", { epoch, action: { kind: "stop" } });
    await emit("complete", {
      ok: true,
      albumCards: true,
      stageCount: count,
      realMusicRequests: 0,
      realAccountWrites: 0,
    });
  } catch (error) {
    const failedAt = last;
    try {
      await capture("diagnostic", { diagnostic: true, error: String(error) });
    } catch {}
    await emit("failed", {
      ok: false,
      albumCards: true,
      last: failedAt,
      message: String(error),
      stack: error?.stack,
      body: document.body.textContent.slice(0, 4500),
      active: document.activeElement?.outerHTML?.slice(0, 900),
    });
  }
})();
