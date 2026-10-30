// Real WebView/menu/clipboard and production IPC; all account/audio data is synthetic.
(async () => {
  const one = (s) => document.querySelector(s),
    all = (s) => [...document.querySelectorAll(s)];
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (condition, message) => {
    if (!condition) throw Error(message);
  };
  const wait = async (condition, message) => {
    for (let i = 0; i < 200; i++) {
      if (condition()) return;
      await sleep(50);
    }
    throw Error(message);
  };
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const emit = (stage, data = {}) =>
    invoke("plugin:event|emit", {
      event: "ux-proof",
      payload: { stage, ...data },
    });
  const button = (label, root = document) =>
    [...root.querySelectorAll("button")].find(
      (n) =>
        n.getAttribute("aria-label") === label ||
        n.textContent.trim() === label,
    );
  const click = (node) => {
    assert(node && !node.disabled, "enabled control");
    node.focus({ preventScroll: true });
    node.click();
  };
  const menu = () => one(".song-action-menu");
  const more = () => one(".track-row .track-more");
  const keys = [
    "播放",
    "下一首播放",
    "查看评论",
    "收藏",
    "下载",
    "分享",
    "复制链接",
  ];
  let count = 0,
    step = "startup",
    failCopy = false,
    holdCopy = false,
    copyCalls = 0,
    playerCalls = 0;
  const originalFetch = window.fetch;
  window.fetch = async function (url, options) {
    if (String(url).endsWith("/copy_text")) {
      copyCalls++;
      if (holdCopy) await sleep(900);
      if (failCopy)
        return new Response(JSON.stringify("synthetic clipboard refusal"), {
          headers: {
            "Content-Type": "application/json",
            "Tauri-Response": "error",
          },
        });
    }
    if (
      String(url).endsWith("/play_selection") ||
      String(url).endsWith("/player_action")
    )
      playerCalls++;
    return originalFetch.call(window, url, options);
  };
  const key = (node, value, extra = {}) =>
    node.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: value,
        bubbles: true,
        cancelable: true,
        ...extra,
      }),
    );
  async function capture(label, evidence = {}) {
    step = label;
    await sleep(200);
    assert(
      document.documentElement.scrollWidth <= innerWidth,
      "document containment",
    );
    const panel = menu();
    if (panel) {
      const r = panel.getBoundingClientRect();
      assert(
        r.left >= 0 &&
          r.top >= 0 &&
          r.right <= innerWidth &&
          r.bottom <= innerHeight,
        "menu viewport containment",
      );
    }
    await emit(`ux_${String(++count).padStart(2, "0")}_${label}`, {
      ...evidence,
      width: innerWidth,
      height: innerHeight,
      theme: document.documentElement.dataset.theme,
      items: panel
        ? [...panel.querySelectorAll("button")].map((n) => n.textContent.trim())
        : [],
    });
    await sleep(420);
  }
  async function open(trigger = more()) {
    trigger.scrollIntoView({ block: "nearest" });
    await new Promise((r) =>
      requestAnimationFrame(() => requestAnimationFrame(r)),
    );
    click(trigger);
    await wait(() => menu()?.style.visibility === "visible", "menu opens");
    await sleep(130);
    assert(
      trigger.getAttribute("aria-expanded") === "true",
      "external trigger expanded",
    );
    return trigger;
  }
  async function escape(trigger) {
    key(menu(), "Escape");
    await wait(() => !menu(), "menu closes");
    assert(
      document.activeElement === trigger,
      "menu returns focus to exact trigger",
    );
    assert(
      trigger.getAttribute("aria-expanded") === "false",
      "external trigger collapsed",
    );
  }
  async function choose(label) {
    click(button(label, menu()));
    await wait(() => !menu(), "selection closes menu");
  }
  async function clipboard(text) {
    window.__searchClipboardAck = null;
    await emit("search-clipboard-check", { expected: text });
    await wait(
      () => window.__searchClipboardAck !== null,
      "clipboard acknowledgement",
    );
    assert(window.__searchClipboardAck, "native clipboard public link");
  }
  async function search(keyword, mode = "preview") {
    window.__searchFixtureAck = false;
    await emit("search-fixture", { mode });
    await wait(() => window.__searchFixtureAck, "search fixture");
    const previousPanel = one(".search-result-panel");
    const input = one('[aria-label="搜索音乐"]');
    input.value = keyword;
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.form.dispatchEvent(
      new Event("submit", { bubbles: true, cancelable: true }),
    );
    await wait(
      () =>
        one(".search-heading-copy h1")?.textContent === keyword &&
        one(".search-result-panel") !== previousPanel &&
        one(".search-result-panel")?.getAttribute("aria-busy") === "false" &&
        one('[data-category="all"]')?.getAttribute("aria-selected") === "true" &&
        !one(
          ".page-enter-active,.page-leave-active,.search-results-enter-active,.search-results-leave-active",
        ),
      "current search ready after navigation",
    );
  }
  try {
    await wait(() => more(), "daily ready");
    for (const [w, h, theme] of [
      [1120, 760, "light"],
      [1120, 760, "dark"],
      [760, 540, "light"],
      [760, 540, "dark"],
    ]) {
      step = `setup_${w}_${theme}`;
      click(button("每日推荐"));
      await wait(
        () =>
          one(".daily-heading-copy") &&
          more() &&
          !one(".page-enter-active,.page-leave-active"),
        "daily",
      );
      await emit("resize", { width: w, height: h });
      await wait(() => innerWidth === w && innerHeight === h, "resize");
      click(button("外观主题"));
      await wait(() => one(".action-menu"), "appearance");
      click(button(theme === "dark" ? "深色" : "浅色"));
      await wait(() => !one(".action-menu"), "theme applied");
      const trigger = await open();
      assert(
        JSON.stringify(
          [...menu().querySelectorAll("button")].map((n) =>
            n.textContent.trim(),
          ),
        ) === JSON.stringify(keys),
        "official ordinary-song item order",
      );
      assert(
        menu().querySelectorAll('[role="separator"]').length === 1,
        "action groups separated",
      );
      assert(
        document.activeElement === button("播放", menu()),
        "first menu choice focused",
      );
      key(menu(), "End");
      assert(
        document.activeElement === button("复制链接", menu()),
        "End reaches last action",
      );
      key(menu(), "ArrowDown");
      assert(document.activeElement === button("播放", menu()), "arrow wraps");
      key(menu(), "ArrowDown");
      key(menu(), "ArrowDown");
      key(menu(), "ArrowDown");
      assert(
        document.activeElement === button("收藏", menu()),
        "arrow skips separator",
      );
      await capture(`ordinary_${w}_${theme}`);
      await escape(trigger);
      trigger.closest(".track-row").dispatchEvent(
        new MouseEvent("contextmenu", {
          bubbles: true,
          cancelable: true,
          clientX: w - 4,
          clientY: h - 4,
        }),
      );
      await wait(() => menu()?.style.visibility === "visible", "right click");
      await capture(`edge_${w}_${theme}`);
      await escape(trigger);
      trigger.focus();
      key(trigger, "F10", { shiftKey: true });
      await wait(() => menu()?.style.visibility === "visible", "keyboard menu");
      await choose("收藏");
      await wait(() => one(".playlist-dialog[open]"), "collection");
      assert(
        one(".playlist-dialog h2").textContent === "收藏到歌单",
        "single-song collection is a dedicated action",
      );
      await capture(`collect_${w}_${theme}`);
      click(button("关闭歌单操作"));
      await wait(() => !one(".playlist-dialog"), "collection closed");
      assert(document.activeElement === trigger, "collection focus return");
      await open(trigger);
      await choose("下载");
      await wait(
        () => one('dialog[open] button[aria-label="关闭下载对话框"]'),
        "download dialog",
      );
      await capture(`download_${w}_${theme}`);
      click(button("关闭下载对话框"));
      await wait(() => !one("dialog"), "download closed");
      assert(document.activeElement === trigger, "download focus return");
      await open(trigger);
      await choose("分享");
      await wait(() => one(".track-share-dialog[open]"), "share dialog");
      assert(
        one('[aria-label="歌曲分享链接"]').value ===
          "https://music.163.com/song?id=1",
        "public share link",
      );
      click(button("复制链接", one(".track-share-dialog")));
      await wait(
        () => one(".track-share-status")?.textContent === "链接已复制",
        "share copy",
      );
      await clipboard("https://music.163.com/song?id=1");
      await capture(`share_${w}_${theme}`);
      click(button("关闭分享"));
      await wait(() => !one(".track-share-dialog"), "share closed");
      assert(document.activeElement === trigger, "share focus return");
      await open(trigger);
      const before = playerCalls;
      trigger.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
      assert(playerCalls === before, "more double click never plays");
      const scroller = one(".track-scroll"), retained = menu();
      scroller.dispatchEvent(new Event("scroll", { bubbles: false }));
      await sleep(80);
      assert(retained.isConnected && !retained.inert && menu() === retained,
        "queued scroll without anchor movement keeps menu");
      const oldTop = scroller.scrollTop, anchorTop = trigger.getBoundingClientRect().top;
      scroller.scrollTop = oldTop + 56;
      assert(scroller.scrollTop > oldTop && Math.abs(trigger.getBoundingClientRect().top - anchorTop) > 1,
        "scroll actually moves menu anchor");
      scroller.dispatchEvent(new Event("scroll", { bubbles: false }));
      await wait(() => !menu(), "moved anchor dismisses menu");
      scroller.scrollTop = oldTop;
      await capture(`scroll_dismissed_${w}_${theme}`);
      await search(`菜单预览${w}${theme}`);
      const card = one(".search-track-card"),
        cardMore = card.querySelector(
          ".search-track-actions button:last-child",
        );
      await open(cardMore);
      await capture(`preview_${w}_${theme}`);
      await escape(cardMore);
      await search(`菜单歌词${w}${theme}`, "lyrics");
      click(one('[data-category="lyrics"]'));
      await wait(() => one(".search-lyric-result .track-more"), "lyric row");
      const lyricMore = one(".search-lyric-result .track-more");
      await open(lyricMore);
      await capture(`lyrics_${w}_${theme}`);
      await escape(lyricMore);
    }
    step = "copy failure";
    click(button("每日推荐"));
    await wait(
      () =>
        one(".daily-heading-copy") &&
        more() &&
        !one(".page-enter-active,.page-leave-active"),
      "daily return",
    );
    const trigger = await open();
    holdCopy = true;
    failCopy = true;
    await choose("复制链接");
    await capture("copy_pending");
    await wait(
      () => one(".track-action-feedback.failed"),
      "copy failure visible",
    );
    await capture("copy_failed");
    failCopy = false;
    holdCopy = true;
    const attempts = copyCalls;
    const retryButton = button("重试", one(".track-action-feedback"));
    click(retryButton);
    await wait(
      () => retryButton.getAttribute("aria-disabled") === "true",
      "retry waiting",
    );
    click(retryButton);
    assert(copyCalls === attempts + 1, "pending retry suppressed");
    await capture("copy_retry_pending");
    holdCopy = false;
    await wait(
      () => one(".track-action-feedback")?.textContent === "链接已复制",
      "copy recovery",
    );
    assert(copyCalls === attempts + 1, "one explicit retry");
    assert(document.activeElement === trigger, "retry success returns focus");
    await clipboard("https://music.163.com/song?id=1");
    await capture("copy_recovered");
    step = "next-play";
    const epoch = (await invoke("account_snapshot")).epoch;
    await open(trigger);
    await choose("下一首播放");
    await wait(
      () => one(".track-action-feedback")?.textContent === "已开始播放",
      "empty queue next starts",
    );
    let queue = await invoke("player_queue", { epoch });
    assert(
      queue.tracks.length === 1 && queue.tracks[0].track.id === "1",
      "empty next selects one song",
    );
    await capture("empty_next");
    await open(trigger);
    await choose("播放");
    await wait(
      () => one(".queue-toggle small")?.textContent === "40",
      "play menu retains complete originating list",
    );
    const paused = await invoke("player_action", {
      epoch,
      action: { kind: "pause", paused: true },
    });
    const second = all(".track-row")[1].querySelector(".track-more");
    await open(second);
    await choose("下一首播放");
    await wait(
      () => one(".track-action-feedback")?.textContent === "已设为下一首播放",
      "next feedback",
    );
    const current = await invoke("player_action", {
      epoch,
      action: { kind: "pause", paused: true },
    });
    assert(
      current.playId === paused.playId && current.state === "paused",
      "queue insertion preserves active audio",
    );
    queue = await invoke("player_queue", { epoch });
    assert(
      queue.tracks.length === 40 && queue.tracks[1].track.id === "2",
      "existing song moved without duplicate",
    );
    click(button("播放队列"));
    await wait(() => one(".queue-panel"), "queue panel");
    await capture("next_queue");
    click(button("播放队列"));
    await wait(() => !one(".queue-panel"), "queue closed");
    await open();
    await choose("查看评论");
    await wait(
      () => one(".comments-page .error-state"),
      "comment failure feedback",
    );
    await capture("comments_failure");
    step = "owned playlist";
    click(button("我的歌单"));
    await wait(() => one(".playlist-card"), "playlist cards");
    const card = all(".playlist-card").find((n) =>
      n.textContent.includes("午后散步"),
    );
    click(card);
    await wait(() => one(".playlist-hero") && more(), "playlist detail");
    const ownTrigger = await open();
    assert(button("从歌单中删除", menu()), "owned playlist remove action");
    await capture("owned_menu");
    await choose("从歌单中删除");
    await wait(() => one(".playlist-dialog[open]"), "remove confirmation");
    assert(
      document.activeElement === button("取消"),
      "remove defaults to cancellation",
    );
    const confirmationGeometry = () => {
      const bounds = (label) => {
        const { width, height, top, bottom } = button(label)
          .getBoundingClientRect();
        return { width, height, top, bottom };
      };
      return {
        cancel: bounds("取消"),
        remove: bounds("删除"),
        dialog: one(".playlist-dialog").className,
        transform: getComputedStyle(one(".playlist-dialog")).transform,
      };
    };
    const entering = confirmationGeometry();
    // Transformed rectangles at different screen positions may round differently.
    // Inspect the actual resting layout, retaining strict equality of both buttons.
    await wait(
      () => {
        const dialog = one(".playlist-dialog[open]");
        return (
          dialog &&
          !dialog.classList.contains("modal-enter-active") &&
          dialog.getAnimations().every(
            (animation) => animation.playState === "finished",
          ) &&
          getComputedStyle(dialog).transform === "none"
        );
      },
      "remove confirmation entrance finished",
    );
    const settled = confirmationGeometry(),
      cancelBox = settled.cancel,
      removeBox = settled.remove;
    assert(
      cancelBox.width === removeBox.width &&
        cancelBox.height === removeBox.height &&
        cancelBox.top === removeBox.top &&
        cancelBox.bottom === removeBox.bottom &&
        cancelBox.width === 120 &&
        cancelBox.height >= 40,
      `confirmation buttons align: ${JSON.stringify({ entering, settled })}`,
    );
    assert(
      document.activeElement === button("取消"),
      "confirmation retains cancel focus",
    );
    await capture("remove_confirmation", { confirmation: { entering, settled } });
    click(button("取消"));
    await wait(() => !one(".playlist-dialog"), "remove cancelled");
    assert(document.activeElement === ownTrigger, "remove cancellation focus");
    document.documentElement.dataset.motion = "reduced";
    await open(ownTrigger);
    assert(
      menu().getAnimations().length === 0 &&
        getComputedStyle(menu())
          .transitionDuration.split(",")
          .every((v) => parseFloat(v) === 0),
      "reduced motion menu",
    );
    await capture("reduced_motion");
    await escape(ownTrigger);
    click(button("我的歌单"));
    await wait(() => one(".playlist-card"), "playlist list");
    window.__searchFixtureAck = false;
    await emit("search-fixture", { mode: "missing_song" });
    await wait(() => window.__searchFixtureAck, "missing song fixture");
    click(all(".playlist-card").find((n) => n.getAttribute("aria-label") === "合成歌单 5"));
    await wait(() => one(".playlist-hero") && more(), "unavailable playlist");
    const local = await open();
    assert(
      !local.closest(".track-row").classList.contains("current"),
      "unmatched local ID cannot impersonate playing online song",
    );
    assert(
      !button("查看评论", menu()) &&
        !button("分享", menu()) &&
        button("播放", menu()).disabled,
      "unmatched local row actions",
    );
    assert(
      document.activeElement === button("从歌单中删除", menu()),
      "local menu skips disabled group",
    );
    await capture("local_unavailable");
    await escape(local);
    const missing = await open(
      all(".track-row")[1].querySelector(".track-more"),
    );
    assert(
      button("播放", menu()).disabled &&
        button("下一首播放", menu()).disabled &&
        button("下载", menu()).disabled,
      "unavailable actions disabled",
    );
    assert(
      !button("分享", menu()) && !button("复制链接", menu()),
      "unavailable share omitted",
    );
    assert(
      document.activeElement === button("查看评论", menu()),
      "keyboard skips disabled actions",
    );
    await capture("unavailable");
    key(menu(), "ArrowDown");
    assert(
      document.activeElement === button("从歌单中删除", menu()),
      "keyboard skips disabled group",
    );
    await capture("unavailable_keyboard");
    await escape(missing);
    await emit("complete", {
      ok: true,
      stageCount: count,
      songMenu: true,
      clipboardVerified: true,
      queueVerified: true,
      scrollGeometryVerified: true,
      realMusicRequests: 0,
      realAccountWrites: 0,
    });
  } catch (error) {
    await emit("failed", {
      step,
      stageCount: count,
      message: String(error),
      stack: error?.stack,
      body: document.body.innerText.slice(0, 2600),
    });
  }
})();
