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
