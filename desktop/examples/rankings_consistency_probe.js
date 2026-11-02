// Real production UI/IPC/Rust cache and native null audio; synthetic API only.
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
  const shown = (selector) =>
    all(selector).find(
      (node) => !node.closest(".page-leave-active,.menu-leave-active"),
    );
  const rank = () => shown(".rankings-page");
  const menu = () => shown(".ranking-year-menu");
  const main = () => document.querySelector(".main-content");
  const button = (label, root = document) =>
    all("button", root).find(
      (node) =>
        node.textContent.trim() === label ||
        node.getAttribute("aria-label") === label,
    );
  const ready = () => rank()?.getAttribute("aria-busy") === "false";
  const emit = (stage, data = {}) =>
    invoke("plugin:event|emit", {
      event: "ux-proof",
      payload: { stage, ...data },
    });
  let count = 0,
    last = "boot",
    epoch,
    sameAccountRefreshVerified = false;
  async function wait(test, label) {
    await sleep(0);
    for (let i = 0; i < 600; i++) {
      if (await test()) return;
      await sleep(40);
    }
    throw Error(label);
  }
  function click(node) {
    assert(node && !node.disabled, "enabled control");
    node.scrollIntoView({ block: "nearest" });
    node.focus({ preventScroll: true });
    node.click();
  }
  async function settle() {
    await wait(
      () =>
        !document.querySelector(
          ".page-enter-active,.page-leave-active,.menu-enter-active,.menu-leave-active",
        ),
      "page/menu transitions",
    );
    await new Promise((resolve) =>
      requestAnimationFrame(() => requestAnimationFrame(resolve)),
    );
    await wait(
      () =>
        !document.getAnimations().some((animation) => {
          const end = animation.effect?.getComputedTiming().endTime;
          return (
            typeof end === "number" &&
            Number.isFinite(end) &&
            (animation.playState === "running" || animation.pending)
          );
        }),
      "finite animations",
    );
    await new Promise((resolve) =>
      requestAnimationFrame(() => requestAnimationFrame(resolve)),
    );
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
  }
  async function mode(value) {
    await emit("fixture", { mode: value });
    await wait(
      () => window.__CUMULUS_ALBUM_MODE__ === value,
      "fixture acknowledged",
    );
  }
  async function resize(width, height) {
    await emit("resize", { width, height, scale: devicePixelRatio });
    await wait(
      () => innerWidth === width && innerHeight === height,
      "window size",
    );
  }
  async function home() {
    click(button("排行榜", document.querySelector(".sidebar")));
    await wait(() => ready() && rank()?.querySelector(".ranking-card"), "home");
    await settle();
  }
  async function accountRefresh() {
    const provides =
      document.querySelector("#app").__vue_app__._context.provides;
    const core =
      provides[
        Reflect.ownKeys(provides).find(
          (key) => typeof key === "symbol" && key.description === "core-client",
        )
      ];
    const original = core.accountSnapshot.bind(core);
    const before = rank(),
      position = main().scrollTop;
    let reads = 0;
    core.accountSnapshot = async () => {
      reads++;
      return original();
    };
    try {
      const account = await invoke("account_snapshot");
      await invoke("plugin:event|emit", {
        event: "cumulus:account-changed",
        payload: { ...account, canSignOut: !account.canSignOut },
      });
      await wait(
        () =>
          reads > 0 &&
          ready() &&
          rank() !== before &&
          document.activeElement.closest("[data-rank-id]")?.dataset.rankId ===
            "101",
        "same-account restoration",
      );
      assert(
        Math.abs(main().scrollTop - position) < 1,
        "same-account reading position",
      );
      await settle();
    } finally {
      core.accountSnapshot = original;
    }
  }
  async function back(test, label) {
    click(button("返回上一页"));
    await wait(test, label);
    await settle();
  }
  async function sales() {
    click(rank().querySelector('[data-rank-id="albumRank"] .rank-open'));
    await wait(
      () => ready() && rank()?.querySelector("[data-sale-id]"),
      "sales",
    );
    await settle();
  }
  async function pause() {
    await invoke("player_action", {
      epoch,
      action: { kind: "pause", paused: true },
    });
    await wait(
      () => document.querySelector(".playerbar")?.dataset.state === "paused",
      "paused",
    );
  }
  function rect(node) {
    const b = node.getBoundingClientRect();
    return {
      left: b.left,
      right: b.right,
      top: b.top,
      bottom: b.bottom,
      width: b.width,
      height: b.height,
    };
  }
  function painted(node) {
    const box = rect(node);
    let left = Math.max(0, box.left),
      right = Math.min(innerWidth, box.right),
      top = Math.max(0, box.top),
      bottom = Math.min(innerHeight, box.bottom);
    for (let parent = node; parent; parent = parent.parentElement) {
      const style = getComputedStyle(parent),
        b = rect(parent);
      if (
        style.display === "none" ||
        style.visibility === "hidden" ||
        Number(style.opacity) === 0
      )
        return null;
      if (["auto", "scroll", "hidden", "clip"].includes(style.overflowX)) {
        left = Math.max(left, b.left);
        right = Math.min(right, b.right);
      }
      if (["auto", "scroll", "hidden", "clip"].includes(style.overflowY)) {
        top = Math.max(top, b.top);
        bottom = Math.min(bottom, b.bottom);
      }
    }
    return right - left > 1 && bottom - top > 1
      ? { left, right, top, bottom }
      : null;
  }
  function facts() {
    const r = rank(),
      surface =
        menu() ??
        r ??
        shown(".catalog-page") ??
        shown(".comments-page") ??
        document.querySelector(".page-view");
    const active = document.activeElement;
    const buttons = all("button", surface).map((node) => {
      const box = painted(node);
      const hit =
        box &&
        document.elementFromPoint(
          (box.left + box.right) / 2,
          (box.top + box.bottom) / 2,
        );
      return {
        label: node.getAttribute("aria-label") ?? node.textContent.trim(),
        visible: !!box,
        disabled: node.disabled,
        hit: !box || hit === node || node.contains(hit),
      };
    });
    const topCard = r?.querySelector(".with-preview .ranking-card"),
      images = topCard ? all(".rank-preview-image", topCard).map(rect) : [];
    const yearMenu = menu();
    return {
      captureTitle: "Cumulus",
      width: innerWidth,
      height: innerHeight,
      ratio: devicePixelRatio,
      screenX,
      screenY,
      visible: document.visibilityState === "visible",
      focused: document.hasFocus(),
      theme: document.documentElement.dataset.theme,
      overflow:
        document.documentElement.scrollWidth > innerWidth ||
        main().scrollWidth > main().clientWidth + 1,
      scrollTop: main().scrollTop,
      period: r
        ?.querySelector('[aria-label="榜单周期"] [aria-pressed="true"]')
        ?.textContent.trim(),
      singles: r
        ?.querySelector('[aria-label="商品类型"] [aria-pressed="true"]')
        ?.textContent.trim(),
      year: r?.querySelector('[aria-label="榜单年份"]')?.textContent.trim(),
      firstSale: r
        ?.querySelector("[data-sale-id] .sale-album")
        ?.textContent.trim(),
      saleIds: r
        ? all("[data-sale-id]", r).map((node) => node.dataset.saleId)
        : [],
      songIds: r
        ? all("[data-sale-song-id]", r).map((node) => node.dataset.saleSongId)
        : [],
      cardIds: r
        ? all("[data-rank-id]", r).map((node) => node.dataset.rankId)
        : [],
      readError: r
        ?.querySelector("[data-ranking-read-error]")
        ?.textContent.trim(),
      empty: r?.querySelector(".empty-state")?.textContent.trim(),
      focusValid:
        active !== document.body && !!active?.isConnected && !!painted(active),
      focusLabel:
        active?.getAttribute("aria-label") ??
        active?.textContent.trim().slice(0, 80),
      focusedCard: active?.closest("[data-rank-id]")?.dataset.rankId,
      focusedAlbum: active?.closest("[data-sale-id]")?.dataset.saleId,
      focusedSong: active?.closest("[data-sale-song-id]")?.dataset.saleSongId,
      focusedAction: active?.dataset.rankAction,
      yearMenu: yearMenu
        ? {
            rect: rect(yearMenu),
            options: all("button", yearMenu).map((node) => ({
              label: node.textContent.trim(),
              rect: rect(node),
              checked: node.getAttribute("aria-checked"),
            })),
          }
        : null,
      topCard: topCard
        ? {
            heading: rect(topCard.querySelector(".rank-heading")),
            frequency: rect(topCard.querySelector(".rank-title-row > small")),
            images,
            rowFont: getComputedStyle(
              topCard.querySelector(".rank-preview-row"),
            ).fontSize,
          }
        : null,
      buttons,
      paintSamples:
        r && topCard && painted(topCard)
          ? [
              {
                x: rect(topCard).left + 5,
                y: rect(topCard).top + rect(topCard).height / 2,
                color: getComputedStyle(topCard).backgroundColor,
              },
            ]
          : [],
    };
  }
  async function capture(label, extra = {}) {
    last = label;
    await settle();
    const value = facts();
    const stage = "ux_" + String(++count).padStart(2, "0") + "_" + label;
    window.__CUMULUS_ALBUM_CAPTURE_ACK__ = "";
    await emit(stage, { rankingsUi: true, ...value, ...extra });
    await wait(
      () => window.__CUMULUS_ALBUM_CAPTURE_ACK__ === stage,
      "native snapshot",
    );
    assert(!value.overflow, "overflow " + label);
    assert(value.focusValid, "focus " + label);
    assert(
      value.buttons.every((item) => item.hit),
      "hit targets " + label,
    );
  }
  async function yearMenu() {
    click(button("榜单年份", rank()));
    await wait(menu, "year menu");
    await settle();
  }
  async function chooseYear(value) {
    click(button(String(value) + "年榜", menu()));
    await wait(
      () =>
        ready() &&
        rank()?.querySelector("[data-sale-id]") &&
        button("榜单年份", rank())?.textContent.includes(String(value)),
      "year result",
    );
    await settle();
  }
  try {
    await wait(() => document.querySelector(".track-row"), "startup");
    epoch = (await invoke("account_snapshot")).epoch;
    await resize(1120, 760);
    await theme("light");
    await home();
    await capture("home_light");
    const playCard = rank().querySelector(
      '[data-rank-id="19723756"] .rank-play',
    );
    playCard.scrollIntoView({ block: "nearest" });
    playCard.focus({ preventScroll: true });
    await capture("card_keyboard_light");
    click(playCard);
    await wait(
      () =>
        document.querySelector(".playerbar")?.dataset.state === "playing" &&
        Number(document.querySelector('[aria-label="播放进度"]')?.value) > 0.2,
      "native card playback",
    );
    await capture("card_playing_light", { playing: true, expectedQueue: 5 });
    await pause();
    let global = rank().querySelector('[data-rank-id="101"] .rank-open');
    global.scrollIntoView({ block: "center" });
    global.focus({ preventScroll: true });
    await theme("dark");
    await accountRefresh();
    global = rank().querySelector('[data-rank-id="101"] .rank-open');
    sameAccountRefreshVerified = true;
    await capture("global_dark", { sameAccountRefreshVerified });
    const homeScroll = main().scrollTop;
    click(global);
    await wait(() => shown(".playlist-hero"), "playlist detail");
    await back(
      () => ready() && rank()?.querySelector('[data-rank-id="101"]'),
      "home return",
    );
    assert(
      Math.abs(main().scrollTop - homeScroll) < 1,
      "home reading position",
    );
    assert(
      document.activeElement.closest("[data-rank-id]")?.dataset.rankId ===
        "101",
      "home source focus",
    );
    await theme("light");
    await capture("home_return_light");
    await sales();
    await capture("sales_daily_light");
    click(button("周榜", rank()));
    click(button("数字单曲", rank()));
    await theme("dark");
    await capture("weekly_singles_dark");
    const weeklyScroll = main().scrollTop;
    click(rank().querySelector('[data-sale-id="7"] .sale-album'));
    await wait(() => shown(".album-header"), "album detail");
    await capture("album_dark");
    await back(
      () => ready() && rank()?.querySelector("[data-sale-id]"),
      "sales return",
    );
    assert(
      button("周榜", rank()).getAttribute("aria-pressed") === "true" &&
        button("数字单曲", rank()).getAttribute("aria-pressed") === "true",
      "weekly singles restored",
    );
    assert(main().scrollTop === weeklyScroll, "sales reading position");
    assert(
      document.activeElement.closest("[data-sale-id]")?.dataset.saleId === "7",
      "album source focus",
    );
    await theme("light");
    await capture("weekly_return_light");
    await resize(760, 540);
    await yearMenu();
    await capture("year_menu_light");
    await chooseYear(2018);
    assert(
      button("数字单曲", rank()).getAttribute("aria-pressed") === "true",
      "year retains singles",
    );
    await theme("dark");
    await capture("historical_singles_dark");
    click(rank().querySelector('[data-sale-id="8"] .sale-album'));
    await wait(() => shown(".album-header"), "historical album");
    await capture("historical_album_dark");
    await back(
      () => ready() && rank()?.querySelector("[data-sale-id]"),
      "historical return",
    );
    assert(
      button("榜单年份", rank()).textContent.includes("2018") &&
        button("数字单曲", rank()).getAttribute("aria-pressed") === "true",
      "historical selection restored",
    );
    await theme("light");
    await capture("historical_return_light");
    click(button("周榜", rank()));
    await wait(
      () =>
        ready() &&
        rank()?.querySelector(".sale-album")?.textContent.includes("week单曲"),
      "historical to weekly",
    );
    await capture("historical_week_light");
    await back(
      () => ready() && button("榜单年份", rank())?.textContent.includes("2018"),
      "annual history",
    );
    await theme("dark");
    await capture("annual_history_dark");
    await yearMenu();
    const panel = menu();
    panel.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "End",
        bubbles: true,
        cancelable: true,
      }),
    );
    await capture("year_keyboard_dark");
    panel.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    );
    await wait(() => !menu(), "year escape");
    assert(
      document.activeElement === button("榜单年份", rank()),
      "year menu returns focus",
    );
    await home();
    click(rank().querySelector('[data-rank-id="songIndexRank"] .rank-open'));
    await wait(
      () => ready() && rank()?.querySelector(".song-sales-list"),
      "song sales",
    );
    await theme("light");
    await capture("song_sales_light");
    click(button("播放全部", rank()));
    await wait(
      () =>
        document.querySelector(".playerbar")?.dataset.state === "playing" &&
        Number(document.querySelector('[aria-label="播放进度"]')?.value) > 0.2,
      "native song sales playback",
    );
    await theme("dark");
    await capture("songs_playing_dark", { playing: true, expectedQueue: 5 });
    await pause();
    click(
      rank().querySelector(
        '[data-sale-song-id="1"] [data-rank-action="comments"]',
      ),
    );
    await wait(
      () => shown(".comments-page")?.querySelector(".comment-card"),
      "song comments",
    );
    await theme("light");
    await capture("song_comments_light");
    await back(
      () => ready() && rank()?.querySelector(".song-sales-list"),
      "song return",
    );
    assert(
      document.activeElement.closest("[data-sale-song-id]")?.dataset
        .saleSongId === "1" &&
        document.activeElement.dataset.rankAction === "comments",
      "comment source focus",
    );
    await theme("dark");
    await capture("song_return_dark");
    await home();
    await mode("rank-offline");
    click(button("刷新榜单", rank()));
    await wait(
      () => ready() && rank()?.querySelector("[data-ranking-read-error]"),
      "cached refresh failure",
    );
    assert(
      rank().querySelectorAll(".ranking-card").length === 8,
      "cached cards retained",
    );
    await capture("cached_error_dark");
    await mode("normal");
    click(button("重试", rank()));
    await wait(
      () => ready() && !rank()?.querySelector("[data-ranking-read-error]"),
      "cached retry",
    );
    await theme("light");
    await capture("cached_recovered_light");
    await mode("rank-empty");
    click(button("刷新榜单", rank()));
    await wait(
      () => ready() && rank()?.textContent.includes("暂无榜单"),
      "empty home",
    );
    await theme("dark");
    await capture("home_empty_dark");
    await mode("normal");
    click(button("刷新榜单", rank()));
    await wait(
      () => ready() && rank()?.querySelector(".ranking-card"),
      "home repopulated",
    );
    await theme("light");
    await capture("home_recovered_light");
    await sales();
    click(button("数字单曲", rank()));
    await yearMenu();
    await mode("rank-offline");
    click(button("2019年榜", menu()));
    await wait(
      () => ready() && rank()?.querySelector(".error-state"),
      "uncached year failure",
    );
    assert(
      !rank().querySelector(".empty-state") &&
        !rank().querySelector("[data-sale-id]"),
      "failure is not empty data",
    );
    await theme("dark");
    await capture("first_read_failed_dark");
    await mode("normal");
    click(button("重试读取", rank()));
    await wait(
      () => ready() && rank()?.querySelector("[data-sale-id]"),
      "uncached retry",
    );
    assert(
      rank()
        .querySelector(".sale-album")
        .textContent.includes("2019年year单曲"),
      "retry uses original year and product",
    );
    await theme("light");
    await capture("first_read_recovered_light");
    await mode("rank-empty-period");
    click(button("周榜", rank()));
    await wait(
      () =>
        ready() &&
        button("周榜", rank())?.getAttribute("aria-pressed") === "true",
      "weekly selection settled",
    );
    click(button("刷新榜单", rank()));
    await wait(
      () => ready() && rank()?.textContent.includes("该周期暂无榜单数据"),
      "empty period",
    );
    await theme("dark");
    await capture("period_empty_dark");
    await mode("normal");
    click(button("刷新榜单", rank()));
    await wait(
      () => ready() && rank()?.querySelector("[data-sale-id]"),
      "period recovered",
    );
    assert(
      button("周榜", rank()).getAttribute("aria-pressed") === "true" &&
        button("数字单曲", rank()).getAttribute("aria-pressed") === "true",
      "refresh keeps selected period",
    );
    await theme("light");
    await capture("period_recovered_light");
    await emit("complete", {
      ok: true,
      rankingsUi: true,
      stageCount: count,
      sameAccountRefreshVerified,
      realAccountWrites: 0,
      realMusicRequests: 0,
      physicalAudioOutputVerified: false,
    });
  } catch (error) {
    await emit("failed", {
      last,
      message: String(error?.message ?? error),
      stack: error?.stack,
      width: innerWidth,
      height: innerHeight,
      ratio: devicePixelRatio,
    });
  }
})();
