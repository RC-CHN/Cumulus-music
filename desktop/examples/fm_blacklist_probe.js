// Actual paged native blacklist, shared scroller and ID-based offscreen selection.
(async () => {
  const p = window.createFmProof();
  const {
    one,
    all,
    wait,
    sleep,
    invoke,
    emit,
    assert,
    button,
    click,
    fixture,
    prepare,
    theme,
    size,
    rect,
    hit,
  } = p;
  const list = () => one('.fm-virtual-list[aria-label="减少推荐的歌曲"]');
  const artists = () => one('.fm-virtual-list[aria-label="屏蔽的歌手"]');
  const selection = () => Number(one(".fm-trash").dataset.selectedCount);
  const loaded = () => Number(list()?.dataset.total);
  async function select(input) {
    const expected = selection() + (input.checked ? -1 : 1);
    click(input);
    await wait(() => selection() === expected, "selection committed");
  }
  const settled = async () => {
    await new Promise(requestAnimationFrame);
    await wait(
      () => one(".fm-trash")?.getAttribute("aria-busy") === "false",
      "list idle",
    );
  };
  async function row(index, target = list()) {
    const root = one(".main-content");
    const height = target === artists() ? 56 : 72;
    root.scrollTop +=
      target.getBoundingClientRect().top -
      root.getBoundingClientRect().top +
      index * height;
    await wait(
      () => target.querySelector(`[data-fm-index="${index}"]`),
      "row rendered " + index,
    );
    await sleep(60);
    return target.querySelector(`[data-fm-index="${index}"]`);
  }
  async function key(target, key) {
    target.focus({ preventScroll: true });
    target.dispatchEvent(
      new KeyboardEvent("keydown", {
        key,
        code: key,
        bubbles: true,
        cancelable: true,
      }),
    );
    await sleep(100);
  }
  async function page() {
    const before = loaded();
    click(button("加载更多", one(".fm-trash")));
    await settled();
    await wait(() => loaded() > before, "page appended");
  }
  async function capture(label) {
    const lists = all(".fm-virtual-list").map((n) => ({
      label: n.getAttribute("aria-label"),
      total: Number(n.dataset.total),
      rendered: n.querySelectorAll("[data-fm-index]").length,
      indices: [...n.querySelectorAll("[data-fm-index]")].map((r) =>
        Number(r.dataset.fmIndex),
      ),
    }));
    for (const n of lists) assert(n.rendered <= 24, "bounded " + n.label);
    for (const n of all(".fm-trash-row")) {
      const r = rect(n);
      assert(
        r.width > 0 && r.right <= innerWidth && r.left >= 0,
        "row horizontally contained",
      );
      const box = n.querySelector("input");
      if (
        box &&
        r.top >= one(".main-content").getBoundingClientRect().top &&
        r.bottom <= one(".playerbar").getBoundingClientRect().top
      )
        assert(hit(box), "visible checkbox hit");
    }
    await p.capture(label, {
      fmBlacklistUi: true,
      lists,
      selectionCount: selection(),
      loaded: loaded(),
      listError: one(".fm-list-error")?.textContent ?? null,
      mainScrollTop: one(".main-content").scrollTop,
      activeRow:
        document.activeElement.closest("[data-fm-index]")?.dataset.fmIndex ??
        null,
    });
  }
  try {
    await wait(
      () =>
        one(".account-button") &&
        !one(".account-button").disabled &&
        !one(".welcome"),
      "account",
    );
    p.setEpoch((await invoke("account_snapshot")).epoch);
    await theme("light");
    click(button("私人漫游"));
    await wait(() => button("开始漫游"), "FM");
    click(button("开始漫游"));
    await prepare();
    await fixture("seed-long-list");
    await wait(
      () => one(".fm-page") && !one(".fm-page").dataset.fmStateError,
      "FM state",
    );
    click(button("减少推荐列表"));
    await settled();
    await wait(
      () => artists()?.dataset.total === "120" && loaded() === 50,
      "seeded lists",
    );
    await row(0, artists());
    await capture("artist_start_light");
    await key(artists().querySelector("button"), "End");
    await wait(
      () =>
        document.activeElement.closest("[data-fm-index]")?.dataset.fmIndex ===
        "119",
      "last artist reachable",
    );
    await capture("artist_end_light");
    click(document.activeElement);
    await settled();
    await wait(
      () => artists()?.dataset.total === "119",
      "one artist unblocked",
    );
    await theme("dark");
    await capture("artist_removed_dark");
    await row(0);
    await capture("songs_start_dark");
    await select(list().querySelector('[data-fm-index="0"] input'));
    await key(list().querySelector('[data-fm-index="0"] input'), "End");
    await wait(
      () =>
        document.activeElement.closest("[data-fm-index]")?.dataset.fmIndex ===
        "49",
      "last loaded song",
    );
    await select(document.activeElement);
    assert(selection() === 2, "two identities selected");
    await capture("songs_selection_dark");
    await fixture("trash-page-error");
    click(button("加载更多", one(".fm-trash")));
    await settled();
    await wait(() => one(".fm-list-error"), "page error visible");
    assert(
      loaded() === 50 && selection() === 2,
      "page failure retains rows and selection",
    );
    await capture("more_failed_dark");
    click(button("重试加载更多", one(".fm-list-error")));
    await settled();
    await wait(
      () => loaded() === 100 && !one(".fm-list-error"),
      "same page retried",
    );
    await capture("more_retried_dark");
    while (button("加载更多", one(".fm-trash"))) await page();
    assert(loaded() === 1001 && selection() === 2, "all pages loaded once");
    await capture("all_loaded_dark");
    await row(1000);
    const last = list().querySelector('[data-fm-index="1000"] input');
    await select(last);
    assert(selection() === 3, "far identity selected");
    await capture("far_end_dark");
    await key(last, "Home");
    await wait(
      () =>
        document.activeElement.closest("[data-fm-index]")?.dataset.fmIndex ===
        "0",
      "home selection",
    );
    assert(
      document.activeElement.checked && selection() === 3,
      "recycled first selection restored",
    );
    await theme("light");
    await capture("reverse_selected_light");
    await row(900);
    click(list().querySelector('[data-fm-index="900"] button'));
    await settled();
    await wait(() => loaded() === 50, "single row refresh");
    assert(selection() === 3, "single restore preserves other selected IDs");
    await wait(
      () =>
        document.activeElement === one(".fm-trash-toolbar") &&
        hit(one(".fm-trash-toolbar")),
      "collapsed list restores visible focus",
    );
    await row(0);
    await capture("row_restored_light");
    click(button("返回私人漫游"));
    await fixture("trash-page-error");
    click(button("减少推荐列表"));
    await settled();
    await wait(() => one(".fm-list-error"), "refresh error");
    assert(
      selection() === 3 &&
        loaded() === 50 &&
        one(".fm-trash-toolbar .primary").disabled,
      "stale list cannot restore",
    );
    one(".fm-trash-toolbar").scrollIntoView({ block: "center" });
    await capture("refresh_failed_light");
    click(button("重新读取列表"));
    await settled();
    await wait(() => !one(".fm-list-error"), "refresh recovered");
    await theme("dark");
    await row(0);
    await capture("refreshed_dark");
    await size(760, 540);
    await row(0);
    await capture("narrow_dark");
    await page();
    await page();
    for (let index = 0; selection() < 100 && index < loaded(); index++) {
      const n = await row(index),
        input = n.querySelector("input");
      if (!input.checked) await select(input);
    }
    assert(selection() === 100, "global selection capped at100");
    await capture("selection_limit_dark");
    await row(loaded() - 1);
    assert(
      list().querySelector(`[data-fm-index="${loaded() - 1}"] input`).disabled,
      "recycled unselected checkbox observes cap",
    );
    await capture("limit_recycled_dark");
    click(one(".fm-trash-toolbar .primary"));
    await settled();
    await wait(() => selection() === 0 && loaded() === 50, "batch finished");
    await row(0);
    await capture("batch_restored_dark");
    await emit("complete", {
      ok: true,
      fmBlacklistUi: true,
      stageCount: p.count,
      realMusicRequests: 0,
      realAccountWrites: 0,
    });
  } catch (error) {
    await emit("failed", {
      ok: false,
      last: p.last,
      count: p.count,
      error: String(error),
      active: document.activeElement?.outerHTML.slice(0, 350),
      body: document.body.textContent.slice(-2000),
    });
  }
})();
