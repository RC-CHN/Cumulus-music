// Production browsing/history with synthetic account-bound collection APIs.
(async () => {
  const invoke = (command, args = {}) =>
    window.__TAURI_INTERNALS__.invoke(command, args);
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
  const wait = async (test, label) => {
    for (let i = 0; i < 600; i++) {
      if (await test()) return;
      await sleep(40);
    }
    throw Error(label);
  };
  const send = (stage, value = {}) =>
    invoke("plugin:event|emit", {
      event: "ux-proof",
      payload: { stage, ...value },
    });
  window.__collectionContextAck = {};
  let command = 0,
    count = 0,
    last = "start";
  async function mode(value) {
    const id = ++command;
    await send("collection-context-fixture", { id, mode: value });
    await wait(
      () => window.__collectionContextAck[id],
      "fixture acknowledgement",
    );
    return window.__collectionContextAck[id];
  }
  async function emit(stage, value) {
    window.__CUMULUS_ALBUM_CAPTURE_ACK__ = null;
    await send(stage, value);
    if (stage.startsWith("ux_"))
      await wait(
        () => window.__CUMULUS_ALBUM_CAPTURE_ACK__ === stage,
        "native capture acknowledgement",
      );
  }
  function click(node) {
    assert(node?.isConnected && !node.disabled, "available control");
    if (!bounds(node)) node.scrollIntoView({ block: "nearest" });
    node.focus({ preventScroll: true });
    node.click();
  }
  async function settle() {
    await wait(async () => {
      await new Promise((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(resolve)),
      );
      return !document.getAnimations().some((animation) => {
        const end = animation.effect?.getComputedTiming().endTime;
        return (
          typeof end === "number" &&
          Number.isFinite(end) &&
          (animation.pending || animation.playState === "running")
        );
      });
    }, "finite transitions");
  }
  const ready = () =>
    one(".collections-page")?.getAttribute("aria-busy") === "false";
  const loaded = () =>
    Number(
      one(".collections-tools>.muted")?.textContent.match(/已加载 (\d+)/)?.[1],
    );
  const input = () => one('[aria-label="筛选收藏列表"]');
  const scroller = () => one(".catalog-card-scroll,.collection-follow-scroll");
  const focusKey = (node) =>
    node?.dataset?.collectionKey ?? node?.dataset?.catalogKey ?? null;
  async function filter(value) {
    const node = input();
    node.focus({ preventScroll: true });
    node.value = value;
    node.dispatchEvent(new Event("input", { bubbles: true }));
    await sleep(0);
    await settle();
  }
  async function choose(label) {
    click(button(label, one(".collection-tabs")));
    await wait(
      () =>
        ready() &&
        button(label, one(".collection-tabs"))?.getAttribute(
          "aria-selected",
        ) === "true",
      "scope " + label,
    );
    await settle();
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
  async function refresh(completed = ready) {
    click(button("刷新收藏列表"));
    await wait(completed, "refresh completion");
    await settle();
  }
  async function retry() {
    click(button("重试", one(".collections-page")));
    // A click can observe the old aria-busy=false before Vue flushes the
    // request's loading state. Require the retry's error removal as well;
    // the first-page fixture deliberately holds its response for 750 ms.
    await wait(() => ready() && !one(".catalog-error"), "retry completion");
    await settle();
  }
  async function more() {
    const before = loaded();
    click(button("加载更多"));
    await wait(
      () => ready() && (loaded() > before || one(".catalog-error")),
      "continuation",
    );
    await settle();
  }
  function bounds(node) {
    if (!(node instanceof HTMLElement) || !node.isConnected) return null;
    const box = node.getBoundingClientRect();
    let left = Math.max(0, box.left),
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
        return null;
      if (["auto", "scroll", "hidden", "clip"].includes(style.overflowX)) {
        left = Math.max(left, rect.left);
        right = Math.min(right, rect.right);
      }
      if (["auto", "scroll", "hidden", "clip"].includes(style.overflowY)) {
        top = Math.max(top, rect.top);
        bottom = Math.min(bottom, rect.bottom);
      }
    }
    return right - left > 1 && bottom - top > 1
      ? { left, right, top, bottom }
      : null;
  }
  async function scroll(top) {
    const node = scroller();
    assert(node, "list scroller");
    node.scrollTop = top;
    node.dispatchEvent(new Event("scroll"));
    await settle();
    await sleep(30);
  }
  function visibleCard() {
    const cards = all("button.catalog-card,button.collection-follow-open");
    return (
      cards.find((node) => {
        const b = bounds(node),
          rect = node.getBoundingClientRect();
        return b && b.bottom - b.top >= rect.height - 1;
      }) ??
      cards.find((node) => {
        const b = bounds(node),
          rect = node.getBoundingClientRect();
        return b && b.bottom - b.top >= Math.min(60, rect.height - 1);
      })
    );
  }
  function observe() {
    const dialog = one("dialog[open]");
    const root =
        one("dialog[open]") ??
        one(".collections-page") ??
        one(".welcome") ??
        one(".catalog-page"),
      active = document.activeElement,
      scroll = scroller();
    const focusSamples = [];
    if (
      active?.matches(
        ".collection-follow-open:focus-visible,[data-collection-key='control:refresh']:focus-visible",
      )
    ) {
      const box = active.getBoundingClientRect();
      focusSamples.push({
        x: box.left + 1,
        y: box.top + box.height / 2,
        color: getComputedStyle(active).outlineColor,
      });
      focusSamples.push({
        x: box.right - 1,
        y: box.top + box.height / 2,
        color: getComputedStyle(active).outlineColor,
      });
    }
    if (active === input()) {
      const label = active.closest(".collections-search"),
        box = label.getBoundingClientRect();
      focusSamples.push({
        x: box.left + 1,
        y: box.top + box.height / 2,
        color: getComputedStyle(label).outlineColor,
      });
    }
    const controls = all("button", root ?? document)
      .filter((node) => !node.disabled && bounds(node))
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
    return {
      width: innerWidth,
      height: innerHeight,
      ratio: devicePixelRatio,
      screenX,
      screenY,
      captureTitle: "Cumulus",
      visible: document.visibilityState === "visible",
      focused: document.hasFocus(),
      theme: document.documentElement.dataset.theme,
      loaded: Number.isFinite(loaded()) ? loaded() : null,
      keyword: input()?.value ?? null,
      selected:
        one('.collection-tabs [aria-selected="true"]')?.textContent.trim() ??
        null,
      scrollTop: scroll?.scrollTop ?? 0,
      focus: focusKey(active),
      focusLabel: active?.getAttribute("aria-label") ?? active?.tagName,
      focusValid: active !== document.body && !!bounds(active),
      focusPath: (() => {
        const list = [];
        for (let node = active; node; node = node.parentElement) {
          const s = getComputedStyle(node),
            r = node.getBoundingClientRect();
          list.push({
            tag: node.tagName,
            classes: node.className,
            opacity: s.opacity,
            display: s.display,
            visibility: s.visibility,
            overflowX: s.overflowX,
            overflowY: s.overflowY,
            rect: {
              left: r.left,
              right: r.right,
              top: r.top,
              bottom: r.bottom,
            },
            scrollTop: node.scrollTop,
          });
        }
        return list;
      })(),
      overflow:
        document.documentElement.scrollWidth > innerWidth ||
        (!!scroll && scroll.scrollWidth > scroll.clientWidth + 1),
      cards: all(".catalog-card,.collection-follow-card").length,
      controls,
      error: one(".collections-page .catalog-error")?.textContent.trim() ?? "",
      empty: one(".collections-page .empty-state")?.textContent.trim() ?? "",
      tabs: all(".collection-tabs button").map((node) => ({
        label: node.textContent.trim(),
        height: node.getBoundingClientRect().height,
        font: getComputedStyle(node).fontSize,
        line: getComputedStyle(node).lineHeight,
        selected: node.getAttribute("aria-selected"),
      })),
      paintSamples: dialog
        ? []
        : [
            ...focusSamples,
            {
              x: innerWidth - 4,
              y: innerHeight - 4,
              color: getComputedStyle(one(".playerbar")).backgroundColor,
            },
          ],
      dialogs: dialog
        ? (() => {
            const box = dialog.getBoundingClientRect(),
              style = getComputedStyle(dialog);
            return [
              {
                open: true,
                opacity: style.opacity,
                background: style.backgroundColor,
                rect: {
                  left: box.left,
                  top: box.top,
                  width: box.width,
                  height: box.height,
                },
              },
            ];
          })()
        : [],
    };
  }
  async function capture(label, extra = {}) {
    last = label;
    await settle();
    const value = { ...observe(), ...extra };
    await emit(`ux_${String(++count).padStart(2, "0")}_${label}`, value);
    assert(
      value.visible && value.focused && value.focusValid,
      "visible focus " + label,
    );
    assert(!value.overflow && value.cards <= 36, "bounded layout " + label);
    assert(
      (value.width === 1120 && value.height === 760) ||
        (value.width === 760 && value.height === 540),
      "exact CSS viewport",
    );
    assert(
      value.controls.every((control) => control.hit),
      "button hit targets " + label,
    );
    assert(
      value.tabs.every(
        (tab) =>
          Math.abs(tab.height - 32) < 0.1 &&
          tab.font === "14px" &&
          tab.line === "18px",
      ),
      "official selection tag geometry",
    );
    return value;
  }
  async function backCollection(expected) {
    click(one('[aria-label="返回上一页"]'));
    await wait(
      () => ready() && loaded() === expected,
      "collection history return",
    );
    await settle();
  }
  async function sameAccount() {
    const old = one(".collections-page"),
      account = await invoke("account_snapshot");
    // The native event is only an invalidation hint; bootstrap reads the actual account again.
    await invoke("plugin:event|emit", {
      event: "cumulus:account-changed",
      payload: { ...account, canSignOut: !account.canSignOut },
    });
    await wait(
      () =>
        one(".collections-page") && one(".collections-page") !== old && ready(),
      "same-account remount",
    );
    await settle();
  }
  try {
    await wait(() => one(".track-row"), "startup");
    await emit("resize", { width: 1120, height: 760, scale: devicePixelRatio });
    await wait(
      () => innerWidth === 1120 && innerHeight === 760,
      "initial CSS viewport",
    );
    await theme("light");
    await mode("fail_saved");
    click(button("专辑与关注"));
    await wait(() => ready() && one(".catalog-error"), "first page error");
    await capture("saved_first_error_light");
    await mode("delay_first");
    await retry();
    assert(
      loaded() === 50,
      "initial retry loaded page: " + loaded(),
    );
    assert(
      focusKey(document.activeElement) === "control:refresh",
      "initial retry focus: " + focusKey(document.activeElement),
    );
    await capture("saved_first_recovered_light");
    await mode("none");
    await more();
    await filter("收藏专辑");
    await scroll(1800);
    const source = visibleCard();
    assert(source, "visible album");
    source.focus({ preventScroll: true });
    const before = await capture("saved_context_light"),
      reads = (await mode("read")).calls.length;
    click(source);
    await wait(() => one(".album-header"), "album detail");
    await capture("album_detail_light");
    await backCollection(100);
    assert(
      input().value === "收藏专辑" &&
        Math.abs(scroller().scrollTop - before.scrollTop) < 1 &&
        focusKey(document.activeElement) === before.focus,
      "exact album return",
    );
    assert(
      (await mode("read")).calls.length === reads,
      "history does not reread first page",
    );
    await capture("saved_return_light");
    await sameAccount();
    assert(
      loaded() === 100 &&
        input().value === "收藏专辑" &&
        Math.abs(scroller().scrollTop - before.scrollTop) < 1,
      "same account retains context",
    );
    await capture("saved_same_account_light");
    await choose("已购专辑");
    await theme("dark");
    assert(
      input().value === "收藏专辑" && loaded() === 2,
      "album tabs share filter",
    );
    await capture("purchased_shared_filter_dark");
    await filter("1002");
    await capture("purchased_filter_dark");
    click(visibleCard());
    await wait(() => one(".album-header"), "purchase detail");
    await capture("purchased_detail_dark");
    await backCollection(2);
    assert(input().value === "1002", "purchase filter return");
    await capture("purchased_return_dark");
    click(button("清空筛选"));
    await settle();
    assert(document.activeElement === input(), "clear retains input");
    await capture("purchased_cleared_dark");
    await choose("收藏专辑");
    assert(
      loaded() === 100 && input().value === "",
      "saved cached prefix/shared clear",
    );
    await capture("saved_tab_return_dark");
    await mode("fail_more");
    await more();
    assert(
      loaded() === 100 && one(".catalog-error"),
      "continuation failure preserves prefix",
    );
    await capture("saved_more_error_dark");
    await choose("已购专辑");
    await choose("收藏专辑");
    await theme("light");
    assert(
      one(".catalog-error") && loaded() === 100,
      "continuation retry restored",
    );
    await capture("saved_error_return_light");
    await mode("none");
    await retry();
    assert(
      loaded() === 123 && !one(".catalog-error"),
      "continuation retries exact page",
    );
    await capture("saved_more_recovered_light");
    await mode("fail_saved");
    await refresh();
    await theme("dark");
    assert(
      loaded() === 123 && one(".catalog-error"),
      "refresh preserves entire prefix",
    );
    await capture("saved_refresh_error_dark");
    await choose("已购专辑");
    await choose("收藏专辑");
    assert(one(".catalog-error") && loaded() === 123, "refresh retry restored");
    await capture("saved_refresh_error_return_dark");
    await mode("none");
    await retry();
    await theme("light");
    assert(
      loaded() === 50 && scroller().scrollTop === 0,
      "refresh replaces prefix at top",
    );
    await capture("saved_refresh_recovered_light");
    await mode("delay_more");
    click(button("加载更多"));
    await wait(
      async () => (await mode("read")).pending,
      "delayed native request",
    );
    await choose("已购专辑");
    await wait(
      async () => (await mode("read")).delayedCompleted > 0,
      "old response completed",
    );
    assert(loaded() === 2 && !one(".catalog-error"), "late scope read ignored");
    await capture("purchased_after_late_read_light");
    await mode("none");
    await choose("收藏专辑");
    assert(loaded() === 50, "unfinished prefix not claimed loaded");
    await capture("saved_after_late_read_light");
    await filter("台湾 a [live]");
    await wait(
      () =>
        ready() &&
        loaded() === 100 &&
        one(".collections-tools").textContent.includes("匹配 1"),
      "normalized later-page search",
    );
    await theme("dark");
    await capture("saved_normalized_filter_dark");
    click(button("清空筛选"));
    await settle();
    assert(document.activeElement === input(), "normalized clear focus");
    await capture("saved_clear_filter_dark");
    input().dispatchEvent(
      new CompositionEvent("compositionstart", { bubbles: true }),
    );
    input().value = "臺";
    input().dispatchEvent(
      new InputEvent("input", { bubbles: true, isComposing: true, data: "臺" }),
    );
    await sleep(100);
    assert(
      !one(".collections-tools").textContent.includes("匹配"),
      "IME partial value not submitted",
    );
    await capture("saved_composing_dark");
    input().value = "臺灣 Ａ [Live]";
    input().dispatchEvent(
      new CompositionEvent("compositionend", { bubbles: true }),
    );
    await wait(
      () => one(".collections-tools").textContent.includes("匹配 1"),
      "complete composition",
    );
    await capture("saved_composed_dark");
    click(button("每日推荐"));
    await wait(() => one(".track-row"), "daily");
    click(button("专辑与关注"));
    await wait(() => ready() && loaded() === 50, "new collection visit");
    await theme("light");
    assert(input().value === "", "new visit separate filter");
    await capture("saved_second_visit_light");
    await more();
    await filter("专辑 2");
    await capture("saved_second_filtered_light");
    click(visibleCard());
    await wait(() => one(".album-header"), "second visit detail");
    await backCollection(100);
    assert(input().value === "专辑 2", "second visit restored");
    await capture("saved_second_return_light");
    click(one('[aria-label="返回上一页"]'));
    await wait(() => one(".track-row"), "intermediate daily");
    await backCollection(100);
    await theme("dark");
    assert(
      input().value === "臺灣 Ａ [Live]",
      "first visit independently retained",
    );
    await capture("saved_first_visit_return_dark");
    await choose("关注歌手");
    assert(
      loaded() === 20 && input().value === "",
      "independent follow filter",
    );
    await capture("followed_first_dark");
    await more();
    await filter("关注歌手");
    await scroll(400);
    const artist = visibleCard();
    assert(artist, "visible artist");
    artist.focus({ preventScroll: true });
    const follow = await capture("followed_context_dark");
    click(artist);
    await wait(() => one(".artist-header"), "artist detail");
    await capture("artist_detail_dark");
    await backCollection(40);
    assert(
      input().value === "关注歌手" &&
        Math.abs(scroller().scrollTop - follow.scrollTop) < 1 &&
        focusKey(document.activeElement) === follow.focus,
      "exact artist return",
    );
    await capture("followed_return_dark");
    await sameAccount();
    assert(
      loaded() === 40 && input().value === "关注歌手",
      "follow account remount",
    );
    await capture("followed_same_account_dark");
    await mode("fail_follow");
    await refresh();
    await theme("light");
    assert(
      loaded() === 40 && one(".catalog-error"),
      "follow error preserves prefix",
    );
    await capture("followed_refresh_error_light");
    await mode("none");
    await retry();
    assert(
      loaded() === 20 && scroller().scrollTop === 0,
      "follow refresh replaces prefix",
    );
    await capture("followed_refresh_recovered_light");
    click(button("清空筛选"));
    await settle();
    await mode("empty_follow");
    await refresh();
    await theme("dark");
    assert(
      loaded() === 0 &&
        one(".collections-page .empty-state")?.textContent.includes(
          "还没有关注歌手",
        ),
      "follow empty distinct",
    );
    await capture("followed_empty_dark");
    await mode("none");
    await refresh();
    await emit("resize", { width: 760, height: 540, scale: devicePixelRatio });
    await wait(
      () => innerWidth === 760 && innerHeight === 540,
      "small CSS viewport",
    );
    await capture("followed_small_dark");
    await choose("已购专辑");
    if (input().value) {
      click(button("清空筛选"));
      await settle();
    }
    await mode("empty_purchase");
    await refresh();
    await theme("light");
    assert(
      loaded() === 0 &&
        one(".collections-page .empty-state")?.textContent.includes(
          "还没有已购专辑",
        ),
      "purchase empty distinct",
    );
    await capture("purchased_empty_light");
    await mode("fail_purchase");
    // The old empty response is still visible before Vue publishes this
    // refresh's loading state. Wait for the new error, not the old idle DOM.
    await refresh(() => ready() && one(".catalog-error"));
    assert(
      one(".catalog-error") && !one(".collections-page .empty-state"),
      "error not empty",
    );
    await capture("purchased_error_light");
    await mode("none");
    await retry();
    await theme("dark");
    assert(loaded() === 2, "purchase recovered");
    await capture("purchased_recovered_dark");
    await mode("expire");
    await refresh();
    await wait(
      () => one(".welcome") && !one(".collections-page"),
      "expired collection removed",
    );
    await theme("light");
    await capture("expired_light");
    await theme("dark");
    await capture("expired_dark");
    click(button("重新登录", one(".welcome")));
    await wait(() => one("dialog[open]"), "relogin dialog");
    await capture("expired_login_dark");
    const close = all("dialog[open] button").find((node) =>
      node.getAttribute("aria-label")?.startsWith("关闭"),
    );
    click(close);
    await wait(() => !one("dialog[open]"), "close relogin");
    await capture("expired_login_closed_dark");
    await send("complete", {
      ok: true,
      collectionContext: true,
      stageCount: count,
      realMusicRequests: 0,
      realAccountWrites: 0,
      playbackVerified: false,
      syntheticResponses: true,
    });
  } catch (error) {
    await emit("ux_99_diagnostic", { ...observe(), diagnostic: true });
    await send("failed", {
      last,
      stageCount: count,
      message: String(error?.message ?? error),
      stack: error?.stack,
      focus: document.activeElement?.outerHTML.slice(0, 500),
    });
  }
})();
