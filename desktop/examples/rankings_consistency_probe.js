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
  const globalRankOpen = (root = rank()) =>
    root?.querySelector('[data-rank-id="101"] [data-rank-action="open"]');
  async function accountRefresh(lateLayout = false) {
    const provides =
      document.querySelector("#app").__vue_app__._context.provides;
    const core =
      provides[
        Reflect.ownKeys(provides).find(
          (key) => typeof key === "symbol" && key.description === "core-client",
        )
      ];
    const original = core.accountSnapshot;
    const before = rank();
    const opener = globalRankOpen(before);
    assert(opener && !opener.disabled, "same-account ranking opener");
    opener.scrollIntoView({ block: "center", behavior: "instant" });
    opener.focus({ preventScroll: true });
    await settle();
    const viewport = () => ({
      top: main().scrollTop,
      max: main().scrollHeight - main().clientHeight,
      card: document.activeElement?.closest("[data-rank-id]")?.dataset.rankId,
      action: document.activeElement?.dataset.rankAction,
      ready: ready(),
      oldRankConnected: before.isConnected,
    });
    const baseline = viewport();
    assert(
      baseline.top > 0 && baseline.max > 0 && document.activeElement === opener,
      `same-account scrolled and focused baseline: ${JSON.stringify(baseline)}`,
    );
    const restored = () => {
      const current = rank();
      return current && current !== before && !before.isConnected && ready() &&
        document.activeElement === globalRankOpen(current) &&
        Math.abs(main().scrollTop - baseline.top) < 1;
    };
    let reads = 0;
    let held = false, temporaryMax;
    const originalRanking = core.rankingPage;
    let release;
    const gate = new Promise((resolve) => { release = resolve; });
    const clamp = document.createElement("style");
    if (lateLayout) {
      // The real read still runs. While its reply is held, make the retained
      // busy layout shorter than the saved offset. The browser must clamp an
      // early restoration; JS never assigns scrollTop during the refresh.
      clamp.textContent = `.rankings-page[aria-busy="true"] { max-height: ${before.offsetHeight - 40}px; overflow: clip; }`;
      document.head.append(clamp);
      core.rankingPage = async (...args) => {
        const reply = await originalRanking.call(core, ...args);
        if (args[1]?.kind === "home") {
          held = true;
          await gate;
        }
        return reply;
      };
    }
    core.accountSnapshot = async () => {
      reads++;
      return original.call(core);
    };
    try {
      const account = await invoke("account_snapshot");
      await invoke("plugin:event|emit", {
        event: "cumulus:account-changed",
        payload: { ...account, canSignOut: !account.canSignOut },
      });
      if (lateLayout) {
        await wait(
          () => held && !before.isConnected && !ready() && globalRankOpen(),
          "real ranking reply held after remount",
        );
        await settle();
        temporaryMax = main().scrollHeight - main().clientHeight;
        assert(temporaryMax < baseline.top, "busy layout cannot represent the saved offset");
        release();
      }
      await wait(
        () => reads === 1 && restored(),
        "same-account reading position and focus",
      );
      await settle();
      assert(restored(), "same-account viewport remains restored after paint");
      const finalMax = main().scrollHeight - main().clientHeight;
      if (lateLayout) assert(finalMax === baseline.max, "final layout recovered its full range");
      return { top: baseline.top, temporaryMax, finalMax };
    } catch (error) {
      throw Error(`${error.message}: baseline=${JSON.stringify(baseline)}, ` +
        `observed=${JSON.stringify(viewport())}, accountReads=${reads}, temporaryMax=${temporaryMax}`);
    } finally {
      release();
      clamp.remove();
      core.rankingPage = originalRanking;
      core.accountSnapshot = original;
    }
  }
  async function back(test, label) {
    click(button("返回上一页"));
    await wait(test, label);
    await settle();
  }
  async function openAtReadingPosition(control, block = "nearest") {
    assert(control && !control.disabled, "enabled navigation control");
    control.scrollIntoView({ block, behavior: "instant" });
    control.focus({ preventScroll: true });
    await settle();
    const position = main().scrollTop;
    // Do not call click(): its second scroll would change the position the
    // app actually checkpoints after we have sampled it.
    control.click();
    return position;
  }
  async function restoredViewport(position, focused, label) {
    try {
      await wait(() => Math.abs(main().scrollTop - position) < 1 && focused(), label);
    } catch {
      throw Error(`${label}: expected ${position}, observed ${main().scrollTop}, focus ${focused()}`);
    }
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
  const playerSnapshot = () => invoke("player_snapshot", { epoch });
  const playerQueue = () => invoke("player_queue", { epoch });
  async function sourceState() {
    return { player: await playerSnapshot(), queue: await playerQueue() };
  }
  function nativeAdvance(current) {
    const bar = document.querySelector(".playerbar");
    // The HTML range quantizes to its step and can show > 0.2 before the
    // underlying position crosses it. Verify the same play in both layers.
    return (
      current.state === "playing" &&
      current.position > 0.2 &&
      current.queueLen === 5 &&
      !current.error &&
      bar?.dataset.state === "playing" &&
      bar.dataset.playId === current.playId &&
      Number(bar.dataset.position) > 0.2
    );
  }
  function sameSource(actual, expected) {
    for (const key of ["playId", "entryId", "queueRevision"])
      assert(actual[key] === expected[key], "source retains " + key);
    assert(
      actual.track.id === expected.track.id,
      "source retains selected track",
    );
    assert(
      actual.origin?.kind === expected.origin?.kind &&
        actual.origin?.id === expected.origin?.id,
      "source retains origin",
    );
    assert(
      actual.position >= expected.position,
      "source retains listening position",
    );
  }
  async function nativePlaying(id) {
    await wait(async () => {
      const current = await playerSnapshot();
      const control = rank()?.querySelector(
        `[data-rank-id="${id}"] .rank-play`,
      );
      return (
        nativeAdvance(current) &&
        current.origin?.kind === "playlist" &&
        current.origin?.id === id &&
        control &&
        !control.disabled &&
        control.getAttribute("aria-pressed") === "true" &&
        control.getAttribute("aria-label").startsWith("暂停榜单")
      );
    }, "source native playback " + id);
    await settle();
    return playerSnapshot();
  }
  async function toggleSource(control, before, paused) {
    click(control);
    await wait(
      () =>
        document.querySelector(".playerbar")?.dataset.state ===
          (paused ? "paused" : "playing") && !control.disabled,
      "source transport settled",
    );
    await settle();
    const after = await playerSnapshot();
    sameSource(after, before);
    assert(
      control.getAttribute("aria-pressed") === String(!paused),
      "card follows native pause state",
    );
    return after;
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
      cardControls: r
        ? all(".rank-play", r).map((node) => ({
            id: node.closest("[data-rank-id]").dataset.rankId,
            label: node.getAttribute("aria-label"),
            pressed: node.getAttribute("aria-pressed"),
            opacity: getComputedStyle(node).opacity,
          }))
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
    let native = await nativePlaying("19723756");
    await capture("card_playing_light", { playing: true, expectedQueue: 5 });
    await invoke("player_action", {
      epoch,
      action: { kind: "seek", seconds: 4 },
    });
    await wait(
      async () => (await playerSnapshot()).position >= 4,
      "source seek",
    );
    native = await playerSnapshot();
    let held = await toggleSource(playCard, native, true);
    await theme("dark");
    await capture("card_paused_dark", { sourceControl: await sourceState() });
    native = await toggleSource(playCard, held, false);
    rank().querySelector("h1").focus({ preventScroll: true });
    await emit("pointer", { x: 10, y: 5 });
    await settle();
    assert(
      !playCard.matches(":hover") &&
        !playCard.closest(".ranking-card").matches(":focus-within") &&
        getComputedStyle(playCard).opacity === "1",
      "playing card remains visible without hover or focus",
    );
    await theme("light");
    await capture("card_resumed_light", {
      playing: true,
      expectedQueue: 5,
      sourceControl: await sourceState(),
    });
    await invoke("player_action", { epoch, action: { kind: "next" } });
    await wait(async () => {
      const p = await playerSnapshot();
      return p.state === "playing" && p.track?.id === "2" && p.position > 0.2;
    }, "next track in ranking");
    native = await playerSnapshot();
    await capture("card_next_light", {
      playing: true,
      expectedQueue: 5,
      sourceControl: await sourceState(),
    });
    held = await toggleSource(playCard, native, true);
    await theme("dark");
    await capture("card_next_paused_dark", {
      sourceControl: await sourceState(),
    });
    await toggleSource(playCard, held, false);
    await nativePlaying("19723756");
    await theme("light");
    await capture("card_next_resumed_light", {
      playing: true,
      expectedQueue: 5,
      sourceControl: await sourceState(),
    });
    const otherCard = rank().querySelector(
      '[data-rank-id="19723757"] .rank-play',
    );
    click(otherCard);
    const other = await nativePlaying("19723757");
    assert(
      other.playId !== native.playId && other.track.id === "1",
      "another source starts its own queue",
    );
    let staleControlRejected = false;
    try {
      await invoke("player_action", {
        epoch,
        action: {
          kind: "pause_for",
          play_id: native.playId,
          expected_queue_revision: native.queueRevision,
          paused: true,
        },
      });
    } catch (error) {
      staleControlRejected = error.code === "stale";
    }
    assert(
      staleControlRejected &&
        (await playerSnapshot()).state === "playing" &&
        (await playerSnapshot()).playId === other.playId,
      "late old card cannot pause the new source",
    );
    await theme("dark");
    await capture("other_card_playing_dark", {
      playing: true,
      expectedQueue: 5,
      staleControlRejected,
      sourceControl: await sourceState(),
    });
    await pause();
    await theme("dark");
    await accountRefresh();
    const layout = await accountRefresh(true);
    const global = globalRankOpen();
    sameAccountRefreshVerified = true;
    await capture("global_dark", {
      sameAccountRefreshVerified,
      expectedScrollTop: layout.top,
      busyScrollMax: layout.temporaryMax,
      settledScrollMax: layout.finalMax,
    });
    const globalPlay = rank().querySelector('[data-rank-id="101"] .rank-play');
    click(globalPlay);
    native = await nativePlaying("101");
    await capture("global_playing_dark", {
      playing: true,
      expectedQueue: 5,
      sourceControl: await sourceState(),
    });
    held = await toggleSource(globalPlay, native, true);
    await theme("light");
    await capture("global_paused_light", {
      sourceControl: await sourceState(),
    });
    await toggleSource(globalPlay, held, false);
    await nativePlaying("101");
    await theme("dark");
    await capture("global_resumed_dark", {
      playing: true,
      expectedQueue: 5,
      sourceControl: await sourceState(),
    });
    await pause();
    const homeScroll = await openAtReadingPosition(global, "center");
    assert(homeScroll > 0, "home reading position exercises a scrolled page");
    await wait(() => shown(".playlist-hero"), "playlist detail");
    await back(
      () => ready() && rank()?.querySelector('[data-rank-id="101"]'),
      "home return",
    );
    await restoredViewport(
      homeScroll,
      () => document.activeElement.closest("[data-rank-id]")?.dataset.rankId === "101",
      "home reading position",
    );
    await theme("light");
    await capture("home_return_light", { expectedScrollTop: homeScroll });
    await sales();
    await capture("sales_daily_light");
    click(button("周榜", rank()));
    click(button("数字单曲", rank()));
    await theme("dark");
    await capture("weekly_singles_dark");
    const weeklyScroll = await openAtReadingPosition(
      rank().querySelector('[data-sale-id="7"] .sale-album'),
    );
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
    await restoredViewport(
      weeklyScroll,
      () => document.activeElement.closest("[data-sale-id]")?.dataset.saleId === "7",
      "sales reading position",
    );
    await theme("light");
    await capture("weekly_return_light", { expectedScrollTop: weeklyScroll });
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
    const priorSalePlay = (await playerSnapshot()).playId;
    click(button("播放全部", rank()));
    await wait(async () => {
      const current = await playerSnapshot();
      return (
        nativeAdvance(current) &&
        current.playId !== priorSalePlay &&
        current.track?.id === "1"
      );
    }, "native song sales playback");
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
