// Exercise actual page unmount/re-entry, with the native synthetic download fixture.
(async () => {
  const one = (s) => document.querySelector(s),
    all = (s) => [...document.querySelectorAll(s)];
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const assert = (value, message) => {
    if (!value) throw Error(message);
  };
  const wait = async (fn, message, attempts = 240) => {
    for (let i = 0; i < attempts; i++) {
      await sleep(50);
      if (fn()) return;
    }
    throw Error(message);
  };
  const emit = (stage, data = {}) =>
    window.__TAURI_INTERNALS__.invoke("plugin:event|emit", {
      event: "ux-proof",
      payload: { stage, ...data },
    });
  const button = (name, root = document) =>
    [...root.querySelectorAll("button")].find(
      (n) =>
        n.getAttribute("aria-label") === name || n.textContent.trim() === name,
    );
  const click = (el) => {
    assert(el && !el.disabled, "enabled control");
    el.focus({ preventScroll: true });
    el.click();
  };
  const fixture = async (mode) => {
    window.__downloadPageAck = null;
    await emit("download-page-fixture", { mode });
    await wait(
      () => window.__downloadPageAck,
      "fixture " + mode,
      mode === "seed" ? 1200 : undefined,
    );
    assert(window.__downloadPageAck.ok, window.__downloadPageAck.message);
  };
  const ready = () =>
    one(".downloads-page") &&
    !one(
      '#download-content[aria-busy="true"],.page-enter-active,.page-leave-active,.download-section-enter-active,.download-section-leave-active',
    );
  const resize = async (w, h) => {
    await emit("resize", { width: w, height: h });
    await wait(() => innerWidth === w && innerHeight === h, "resize");
  };
  const select = async (id) => {
    click(one("#download-tab-" + id));
    await wait(
      () =>
        ready() &&
        one("#download-tab-" + id)?.getAttribute("aria-selected") === "true" &&
        one(".download-row,.download-mv-card"),
      "category",
    );
  };
  const grid = () => one(".download-mv-scroll"),
    scroller = () => one(".download-mv-scroll,.download-scroll");
  const task = (row) => row.dataset.downloadTask ?? row.dataset.downloadMv;
  const byId = (id) =>
    all("[data-download-mv],[data-download-task]").find((n) => task(n) === id);
  const bounds = () => {
    const el = scroller(),
      r = el.getBoundingClientRect();
    return {
      top: el.scrollTop,
      first: all("[data-download-mv],[data-download-task]").find(
        (n) => n.getBoundingClientRect().bottom > r.top,
      )?.dataset.mvIndex,
      focus: document.activeElement?.dataset.downloadAction,
      focusTask: document.activeElement?.closest(
        "[data-download-mv],[data-download-task]",
      )?.dataset.downloadMv,
    };
  };
  let count = 0,
    step = "startup";
  async function capture(label, extra = {}) {
    step = label;
    await sleep(300);
    assert(
      document.documentElement.scrollWidth <= innerWidth,
      "no document overflow",
    );
    assert(
      all(".download-mv-card").length <= 32 &&
        all(".download-row").length <= 32,
      "bounded rows",
    );
    await emit("ux_" + String(++count).padStart(2, "0") + "_" + label, {
      width: innerWidth,
      height: innerHeight,
      ...(scroller() ? bounds() : {}),
      ...extra,
    });
    await sleep(400);
  }
  async function settings() {
    click(button("打开设置"));
    await wait(
      () =>
        one(".settings-page") && !one(".page-enter-active,.page-leave-active"),
      "settings opened",
    );
  }
  async function back() {
    click(button("返回上一页"));
    await wait(() => ready(), "return to downloads");
  }
  async function place(index) {
    const el = scroller();
    el.scrollTop =
      Math.floor(index / Number(el.dataset.downloadColumns ?? 1)) *
        Number(el.dataset.downloadPitch ?? 64) +
      7;
    el.dispatchEvent(new Event("scroll"));
    await wait(
      () =>
        one(
          '[data-mv-index="' +
            index +
            '"],[data-download-index="' +
            index +
            '"]',
        ),
      "target rendered",
    );
    await sleep(150);
  }
  try {
    await wait(() => one(".track-row"), "initial");
    await fixture("seed");
    await fixture("mv_window");
    await resize(1120, 760);
    click(button("下载管理"));
    await wait(() => ready(), "downloads");
    await select("mv");
    grid().scrollTop = grid().scrollHeight;
    grid().dispatchEvent(new Event("scroll"));
    await wait(() => !one(".download-continuation"), "full MV prefix");
    await place(120);
    const artist = button("Artist 17", grid());
    assert(artist, "identified artist beyond first read");
    const id = task(artist.closest("[data-download-mv]")),
      before = grid().scrollTop;
    await capture("mv_deep_before", { id });
    click(artist);
    await wait(
      () =>
        one('.catalog-page[aria-label="歌手详情"] h1')?.textContent.includes(
          "17",
        ),
      "artist",
    );
    await capture("artist_detail");
    await back();
    await wait(
      () =>
        Math.abs(grid().scrollTop - before) < 1 &&
        document.activeElement?.dataset.downloadAction === "artist:17",
      "artist return restores position and exact action",
    );
    assert(
      task(document.activeElement.closest("[data-download-mv]")) === id,
      "correct record focus",
    );
    await capture("artist_return");
    click(button("Artist 17", grid()));
    await wait(
      () => one('.catalog-page[aria-label="歌手详情"]'),
      "artist second",
    );
    await resize(760, 540);
    await back();
    await wait(
      () =>
        Number(grid().dataset.downloadColumns) === 2 &&
        document.activeElement?.dataset.downloadAction === "artist:17",
      "two-column return",
    );
    const card = byId(id),
      r = card.getBoundingClientRect(),
      g = grid().getBoundingClientRect();
    assert(
      r.bottom > g.top && r.top < g.bottom && grid().scrollTop > before,
      "anchor follows column count",
    );
    await capture("resized_return");
    const cover = card.querySelector(".download-mv-cover"),
      movieTop = grid().scrollTop;
    click(cover);
    await wait(
      () =>
        Number(one(".video-canvas")?.dataset.renderedFrames) > 0 &&
        one(".mv-play-facts")?.textContent.includes("本地文件"),
      "actual local playback",
    );
    await capture("offline_detail");
    await back();
    await wait(
      () =>
        Math.abs(grid().scrollTop - movieTop) < 1 &&
        document.activeElement?.dataset.downloadAction === "cover",
      "MV return",
    );
    assert(
      task(document.activeElement.closest("[data-download-mv]")) === id,
      "MV returns original card",
    );
    await capture("offline_return");
    click(button("关闭视频", one(".mv-mini")));
    await wait(() => !one(".mv-mini"), "mini video closes through native stop");
    // A missing anchor cannot restore a detached control; preserve nearby position.
    await place(0);
    const missing = task(one('[data-mv-index="0"]'));
    one('[data-mv-index="0"] .download-mv-cover').focus();
    await settings();
    await fixture("remove_first");
    await back();
    await wait(
      () => document.activeElement === grid(),
      "missing focus returns to grid",
    );
    assert(
      !byId(missing) && grid().scrollTop < 20,
      "deleted first item is not resurrected",
    );
    await capture("removed_anchor");
    await fixture("restore");
    await select("song");
    const table = one(".download-scroll");
    table.scrollTop = table.scrollHeight;
    table.dispatchEvent(new Event("scroll"));
    await wait(() => !one(".download-continuation"), "full song prefix");
    await place(80);
    const row = one('[data-download-index="80"]'),
      songId = task(row);
    row.focus({ preventScroll: true });
    const handle = one('[data-download-resize="1"]');
    handle.focus();
    handle.dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }),
    );
    row.focus({ preventScroll: true });
    await sleep(100);
    const widths = one(".download-table-header").getAttribute("style");
    await capture("song_deep_before");
    const songTop = table.scrollTop;
    await settings();
    await back();
    await wait(
      () =>
        Math.abs(one(".download-scroll").scrollTop - songTop) < 1 &&
        task(document.activeElement) === songId,
      "song return restores row",
    );
    assert(
      one(".download-table-header").getAttribute("style") === widths,
      "column width returns",
    );
    await capture("song_return");
    const input = one(".download-search input");
    input.value = "单曲 01";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await wait(
      () =>
        ready() &&
        one(".download-table")?.getAttribute("aria-rowcount") === "11",
      "filtered songs",
    );
    one('[data-download-index="4"]').scrollIntoView({ block: "nearest" });
    one('[data-download-index="4"]').focus({ preventScroll: true });
    await sleep(100);
    const filtered = task(document.activeElement);
    await settings();
    await back();
    await wait(
      () => task(document.activeElement) === filtered,
      "filtered focus",
    );
    assert(
      one(".download-search input").value === "单曲 01" &&
        one(".download-table").getAttribute("aria-rowcount") === "11",
      "filter returns",
    );
    await capture("filter_return");
    const provides = one("#app").__vue_app__._context.provides,
      client =
        provides[
          Reflect.ownKeys(provides).find(
            (key) =>
              typeof key === "symbol" && key.description === "core-client",
          )
        ],
      original = client.downloads;
    let fault = "",
      release,
      held = false;
    client.downloads = async (...args) => {
      if (fault === "hold") {
        held = true;
        await new Promise((resolve) => (release = resolve));
      } else if (fault === "fail")
        throw { code: "state", message: "合成返回读取失败", retryable: true };
      return original(...args);
    };
    await settings();
    fault = "hold";
    click(button("返回上一页"));
    await wait(() => held && one(".download-scroll"), "return read pending");
    const userRow = one('[data-download-index="0"]');
    one(".downloads-page").dispatchEvent(
      new PointerEvent("pointerdown", { bubbles: true }),
    );
    one(".download-scroll").scrollTop = 0;
    userRow.focus({ preventScroll: true });
    fault = "";
    release();
    await wait(() => ready(), "delayed return read completed");
    await sleep(300);
    assert(
      document.activeElement === userRow &&
        one(".download-scroll").scrollTop === 0,
      "new user action cancels pending restore",
    );
    await capture("interrupted_return");
    one('[data-download-index="4"]').scrollIntoView({ block: "nearest" });
    one('[data-download-index="4"]').focus({ preventScroll: true });
    await sleep(100);
    const retryTask = task(document.activeElement);
    await settings();
    fault = "fail";
    await back();
    await wait(
      () =>
        one(".error-state")?.textContent.includes("合成返回读取失败") &&
        task(document.activeElement) === retryTask,
      "cached return remains usable after read failure",
    );
    const focusedBounds = document.activeElement.getBoundingClientRect(),
      listBounds = scroller().getBoundingClientRect();
    assert(
      Math.min(focusedBounds.bottom, listBounds.bottom) -
        Math.max(focusedBounds.top, listBounds.top) >=
        Math.min(32, focusedBounds.height),
      "returned focus is visibly operable after error banner",
    );
    await capture("failed_return");
    fault = "";
    click(button("刷新状态"));
    await wait(
      () => ready() && !one(".error-state"),
      "refresh after return error",
    );
    assert(
      one(".download-search input").value === "单曲 01",
      "retry keeps query",
    );
    await capture("retried_return");
    client.downloads = original;
    // Starting a new category deliberately drops the old location.
    await select("voice");
    assert(
      one(".download-scroll").scrollTop === 0 &&
        !one(".download-search input").value,
      "new category starts fresh",
    );
    await capture("new_category");
    function historyRow(i) {
      return one('[data-download-index="' + i + '"]');
    }
    const trackDownloads = client.downloads;
    let latestQuery;
    client.downloads = async (e, q) => {
      const result = await trackDownloads(e, q);
      latestQuery = { ...q };
      return result;
    };
    // Distinct visits must survive intervening settings and another download category.
    await resize(1120, 760);
    await select("song");
    const historySearch = one(".download-search input");
    historySearch.value = "单曲";
    historySearch.dispatchEvent(new Event("input", { bubbles: true }));
    await wait(
      () =>
        ready() &&
        latestQuery?.keyword === "单曲" &&
        one(".download-table")?.getAttribute("aria-rowcount") === "121",
      "history song query",
    );
    const songList = one(".download-scroll");
    songList.scrollTop = songList.scrollHeight;
    songList.dispatchEvent(new Event("scroll"));
    await wait(() => !one(".download-continuation"), "history complete prefix");
    click(one('[data-download-sort="name"]'));
    await wait(() => ready() && historyRow(0), "history sort");
    // Sorting deliberately resets the prefix; load it again before saving the visit.
    const sortedList = one(".download-scroll");
    sortedList.scrollTop = sortedList.scrollHeight;
    sortedList.dispatchEvent(new Event("scroll"));
    await wait(() => !one(".download-continuation"), "history sorted prefix");
    await place(80);
    const historyA = task(historyRow(80));
    historyRow(80).focus({ preventScroll: true });
    historyRow(80)
      .querySelector('[data-download-action="manage"]')
      .focus({ preventScroll: true });
    await sleep(100);
    const historyATop = one(".download-scroll").scrollTop,
      historyAColumns = one(".download-table-header").getAttribute("style");
    await capture("history_song");
    await settings();
    await capture("history_settings_first");
    click(button("下载管理"));
    await wait(() => ready() && one(".download-row"), "second visit");
    await select("voice");
    const voiceSearch = one(".download-search input");
    voiceSearch.value = "声音";
    voiceSearch.dispatchEvent(new Event("input", { bubbles: true }));
    await wait(
      () =>
        ready() &&
        latestQuery?.keyword === "声音" &&
        latestQuery?.category === "voice" &&
        historyRow(6),
      "voice query",
    );
    await place(6);
    const historyB = task(historyRow(6));
    historyRow(6).focus({ preventScroll: true });
    await sleep(100);
    const historyBTop = one(".download-scroll").scrollTop;
    await capture("history_voice");
    await settings();
    await capture("history_settings_second");
    click(button("下载管理"));
    await wait(() => ready() && one(".download-row"), "third visit");
    await select("active");
    await capture("history_active");
    click(button("返回上一页"));
    await wait(
      () =>
        one(".settings-page") && !one(".page-enter-active,.page-leave-active"),
      "settings second back",
    );
    await back();
    await wait(
      () =>
        one("#download-tab-voice")?.getAttribute("aria-selected") === "true" &&
        one(".download-search input").value === "声音" &&
        Math.abs(one(".download-scroll").scrollTop - historyBTop) < 1 &&
        task(document.activeElement) === historyB,
      "second visit restores its own state",
    );
    await capture("history_voice_return");
    click(button("返回上一页"));
    await wait(
      () =>
        one(".settings-page") && !one(".page-enter-active,.page-leave-active"),
      "settings first back",
    );
    await back();
    await wait(
      () =>
        one("#download-tab-song")?.getAttribute("aria-selected") === "true" &&
        one(".download-search input").value === "单曲" &&
        Math.abs(one(".download-scroll").scrollTop - historyATop) < 1 &&
        task(
          document.activeElement?.closest("[data-download-task]") ??
            document.activeElement,
        ) === historyA &&
        document.activeElement?.dataset.downloadAction === "manage",
      "first visit restores its own state",
    );
    assert(
      !one(".download-continuation") &&
        one(".download-table-header").getAttribute("style") === historyAColumns,
      "first visit prefix and column widths",
    );
    await capture("history_song_return");
    await settings();
    click(button("下载管理"));
    await wait(() => ready() && one(".download-row"), "temporary visit");
    await select("voice");
    click(button("返回上一页"));
    await wait(
      () =>
        one(".settings-page") && !one(".page-enter-active,.page-leave-active"),
      "settings before failed return",
    );
    const nativeRead = client.downloads;
    let failReturn = true;
    client.downloads = async (e, q) => {
      if (failReturn && q.category === "song")
        throw { code: "state", message: "合成旧访问读取失败", retryable: true };
      return nativeRead(e, q);
    };
    await back();
    await wait(
      () => one(".error-state")?.textContent.includes("合成旧访问读取失败"),
      "failed earlier visit read",
    );
    assert(
      one("#download-tab-song").getAttribute("aria-selected") === "true" &&
        !one(".download-row"),
      "failed return cannot display another category records",
    );
    await capture("history_read_failed");
    failReturn = false;
    const retry = button("刷新状态");
    retry.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    click(retry);
    await wait(
      () =>
        ready() &&
        !one(".error-state") &&
        Math.abs(one(".download-scroll")?.scrollTop - historyATop) < 1 &&
        task(
          document.activeElement?.closest("[data-download-task]") ??
            document.activeElement,
        ) === historyA &&
        document.activeElement?.dataset.downloadAction === "manage",
      "retry resumes original anchor",
    );
    assert(
      !one(".download-continuation"),
      "retry reads original loaded prefix",
    );
    await capture("history_read_retried");
    client.downloads = nativeRead;
    // Leave before the search debounce can fire. The old timer must never filter the new visit.
    const pendingSearch = one(".download-search input");
    pendingSearch.value = "单曲 119";
    pendingSearch.dispatchEvent(new Event("input", { bubbles: true }));
    await settings();
    click(button("下载管理"));
    await wait(
      () => ready() && one(".download-row"),
      "pending search new visit",
    );
    await select("voice");
    click(button("返回上一页"));
    await wait(
      () =>
        one(".settings-page") && !one(".page-enter-active,.page-leave-active"),
      "pending search settings back",
    );
    await back();
    await wait(
      () =>
        one("#download-tab-song")?.getAttribute("aria-selected") === "true" &&
        one(".download-search input").value === "单曲 119" &&
        all("[data-download-index]").length === 1 &&
        historyRow(0)?.textContent.includes("119"),
      "pending search belongs only to source visit",
    );
    assert(one(".download-scroll").scrollTop === 0, "new query starts at top");
    await capture("history_pending_search");
    client.downloads = trackDownloads;

    await emit("complete", {
      ok: true,
      downloadNavigationUi: true,
      stageCount: count,
      realMusicRequests: 0,
      realAccountWrites: 0,
      syntheticNativeRecords: 146,
    });
  } catch (error) {
    const failedAt = step;
    await capture("failure_state").catch(() => {});
    await emit("failed", {
      step: failedAt,
      message: String(error),
      stack: error.stack,
      active: document.activeElement?.outerHTML.slice(0, 600),
      viewport:
        one(".downloads-page")?.__vueParentComponent?.props?.model?.viewport
          ?.value,
      body: document.body.innerText.slice(-1500),
    });
  }
})();
