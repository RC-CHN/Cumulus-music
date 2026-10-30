// Production library UI/IPC and null-output audio, synthetic in-memory account.
(async () => {
  const one = (s) => document.querySelector(s),
    all = (s) => [...document.querySelectorAll(s)];
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const assert = (v, m) => {
    if (!v) throw Error(m);
  };
  const wait = async (fn, m) => {
    for (let i = 0; i < 200; i++) {
      if (fn()) return;
      await sleep(50);
    }
    throw Error(m);
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
  const click = (n) => {
    assert(n && !n.disabled, "enabled control");
    n.scrollIntoView({ block: "nearest" });
    n.focus({ preventScroll: true });
    n.click();
  };
  const key = (n, k, extra = {}) =>
    n.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: k,
        bubbles: true,
        cancelable: true,
        ...extra,
      }),
    );
  const group = (k) => one(`.library-section[data-group="${k}"]`);
  const scroller = () => one(".library-scroll");
  const prefix = (k) => (k === "created" ? "我创建的歌单" : "我收藏的歌单");
  const fixture = async (mode) => {
    window.__libraryFixtureAck = null;
    await emit("library-fixture", { mode });
    await wait(() => window.__libraryFixtureAck, "fixture");
    return window.__libraryFixtureAck;
  };
  let step = "startup",
    count = 0,
    reads = 0;
  const fetch0 = window.fetch;
  window.fetch = async function (url, options) {
    if (String(url).endsWith("/account_playlists")) reads++;
    return fetch0.call(window, url, options);
  };
  async function capture(label) {
    step = label;
    await sleep(200);
    assert(document.documentElement.scrollWidth <= innerWidth, "root width");
    const s = scroller();
    if (s) {
      assert(s.scrollWidth <= s.clientWidth, "library width");
      assert(
        all(".playlist-library-page .playlist-card").length <= 40,
        "bounded page cards",
      );
    }
    for (const row of all(".library-row"))
      assert(
        Math.round(row.getBoundingClientRect().height) === 64,
        "official row height",
      );
    await emit(`ux_${String(++count).padStart(2, "0")}_${label}`, {
      width: innerWidth,
      height: innerHeight,
      theme: document.documentElement.dataset.theme,
      rendered: all(".playlist-library-page .playlist-card").length,
    });
    await sleep(420);
  }
  async function mode(k, value) {
    const b = button(`${prefix(k)}：${value === "grid" ? "卡片" : "列表"}视图`);
    click(b);
    await wait(
      () =>
        group(k)?.querySelector(
          value === "grid" ? ".playlist-grid" : ".library-table",
        ) && !one(".library-mode-enter-active,.library-mode-leave-active"),
      "mode transition",
    );
    assert(document.activeElement === b, "mode retains focus");
  }
  async function page(k, n) {
    const b = button(`${prefix(k)}第${n}页`);
    if (b.getAttribute("aria-current") !== "page") {
      click(b);
      await sleep(150);
    }
    await wait(
      () =>
        button(`${prefix(k)}第${n}页`)?.getAttribute("aria-current") ===
          "page" &&
        !one(".library-mode-enter-active,.library-mode-leave-active"),
      "group page",
    );
  }
  async function theme(value) {
    click(button("外观主题"));
    await wait(() => one(".action-menu"), "theme menu");
    click(button(value === "dark" ? "深色" : "浅色"));
    await wait(() => !one(".action-menu"), "theme");
  }
  async function library() {
    click(button("我的歌单"));
    await wait(
      () => scroller() && !one(".page-enter-active,.page-leave-active"),
      "library ready",
    );
  }
  async function closeMenu() {
    key(one(".action-menu"), "Escape");
    await wait(() => !one(".action-menu"), "menu closed");
  }
  try {
    await wait(() => one(".track-more"), "daily");
    await library();
    if (localStorage.getItem("cumulus.library.proof.finished")) {
      assert(
        group("created").querySelector(".library-table"),
        "created mode persists after process restart",
      );
      assert(
        group("subscribed").querySelector(".playlist-grid"),
        "subscribed mode independent after restart",
      );
      assert(
        button("我创建的歌单第1页").getAttribute("aria-current") === "page",
        "resource pagination not persisted",
      );
      await capture("reopened_modes");
      await theme("light");
      await capture("reopened_light");
      await emit("complete", {
        ok: true,
        reopened: true,
        libraryUi: true,
        stageCount: count,
        realMusicRequests: 0,
        realAccountWrites: 0,
      });
      return;
    }
    for (const [w, h, t] of [
      [1120, 760, "light"],
      [1120, 760, "dark"],
      [760, 540, "light"],
      [760, 540, "dark"],
    ]) {
      step = `setup_${w}_${t}`;
      await emit("resize", { width: w, height: h });
      await wait(() => innerWidth === w && innerHeight === h, "resize");
      await theme(t);
      await mode("created", "grid");
      await mode("subscribed", "grid");
      await page("created", 1);
      await page("subscribed", 1);
      scroller().scrollTop = 0;
      await sleep(70);
      assert(
        group("created").querySelectorAll(".playlist-card").length === 20,
        "created 20/page",
      );
      assert(
        group("subscribed").querySelectorAll(".playlist-card").length === 20,
        "subscribed 20/page",
      );
      assert(
        group("created").querySelector("h2").textContent.includes("60"),
        "created group total",
      );
      await capture(`grid_${w}_${t}`);
      const target = one('[data-library-open="2"]');
      target.focus({ preventScroll: true });
      await wait(
        () =>
          getComputedStyle(
            target.closest("article").querySelector(".library-card-play"),
          ).opacity === "1",
        "focused card reveals play",
      );
      await capture(`focused_${w}_${t}`);
      const trigger = target.closest("article").querySelector(".menu-trigger");
      click(trigger);
      await wait(
        () => one(".action-menu")?.style.visibility === "visible",
        "card menu",
      );
      assert(
        button("上移歌单").disabled,
        "first regular playlist remains at boundary",
      );
      await capture(`menu_${w}_${t}`);
      await closeMenu();
      assert(document.activeElement === trigger, "menu return");
      target.focus({ preventScroll: true });
      key(target, "F10", { shiftKey: true });
      await wait(
        () => one(".action-menu")?.style.visibility === "visible",
        "keyboard context menu",
      );
      await closeMenu();
      assert(
        document.activeElement === target,
        "keyboard context returns to card",
      );
      await mode("created", "list");
      scroller().scrollTop = 0;
      await capture(`created_list_${w}_${t}`);
      await mode("subscribed", "list");
      group("subscribed").scrollIntoView({ block: "start" });
      await capture(`subscribed_list_${w}_${t}`);
      await page("subscribed", 2);
      await capture(`subscribed_page2_${w}_${t}`);
      const open = group("subscribed").querySelector("[data-library-open]"),
        id = open.dataset.libraryOpen;
      const oldScroll = scroller().scrollTop;
      click(open);
      await wait(
        () =>
          one(".playlist-title") &&
          !one(".page-enter-active,.page-leave-active"),
        "detail",
      );
      click(button("返回上一页"));
      await wait(
        () => scroller() && !one(".page-enter-active,.page-leave-active"),
        "back",
      );
      assert(
        document.activeElement?.dataset.libraryOpen === id,
        "detail returns focus to original playlist",
      );
      assert(
        Math.abs(scroller().scrollTop - oldScroll) < 4,
        "detail returns to same scroll position",
      );
      assert(
        button("我收藏的歌单第2页").getAttribute("aria-current") === "page",
        "detail retains group page",
      );
      await capture(`returned_${w}_${t}`);
    }
    await mode("created", "grid");
    await mode("subscribed", "grid");
    scroller().scrollTop = 0;
    await sleep(100);
    let item = one('[data-library-open="2"]').closest("article");
    click(item.querySelector(".library-card-play"));
    await wait(
      () =>
        item
          .querySelector(".library-card-play")
          ?.getAttribute("aria-label")
          .startsWith("暂停"),
      "native playback",
    );
    await capture("playing");
    click(item.querySelector(".library-card-play"));
    await wait(
      () =>
        item
          .querySelector(".library-card-play")
          ?.getAttribute("aria-label")
          .startsWith("播放"),
      "paused",
    );
    await capture("paused");
    assert(
      one('[data-library-open="5"]')
        .closest("article")
        .querySelector(".library-card-play").disabled,
      "empty playlist play disabled",
    );
    click(item.querySelector(".menu-trigger"));
    await wait(
      () => one(".action-menu")?.style.visibility === "visible",
      "order menu",
    );
    click(button("下移歌单"));
    await wait(
      () =>
        group("created").querySelectorAll("[data-library-open]")[1]?.dataset
          .libraryOpen === "3",
      "order readback",
    );
    await capture("reordered");
    const order = await fixture("fail");
    assert(order.writes === 1, "one isolated order mutation");
    click(button("刷新当前页面"));
    await wait(() => one(".library-message.error-state"), "failure");
    assert(
      !one(".library-message.notice"),
      "refresh clears previous success feedback",
    );
    await capture("read_failed");
    assert(
      group("created").querySelectorAll(".playlist-card").length === 20,
      "refresh failure retains cards",
    );
    await fixture("hold");
    const retry = button("重试", one(".library-message")),
      before = reads;
    click(retry);
    click(retry);
    await capture("retry_wait");
    assert(reads === before + 1, "retry deduplicated");
    await fixture("success");
    await wait(
      () =>
        !one(".library-message.error-state") &&
        button("刷新当前页面").getAttribute("aria-disabled") === "false",
      "read recovered",
    );
    assert(
      document.activeElement === button("刷新当前页面"),
      "retry success moves to stable refresh control",
    );
    await capture("read_recovered");
    await fixture("empty");
    click(button("刷新当前页面"));
    await wait(
      () =>
        all(".library-empty").length === 2 && !one(".library-message.muted"),
      "empty",
    );
    await capture("empty_groups");
    assert(
      scroller().scrollTop === 0,
      "empty result resets old scroll position",
    );
    assert(
      group("created").getBoundingClientRect().top >=
        scroller().getBoundingClientRect().top,
      "empty group heading remains visible",
    );
    click(button("新建歌单", one(".library-toolbar")));
    await wait(() => one(".playlist-dialog[open]"), "create");
    await capture("empty_create");
    click(button("关闭歌单操作"));
    await wait(() => !one(".playlist-dialog"), "create close");
    await fixture("success");
    click(button("刷新当前页面"));
    await wait(() => button("我创建的歌单第3页"), "restored lists");
    await page("created", 3);
    await fixture("shrink");
    click(button("刷新当前页面"));
    await wait(
      () => group("created").querySelectorAll(".playlist-card").length === 2,
      "shrink clamps page",
    );
    assert(
      !group("created").querySelector(".library-pagination"),
      "shrunk pagination hidden",
    );
    await capture("shrunk");
    const setter = Storage.prototype.setItem;
    Storage.prototype.setItem = function (k, v) {
      if (k === "cumulus.playlist-library-layout.v1")
        throw Error("synthetic preference refusal");
      return setter.call(this, k, v);
    };
    await mode("created", "list");
    await wait(
      () => one(".library-message.notice")?.textContent.includes("未能保存"),
      "preference failure",
    );
    await capture("preference_failed");
    Storage.prototype.setItem = setter;
    await mode("created", "grid");
    window.__motionProof.system(true);
    await wait(
      () => document.documentElement.dataset.motion === "reduced",
      "reduced motion",
    );
    await mode("created", "list");
    await capture("reduced_motion");
    window.__motionProof.system(false);
    await fixture("expired");
    click(button("刷新当前页面"));
    await wait(() => one(".welcome"), "expired welcome");
    assert(!scroller(), "old account library unmounted");
    await capture("session_expired");
    localStorage.setItem("cumulus.library.proof.finished", "1");
    await emit("complete", {
      ok: true,
      libraryUi: true,
      stageCount: count,
      realMusicRequests: 0,
      realAccountWrites: 0,
      orderVerified: true,
      playbackVerified: true,
      returnVerified: true,
    });
  } catch (error) {
    await emit("failed", {
      step,
      count,
      message: String(error),
      focus: document.activeElement?.outerHTML?.slice(0, 250),
      page: one(".playlist-library-page")?.textContent?.slice(0, 400),
    });
  }
})();
