// Real resource menus and native file operations, over an isolated synthetic account.
(async () => {
  const one = (s) => document.querySelector(s),
    all = (s) => [...document.querySelectorAll(s)],
    sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const assert = (v, m) => {
    if (!v) throw Error(m);
  };
  const wait = async (fn, m, attempts = 260) => {
    for (let i = 0; i < attempts; i++) {
      await sleep(50);
      if (await fn()) return;
    }
    throw Error(m);
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
  const click = (n) => {
    assert(n && !n.disabled, "enabled " + n?.outerHTML);
    n.focus({ preventScroll: true });
    n.click();
  };
  const key = (el, k, extra = {}) =>
    el.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: k,
        bubbles: true,
        cancelable: true,
        ...extra,
      }),
    );
  const input = (el, text) => {
    el.value = text;
    el.dispatchEvent(new Event("input", { bubbles: true }));
  };
  const fixture = async (mode, extra = {}) => {
    window.__downloadPageAck = null;
    await emit("download-page-fixture", { mode, ...extra });
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
      '#download-content[aria-busy="true"],.download-section-enter-active,.download-section-leave-active,.page-enter-active,.page-leave-active',
    );
  const row = (i = 0) => one('[data-download-index="' + i + '"]');
  const menu = () => one('.action-menu[aria-label="下载资源操作"]');
  const select = async (id) => {
    click(one("#download-tab-" + id));
    await wait(() => ready() && row(), "category " + id);
  };
  const open = async (i = 0, mode = "more") => {
    const r = row(i);
    assert(r, "source row");
    const more = r.querySelector('[aria-label^="管理下载"]');
    if (mode === "keyboard") {
      more.focus({ preventScroll: true });
      key(more, "F10", { shiftKey: true });
    } else if (mode === "pointer") {
      r.focus({ preventScroll: true });
      r.dispatchEvent(
        new MouseEvent("contextmenu", {
          button: 2,
          bubbles: true,
          cancelable: true,
          clientX: innerWidth - 10,
          clientY: innerHeight - 95,
        }),
      );
    } else click(more);
    await wait(
      () =>
        menu()?.style.visibility === "visible" &&
        menu().contains(document.activeElement),
      "menu ready",
    );
  };
  const action = async (label) => {
    const choice = button(label, menu());
    assert(choice, "menu choice " + label);
    await wait(() => !choice.disabled, "menu choice enabled " + label);
    choice.scrollIntoView({ block: "nearest" });
    await sleep(100);
    const r = choice.getBoundingClientRect();
    assert(
      choice.contains(
        document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2),
      ),
      "menu choice can be hit " + label,
    );
    click(choice);
    await wait(() => !menu(), "menu selects");
  };
  const close = async () => {
    click(button("关闭下载对话框"));
    await wait(() => !one(".download-dialog"), "dialog closed");
  };
  let count = 0,
    step = "startup";
  async function capture(label, extra = {}) {
    step = label;
    await sleep(250);
    assert(document.documentElement.scrollWidth <= innerWidth, "document fits");
    assert(all("[data-download-index]").length <= 32, "bounded rows");
    for (const el of [menu(), one("dialog")].filter(Boolean)) {
      const r = el.getBoundingClientRect();
      assert(
        r.left >= 0 &&
          r.right <= innerWidth + 1 &&
          r.top >= 0 &&
          r.bottom <= innerHeight + 1,
        "overlay fits",
      );
    }
    await emit("ux_" + String(++count).padStart(2, "0") + "_" + label, {
      width: innerWidth,
      height: innerHeight,
      theme: document.documentElement.dataset.theme,
      focused: document.activeElement?.outerHTML.slice(0, 240),
      ...extra,
    });
    await sleep(400);
  }
  try {
    await wait(() => one(".track-row"), "initial");
    await fixture("seed");
    await fixture("menu_targets");
    click(button("下载管理"));
    await wait(() => ready() && row(), "download list");
    const provides = one("#app").__vue_app__._context.provides,
      client =
        provides[
          Reflect.ownKeys(provides).find(
            (k) => typeof k === "symbol" && k.description === "core-client",
          )
        ],
      epoch = (await client.accountSnapshot()).epoch;
    for (const [w, h, theme] of [
      [1120, 760, "浅色"],
      [1120, 760, "深色"],
      [760, 540, "浅色"],
      [760, 540, "深色"],
    ]) {
      await emit("resize", { width: w, height: h });
      await wait(() => innerWidth === w && innerHeight === h, "resize");
      await emit("focus-sample");
      await wait(
        () => document.hasFocus(),
        "native WebView focused after resize",
      );
      click(button("外观主题"));
      await wait(() => one(".action-menu"), "themes");
      click(button(theme, one(".action-menu")));
      await wait(() => !one(".action-menu"), "theme");
      await select("song");
      await open(0, "pointer");
      assert(
        [...menu().querySelectorAll("button")]
          .slice(0, 8)
          .map((n) => n.textContent.trim())
          .join("|") ===
          "播放|下一首播放|查看评论|收藏|分享|复制链接|打开文件所在目录|删除下载",
        "recovered resource order",
      );
      await capture(
        "song_menu_" + w + "_" + (theme === "深色" ? "dark" : "light"),
      );
      key(menu(), "Escape");
      await wait(
        () => !menu() && document.activeElement === row(),
        "context returns exact row",
      );
      await open(0, "keyboard");
      await action("重命名…");
      await wait(
        () => one('[aria-label="新文件名"]') === document.activeElement,
        "direct rename focuses input",
      );
      await capture(
        "rename_" + w + "_" + (theme === "深色" ? "dark" : "light"),
      );
      input(one('[aria-label="新文件名"]'), " ");
      await wait(
        () => one(".download-dialog button.primary").disabled,
        "blank filename cannot submit",
      );
      await close();
      // A headless resize/modal close can leave the native toplevel inactive.
      // :focus-within actions cannot be focused until our own WebView is active.
      await emit("focus-sample");
      await wait(
        () => document.hasFocus(),
        "native WebView focused after modal",
      );
      await wait(
        () =>
          document.activeElement ===
          row().querySelector('[aria-label^="管理下载"]'),
        "cancel returns more control",
      );
      await select("active");
      await open(0, "keyboard");
      assert(button("暂停下载", menu()), "running task pauses");
      await action("删除下载");
      await wait(() => button("确认删除任务"), "single cancel confirm");
      await capture(
        "cancel_" + w + "_" + (theme === "深色" ? "dark" : "light"),
      );
      await close();
      assert(
        all("[data-download-index]").length === 5,
        "cancel dismissal preserves inventory",
      );
    }
    await emit("resize", { width: 1120, height: 760 });
    await wait(() => innerWidth === 1120, "wide");
    await select("voice");
    await open();
    assert(
      !button("收藏", menu()) && !button("复制链接", menu()),
      "voice is not a song",
    );
    await capture("voice_menu");
    const originalComments = client.commentsPage;
    let requested;
    client.commentsPage = async (e, resource, ...args) => {
      requested = resource;
      return originalComments(e, resource, ...args);
    };
    await action("查看评论");
    await wait(() => requested, "voice comment route");
    assert(
      requested.kind === "voice" && requested.id === "121",
      "voice identifier and resource type",
    );
    await capture("voice_comments");
    client.commentsPage = originalComments;
    click(button("返回上一页"));
    await wait(() => ready() && row(), "comments return");
    await select("song");
    const originalAction = client.playerAction;
    let latest;
    client.playerAction = async (e, command) => {
      const result = await originalAction(e, command);
      latest = { command, result };
      return result;
    };
    await open();
    await action("播放");
    await fixture("verify_queue", {
      ids: Array.from({ length: 120 }, (_, i) => i + 1),
      current: 1,
    });
    await capture("plays_full_download_selection");
    click(button("暂停", one(".playerbar")));
    await wait(
      () =>
        latest?.command.kind === "pause" && latest.result.state === "paused",
      "paused",
    );
    const before = latest.result;
    await open(1);
    await action("下一首播放");
    await wait(
      () => latest.command.kind === "queue_downloads",
      "offline insertion",
    );
    assert(
      latest.result.playId === before.playId &&
        latest.result.state === "paused" &&
        latest.result.entryId === before.entryId,
      "queue insertion preserves transport",
    );
    const q = await client.playerQueue(epoch);
    assert(q.tracks[1].track.id === "2", "offline queue identity");
    await fixture("verify_menu_queue");
    await capture("next_preserves_paused_transport");
    client.playerAction = originalAction;
    click(button("停止播放", one(".playerbar")));
    await open();
    await action("收藏");
    await wait(() => one(".playlist-collect-dialog"), "collection dialog");
    await capture("collect_handoff");
    click(button("关闭歌单操作", one("dialog")));
    await wait(() => !one("dialog"), "collect dismissed");
    await open();
    await action("分享");
    await wait(() => one(".track-share-dialog"), "share dialog");
    await capture("share_handoff");
    click(button("关闭分享"));
    await wait(() => !one("dialog"), "share dismissed");
    const originalCopy = client.copyText;
    let copies = 0;
    client.copyText = async (text) => {
      assert(
        text === "https://music.163.com/song?id=1",
        "public link for selected song",
      );
      copies++;
      if (copies === 1) throw Error("synthetic clipboard busy");
    };
    await open();
    await action("复制链接");
    await wait(() => one(".track-action-feedback.failed"), "copy failure");
    await capture("copy_failed");
    click(button("重试", one(".track-action-feedback")));
    await wait(
      () => one(".track-action-feedback")?.textContent.includes("链接已复制"),
      "copy retry",
    );
    assert(copies === 2, "one explicit retry");
    await capture("copy_retried");
    click(button("关闭", one(".track-action-feedback")));
    client.copyText = originalCopy;
    const state = await client.downloads(epoch, {
        tab: "completed",
        category: "song",
        keyword: "",
        sort: "completed",
        descending: true,
        offset: 0,
        limit: 100,
      }),
      directory = state.directory.root,
      id = state.tasks[0].id;
    await open();
    await action("重命名…");
    await wait(() => one('[aria-label="新文件名"]'), "rename field");
    input(one('[aria-label="新文件名"]'), "ui-conflict");
    click(one(".download-dialog button.primary"));
    await wait(
      () =>
        one(".download-dialog .error-state") &&
        !one(".download-dialog button.primary").disabled,
      "real rename collision",
    );
    await capture("rename_conflict");
    input(one('[aria-label="新文件名"]'), "menu-renamed");
    click(one(".download-dialog button.primary"));
    await wait(() => !one("dialog"), "rename submitted");
    const current = async () =>
      await client.downloads(epoch, {
        tab: "completed",
        category: "song",
        keyword: "",
        sort: "completed",
        descending: true,
        offset: 0,
        limit: 100,
      });
    await wait(async () => {
      const v = await current(),
        t = v.tasks.find((t) => t.id === id);
      return t?.path.endsWith("menu-renamed.wav") && !t.fileOperation;
    }, "native rename finished");
    await capture("renamed");
    await open();
    await action("移动到…");
    await wait(
      () => one('[aria-label="移动目标目录"]') === document.activeElement,
      "move input focus",
    );
    // Windows verbatim paths do not normalize '/' separators. Keep the native
    // root spelling when entering a child directory, as a folder picker does.
    const separator = directory.includes("\\") ? "\\" : "/";
    input(
      one('[aria-label="移动目标目录"]'),
      directory + separator + "menu-moved",
    );
    await wait(
      () => !one(".download-dialog button.primary").disabled,
      "valid move directory",
    );
    click(one(".download-dialog button.primary"));
    await wait(() => !one("dialog"), "move submitted");
    await wait(async () => {
      const t = (await current()).tasks.find((t) => t.id === id);
      return (
        t?.path
          .replaceAll("\\", "/")
          .endsWith("/menu-moved/menu-renamed.wav") && !t.fileOperation
      );
    }, "native move finished");
    await capture("moved");
    await open(1);
    await action("仅移出下载列表…");
    await wait(() => one(".download-dialog form"), "forget confirm");
    await capture("forget_confirm");
    click(one(".download-dialog button.primary"));
    await wait(
      async () => !one("dialog") && (await current()).total === 119,
      "forget done",
    );
    await capture("forgotten");
    await open(1);
    await action("删除下载");
    await wait(() => button("确认删除本地文件"), "delete confirm");
    await capture("delete_confirm");
    const submitDelete = client.downloadCommand;
    let deleteOperation;
    client.downloadCommand = async (e, command) => {
      const result = await submitDelete(e, command);
      if (
        command.kind === "file" &&
        command.action === "delete" &&
        command.task_id === state.tasks[2].id
      )
        deleteOperation = result.operationId;
      return result;
    };
    click(button("确认删除本地文件"));
    await wait(async () => {
      const value = await current();
      const job = value.jobs.find((item) => item.id === deleteOperation);
      // Hidden inventory rows mean deletion started, not filesystem cleanup.
      // Completed operations disappear from the public active-operation list.
      // Keep exact journal and native file assertions in verify_menu_files.
      return (
        !one("dialog") &&
        value.total === 118 &&
        !!job &&
        !job.running &&
        !job.error &&
        !value.operations.some((item) => item.id === deleteOperation)
      );
    }, "delete done");
    client.downloadCommand = submitDelete;
    await fixture("verify_menu_files");
    await capture("deleted");
    await open();
    await action("删除下载");
    await wait(() => one(".download-dialog form"), "stale deletion");
    await fixture("read_error");
    await wait(
      () =>
        one(".download-dialog")?.textContent.includes("暂时无法确认下载状态") &&
        button("确认删除本地文件").disabled,
      "read failure blocks existing confirmation",
    );
    await capture("delete_read_failed");
    await close();
    await fixture("repair_read");
    click(button("刷新状态"));
    await wait(
      () => ready() && !one(".downloads-page .error-state"),
      "read repaired",
    );
    await open();
    await action("删除下载");
    await wait(() => one(".download-dialog form"), "stale deletion");

    await fixture("remove_first");
    await wait(
      () =>
        one('.download-dialog [role="alert"]')?.textContent.includes(
          "记录已改变",
        ),
      "inventory invalidates intent",
    );
    assert(button("确认删除本地文件").disabled, "stale deletion disabled");
    await capture("stale_confirm");
    await close();
    await open(0, "keyboard");
    const scroll = one(".download-scroll");
    scroll.scrollTop = scroll.scrollHeight;
    scroll.dispatchEvent(new Event("scroll"));
    await wait(
      () => !menu() && document.activeElement === scroll,
      "outside scroll dismisses and returns to list",
    );
    await capture("menu_scroll");
    await fixture("read_error");
    await wait(() => one(".downloads-page .error-state"), "read failure");
    scroll.scrollTop = 0;
    scroll.dispatchEvent(new Event("scroll"));
    await wait(() => row(), "first after error");
    await open();
    assert(
      [...menu().querySelectorAll("button")].every((n) => n.disabled),
      "read failure disables stale actions",
    );
    await capture("read_locked");
    key(menu(), "Escape");
    await fixture("repair_read");
    await select("active");
    const paused = all("[data-download-index]").find((n) =>
      n.textContent.includes("合成任务 143"),
    );
    assert(paused, "paused task retained");
    await open(Number(paused.dataset.downloadIndex));
    await action("删除下载");
    await wait(() => button("确认删除任务"), "cancel single task");
    const originalCommand = client.downloadCommand;
    let releaseCancel,
      cancelCount = 0;
    client.downloadCommand = async (e, command) => {
      if (command.kind === "control" && command.action === "cancel") {
        cancelCount++;
        await new Promise((resolve) => (releaseCancel = resolve));
      }
      return originalCommand(e, command);
    };
    click(button("确认删除任务"));
    await wait(
      () =>
        releaseCancel &&
        button("确认删除任务").disabled &&
        button("关闭下载对话框").disabled,
      "cancel pending",
    );
    button("确认删除任务").click();
    await capture("cancel_pending");
    releaseCancel();
    await wait(
      () =>
        !one("dialog") &&
        !all("[data-download-index]").some((n) =>
          n.textContent.includes("合成任务 143"),
        ),
      "selected cancellation done",
    );
    assert(cancelCount === 1, "cancel submitted exactly once");
    await fixture("verify_single_cancel");
    await capture("cancelled_single");
    client.downloadCommand = originalCommand;
    await emit("complete", {
      ok: true,
      downloadMenuUi: true,
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
      active: document.activeElement?.outerHTML.slice(0, 500),
      moreDiagnostic: (() => {
        const n = row()?.querySelector('[aria-label^="管理下载"]'),
          r = n?.closest(".download-row"),
          a = n?.closest(".download-row-actions");
        if (!n) return null;
        const before = document.activeElement;
        const value = {
          html: n.outerHTML,
          rects: n.getClientRects().length,
          disabled: n.disabled,
          inert: !!n.closest("[inert]"),
          windowFocused: document.hasFocus(),
          rowFocused: r === before,
          rowWithin: r?.matches(":focus-within"),
          actionsDisplay: a && getComputedStyle(a).display,
          visibility: getComputedStyle(n).visibility,
        };
        n.focus({ preventScroll: true });
        return { ...value, manualFocus: document.activeElement === n };
      })(),
      body: document.body.innerText.slice(-1600),
    });
  }
})();
