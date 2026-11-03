// Production WebView, IPC, core, filesystem and queue; isolated synthetic service/media.
(async () => {
  const invoke = (name, args = {}) =>
    window.__TAURI_INTERNALS__.invoke(name, args);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (value, label) => {
    if (!value) throw Error(label);
  };
  async function wait(test, label) {
    await sleep(0);
    for (let i = 0; i < 600; i++) {
      if (await test()) return;
      await sleep(40);
    }
    throw Error(label);
  }
  const all = (selector, root = document) => [
    ...root.querySelectorAll(selector),
  ];
  const shown = (selector) =>
    all(selector).find(
      (node) => !node.closest(".page-leave-active,.modal-leave-active"),
    );
  const local = () => shown(".local-page"),
    downloads = () => shown(".downloads-page"),
    cloud = () => shown(".cloud-page");
  const dialog = () => shown("dialog[open]"),
    queue = () => shown(".queue-panel");
  const localTrackMenu = () => shown('[role="menu"][aria-label="本地歌曲操作"]');
  const localField = () =>
    local()?.querySelector('[aria-label="搜索本地音乐"]');
  const localRows = () => all(".local-tracks tbody tr", local() ?? document);
  const downloadRows = () =>
    all("[data-download-task]", downloads() ?? document);
  const readyLocal = () =>
    local()?.dataset.localReady === "true" &&
    local()?.dataset.localLoading === "false";
  const readyDownload = () =>
    downloads()
      ?.querySelector('[role="tabpanel"]')
      ?.getAttribute("aria-busy") === "false";
  const emit = (value) =>
    invoke("plugin:event|emit", { event: "mini-proof", payload: value });
  window.__miniAck = {};
  let serial = 0,
    count = 0,
    last = "startup",
    client,
    originalLocal,
    originalCommand,
    originalSelect;
  let readMode = "normal",
    commandFails = false,
    hideCommands = 0,
    held = [],
    localCalls = [],
    selections = [],
    resolvedSelections = 0,
    completedReads = 0;
  const localQuery = {
    keyword: "",
    sort: "title",
    descending: true,
    group: "list",
    groupKey: null,
    includeMissing: true,
    offset: 0,
    limit: 200,
    revision: null,
  };
  const downloadQuery = {
    tab: "completed",
    category: "song",
    keyword: "",
    sort: "completed",
    descending: true,
    offset: 0,
    limit: 100,
  };
  async function control(kind, data = {}) {
    const id = ++serial;
    await emit({ kind, id, ...data });
    await wait(() => id in window.__miniAck, kind);
    const value = window.__miniAck[id];
    delete window.__miniAck[id];
    assert(!value.error, value.error);
    return value.value ?? value;
  }
  function button(label, root = document) {
    return all("button", root).find(
      (node) =>
        node.textContent.trim() === label ||
        node.getAttribute("aria-label") === label,
    );
  }
  function click(label, root = document) {
    const node = button(label, root);
    assert(node && !node.disabled, "enabled " + label);
    node.scrollIntoView({ block: "nearest" });
    node.focus({ preventScroll: true });
    node.click();
    return node;
  }
  async function settle() {
    await wait(
      () =>
        !document.querySelector(
          ".page-enter-active,.page-leave-active,.modal-enter-active,.modal-leave-active,.queue-panel-enter-active,.queue-panel-leave-active",
        ),
      "transitions",
    );
    await new Promise((resolve) =>
      requestAnimationFrame(() => requestAnimationFrame(resolve)),
    );
    // Theme variables also animate button colors without transition classes.
    // Preserve those animations and capture their settled compositor frame.
    await wait(
      () =>
        !document.getAnimations().some((animation) => {
          const end = animation.effect?.getComputedTiming().endTime;
          return (
            (animation.playState === "running" || animation.pending) &&
            typeof end === "number" &&
            Number.isFinite(end)
          );
        }),
      "finite interface animations",
    );
    await new Promise((resolve) =>
      requestAnimationFrame(() => requestAnimationFrame(resolve)),
    );
  }
  async function nav(label) {
    click(label, document.querySelector(".sidebar"));
    await settle();
  }
  async function theme(value) {
    localStorage.setItem("cumulus.appearance.v1", value);
    window.dispatchEvent(
      new StorageEvent("storage", {
        key: "cumulus.appearance.v1",
        newValue: value,
      }),
    );
    await wait(() => document.documentElement.dataset.theme === value, "theme");
  }
  async function resize(width, height) {
    await control("report-resize", { width, height, scale: devicePixelRatio });
    await wait(
      () => innerWidth === width && innerHeight === height,
      "window size",
    );
  }
  function input(node, value, submit = false) {
    assert(node, "input");
    node.focus();
    node.value = value;
    node.dispatchEvent(new Event("input", { bubbles: true }));
    if (submit)
      node.form.dispatchEvent(
        new Event("submit", { bubbles: true, cancelable: true }),
      );
  }
  async function localSearch(value) {
    input(localField(), value);
    await sleep(15); // Flush the recovered 10 ms debounce before checking rendered readiness.
    await wait(
      () => readyLocal() && localField().value === value,
      "local query " + value,
    );
    await sleep(25);
  }
  async function pause(epoch) {
    await invoke("player_action", {
      epoch,
      action: { kind: "pause", paused: true },
    });
    await wait(
      () => document.querySelector(".playerbar")?.dataset.state === "paused",
      "native paused",
    );
  }
  async function openQueue() {
    click("播放队列", document.querySelector(".playerbar"));
    await wait(queue, "queue open");
    await settle();
  }
  async function closeQueue() {
    click("关闭播放队列", queue());
    await wait(() => !document.querySelector(".queue-panel"), "queue closed");
    await settle();
  }
  function rect(node) {
    const r = node.getBoundingClientRect();
    return {
      left: r.left,
      top: r.top,
      right: r.right,
      bottom: r.bottom,
      width: r.width,
      height: r.height,
    };
  }
  function paintedBox(node) {
    const closed = node.closest("details:not([open])");
    if (closed && node.closest("summary")?.parentElement !== closed)
      return null;
    const box = rect(node);
    let left = Math.max(0, box.left),
      right = Math.min(innerWidth, box.right),
      top = Math.max(0, box.top),
      bottom = Math.min(innerHeight, box.bottom);
    for (let parent = node; parent; parent = parent.parentElement) {
      const style = getComputedStyle(parent),
        bounds = rect(parent);
      if (
        style.display === "none" ||
        style.visibility === "hidden" ||
        Number(style.opacity) === 0
      )
        return null;
      if (["auto", "scroll", "hidden", "clip"].includes(style.overflowX)) {
        left = Math.max(left, bounds.left);
        right = Math.min(right, bounds.right);
      }
      if (["auto", "scroll", "hidden", "clip"].includes(style.overflowY)) {
        top = Math.max(top, bounds.top);
        bottom = Math.min(bottom, bounds.bottom);
      }
    }
    return right - left >= 1 && bottom - top >= 1
      ? { left, right, top, bottom }
      : null;
  }
  function facts() {
    const l = local(),
      d = downloads(),
      q = queue(),
      modal = dialog(),
      active = document.activeElement;
    const buttons = all(
      "button",
      modal ?? localTrackMenu() ?? q ?? l ?? d ?? cloud() ?? document,
    ).filter((node) => node.getBoundingClientRect().width > 0);
    return {
      captureTitle: "Cumulus",
      windowKind: "main",
      width: innerWidth,
      height: innerHeight,
      screenX,
      screenY,
      ratio: devicePixelRatio,
      visible: document.visibilityState === "visible",
      focused: document.hasFocus(),
      theme: document.documentElement.dataset.theme,
      overflow:
        document.documentElement.scrollWidth > innerWidth ||
        !!(l && l.scrollWidth > l.clientWidth + 1),
      focusValid:
        active !== document.body &&
        !!active?.isConnected &&
        !!paintedBox(active),
      focusLabel:
        active?.getAttribute("aria-label") ??
        active?.textContent?.trim().slice(0, 80),
      localSearch: localField()?.value,
      localIds: l ? localRows().map((row) => row.dataset.localId) : [],
      localCount: l ? localRows().length : null,
      localPlayable: l ? all("tbody tr button:not(:disabled)", l).length : null,
      cueCount: l
        ? all(".local-badge", l).filter((node) => node.textContent === "CUE")
            .length
        : null,
      readError: l
        ?.querySelector("[data-local-read-error]")
        ?.textContent.trim(),
      commandError: l
        ?.querySelector("[data-local-command-error]")
        ?.textContent.trim(),
      localEmpty: l
        ?.querySelector(".local-tracks .empty-state")
        ?.textContent.trim(),
      loading: l?.dataset.localLoading,
      group: l
        ?.querySelector('.local-tabs button[aria-pressed="true"]')
        ?.textContent.trim(),
      downloadIds: d
        ? downloadRows().map((row) => row.dataset.downloadTask)
        : [],
      downloadSearch: d?.querySelector('[aria-label="搜索下载的歌曲"]')?.value,
      queueEntries: q
        ? all("[data-entry-id]", q).map((row) => row.dataset.entryId)
        : [],
      paintSamples: buttons
        .filter(
          (node) =>
            !node.disabled &&
            paintedBox(node) &&
            node.matches(
              ".secondary,.download-toolbar .action-menu-host > .menu-trigger",
            ),
        )
        .map((node) => {
          const box = rect(node);
          return {
            x: box.left + 5,
            y: box.top + box.height / 2,
            color: getComputedStyle(node).backgroundColor,
          };
        }),
      buttons: buttons.map((node) => {
        const visibleBox = paintedBox(node),
          box = rect(node);
        const top = visibleBox
          ? document.elementFromPoint(
              (visibleBox.left + visibleBox.right) / 2,
              (visibleBox.top + visibleBox.bottom) / 2,
            )
          : null;
        return {
          label: node.textContent.trim() || node.getAttribute("aria-label"),
          disabled: node.disabled,
          visible: !!visibleBox,
          rect: box,
          painted: visibleBox,
          hit: !visibleBox || top === node || node.contains(top),
          hitElement: top
            ? top.tagName + ":" + String(top.className ?? "")
            : null,
        };
      }),
      dialogs: modal
        ? [
            {
              open: true,
              rect: rect(modal),
              opacity: getComputedStyle(modal).opacity,
              background: getComputedStyle(modal).backgroundColor,
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
    window.__miniRecorded = null;
    await emit({
      kind: "snapshot",
      stage,
      localResourceUi: true,
      ...value,
      ...extra,
    });
    await wait(() => window.__miniRecorded === stage, "snapshot");
    assert(!value.overflow, "horizontal overflow " + label);
    assert(value.focusValid, "focus " + label);
    assert(
      value.buttons.every((node) => node.hit),
      "hit targets " + label,
    );
  }
  async function selectCloud(id, deleteLocal) {
    input(
      cloud().querySelector('[aria-label="搜索云盘歌曲"]'),
      "cloud-00",
      true,
    );
    await wait(
      () =>
        cloud()?.dataset.cloudReady === "true" &&
        cloud()?.dataset.cloudLoading === "false" &&
        !!cloud().querySelector('[data-cloud-id="' + id + '"]'),
      "visible filtered cloud target",
    );
    const node = cloud().querySelector('[data-cloud-id="' + id + '"] input');
    assert(node, "cloud target " + id);
    node.scrollIntoView({ block: "nearest" });
    node.focus({ preventScroll: true });
    if (!node.checked) node.click();
    await sleep(0);
    click("删除所选", cloud());
    await wait(
      () => dialog()?.dataset.cloudDeleteReady === "true",
      "delete preview",
    );
    const option = dialog().querySelector("[data-cloud-delete-local]");
    assert(option && !option.checked, "default local retention");
    if (deleteLocal) option.click();
    return option;
  }
  try {
    await wait(
      () => document.querySelector(".track-row .track-play:not(:disabled)"),
      "startup",
    );
    const provides =
      document.querySelector("#app").__vue_app__._context.provides;
    client =
      provides[
        Reflect.ownKeys(provides).find(
          (key) => typeof key === "symbol" && key.description === "core-client",
        )
      ];
    originalLocal = client.localPage.bind(client);
    originalCommand = client.localCommand.bind(client);
    originalSelect = client.playSelection.bind(client);
    client.localPage = async (query) => {
      localCalls.push({ ...query });
      if (readMode === "reject")
        throw { code: "io", message: "合成本地目录读取失败" };
      const value = await originalLocal(query);
      if (readMode === "hold" && query.keyword === "005")
        await new Promise((resolve) => held.push(resolve));
      completedReads++;
      return value;
    };
    client.localCommand = async (command) => {
      if (command.kind === "hide_track") hideCommands++;
      if (commandFails && command.kind === "rescan")
        throw { code: "io", message: "合成扫描失败" };
      return originalCommand(command);
    };
    client.playSelection = async (epoch, selection) => {
      selections.push(selection);
      const result = await originalSelect(epoch, selection);
      resolvedSelections++;
      return result;
    };
    document
      .querySelector(".track-row")
      .dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    await wait(
      () => document.querySelector(".playerbar")?.dataset.state === "playing",
      "startup audio",
    );
    const epoch = (await invoke("account_snapshot")).epoch;
    await pause(epoch);
    await resize(1120, 760);
    for (const songId of ["1004", "1005", "1006"]) {
      await control("cloud-menu-seed", { songId });
      const seeded = await control("cloud-removal-file", {
        songId,
        action: "observe",
      });
      assert(seeded.exists && seeded.downloadPresent, "seeded file " + songId);
    }
    await control("cloud-removal-file", { songId: "1004", action: "cue" });
    await control("cloud-removal-file", { songId: "1004", action: "index" });
    const initial = await originalLocal(localQuery);
    assert(
      initial.total === 3 &&
        initial.tracks.filter((row) => row.cue).length === 1,
      "three actual indexed resources",
    );
    const cue = initial.tracks.find((row) => row.cue).track.id,
      removedLocal = initial.tracks.find((row) => row.fileName.includes("006"))
        .track.id;
    for (const path of new Set(initial.tracks.map((row) => row.folder)))
      await originalCommand({ kind: "watch", path });
    await wait(
      async () =>
        (await originalLocal(localQuery)).roots.every(
          (root) => root.status === "ready",
        ),
      "native watched directories ready",
    );
    const beforeDownloads = await client.downloads(epoch, downloadQuery);
    assert(beforeDownloads.tasks.length === 3, "three completed downloads");
    const removedTask = beforeDownloads.tasks.find(
        (task) => task.track.id === "1006",
      ).id,
      keptTask = beforeDownloads.tasks.find(
        (task) => task.track.id === "1005",
      ).id;
    await nav("本地音乐");
    await wait(
      () => readyLocal() && localRows().length === 3,
      "local baseline",
    );
    await theme("light");
    await capture("library_light");
    const indexedCue = (await originalLocal(localQuery)).tracks.find((row) => row.cue);
    const cueTrigger = localRows()
      .find((row) => row.dataset.localId === indexedCue.track.id)
      .querySelector("[data-local-track-menu]");
    let directoryRequests = 0, directoryAccepted = false;
    const openDirectory = client.openLocalTrackDirectory.bind(client);
    client.openLocalTrackDirectory = async (id, revision) => {
      directoryRequests++;
      assert(id === indexedCue.track.id && revision === indexedCue.revision, "opaque current CUE identity");
      let rejected = false;
      try { await openDirectory(id, "0".repeat(64)); } catch (error) { rejected = error?.code === "snapshot_changed"; }
      assert(rejected, "stale revision rejected by native command");
      if (window.__localDirectoryLauncherProof) await openDirectory(id, revision);
      directoryAccepted = true;
    };
    cueTrigger.focus();
    cueTrigger.dispatchEvent(new KeyboardEvent("keydown", { key: "F10", shiftKey: true, bubbles: true, cancelable: true }));
    await wait(localTrackMenu, "local track keyboard menu");
    await wait(() => document.activeElement?.closest('[aria-label="本地歌曲操作"]'), "local track menu focus");
    await capture("local_directory_menu_light");
    click("打开文件所在目录", localTrackMenu());
    await wait(() => directoryAccepted, "local indexed directory dispatched");
    await wait(() => document.activeElement === cueTrigger, "local directory returns to row menu trigger");
    assert(directoryRequests === 1, "local directory accepted once");
    client.openLocalTrackDirectory = openDirectory;
    await capture("local_directory_light", { directoryRequests, directoryAccepted });
    const composingField = localField();
    composingField.focus();
    composingField.dispatchEvent(
      new CompositionEvent("compositionstart", {
        bubbles: true,
        data: "zhong",
      }),
    );
    input(composingField, "zhong");
    await sleep(25);
    assert(
      !localCalls.some((query) => query.keyword === "zhong"),
      "no partial IME native query",
    );
    await capture("composition_pending_light", { imePending: true });
    composingField.value = "合成";
    composingField.dispatchEvent(
      new CompositionEvent("compositionend", { bubbles: true, data: "合成" }),
    );
    await wait(
      () =>
        readyLocal() &&
        localField().value === "合成" &&
        localCalls.some((query) => query.keyword === "合成"),
      "completed IME query",
    );
    await theme("dark");
    await capture("composition_complete_dark", { imeCommitted: true });
    click("清空本地搜索", local());
    await wait(
      () => readyLocal() && localRows().length === 3,
      "clear IME search",
    );

    await localSearch("006");
    await wait(
      () =>
        localRows()
          .map((row) => row.dataset.localId)
          .join() === removedLocal,
      "live filter",
    );
    await theme("dark");
    await capture("search_live_dark");
    input(localField(), "005");
    click("音乐云盘", document.querySelector(".sidebar"));
    await settle();
    await nav("本地音乐");
    await wait(
      () =>
        readyLocal() &&
        localField().value === "005" &&
        localRows().length === 1,
      "flushed return query",
    );
    await theme("light");
    await capture("search_returned_light");
    await resize(900, 620);
    await localSearch("006");
    await wait(
      () =>
        localRows().length === 1 &&
        localRows()[0].dataset.localId === removedLocal,
      "old query ready",
    );
    const old = localRows()[0],
      beforePlay = selections.length;
    readMode = "hold";
    input(localField(), "005");
    await wait(
      () => held.length === 1 && local()?.dataset.localLoading === "true",
      "held local query",
    );
    assert(
      localRows().length === 0,
      "old rows hidden during a different query",
    );
    old.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    button("播放全部", local()).click();
    await sleep(20);
    assert(
      selections.length === beforePlay,
      "no stale row or play-all command",
    );
    await theme("dark");
    await capture("query_pending_dark", { heldActualNativeRead: true });
    readMode = "normal";
    await localSearch("004");
    await wait(
      () => localRows().length === 1 && localRows()[0].dataset.localId === cue,
      "newest native cue query",
    );
    held.splice(0).forEach((resolve) => resolve());
    await sleep(50);
    assert(localRows()[0].dataset.localId === cue, "late reply ignored");
    await theme("light");
    await capture("query_latest_light");
    readMode = "reject";
    input(localField(), "missing synthetic");
    await wait(
      () =>
        local()?.querySelector("[data-local-read-error]") &&
        localRows().length === 0,
      "new query read failure",
    );
    assert(
      !local().textContent.includes("让收藏的音乐回到播放列表"),
      "failure is not an empty library",
    );
    await theme("dark");
    await capture("query_failed_dark", { injectedReadFailure: true });
    readMode = "normal";
    click("重试读取", local());
    await wait(
      () => readyLocal() && local().textContent.includes("没有匹配的本地音乐"),
      "genuine empty result",
    );
    assert(
      document.activeElement === localField(),
      "retry restores visible search focus",
    );
    await theme("light");
    await capture("query_empty_light");
    click("清空本地搜索", local());
    await wait(
      () => readyLocal() && localRows().length === 3,
      "clear returns full library",
    );
    assert(document.activeElement === localField(), "clear keeps input focus");
    input(localField(), "queued obsolete search");
    click("歌手", local());
    await wait(
      () =>
        readyLocal() && localRows().length === 3 && localField().value === "",
      "group clears pending search",
    );
    await sleep(25);
    assert(localField().value === "", "old debounce cannot restore search");
    await theme("dark");
    await capture("group_cleared_dark");
    await resize(1120, 760);
    const separator = () => local().querySelector('[role="separator"][aria-label="调整本地分组宽度"]');
    const groupWidth = () => Math.round(local().querySelector(".local-groups").getBoundingClientRect().width);
    const press = (key) => separator().dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
    const pointer = (target, type, x) => target.dispatchEvent(new PointerEvent(type, {
      pointerId: 91, pointerType: "mouse", isPrimary: true, button: 0, clientX: x, bubbles: true, cancelable: true,
    }));
    await wait(() => separator() && groupWidth() === 155, "wide group layout starts at recovered width");
    separator().focus();
    press("End");
    const maxWidth = Number(separator().getAttribute("aria-valuemax"));
    await wait(() => groupWidth() === maxWidth, "keyboard grows group list to its bound");
    assert(local().querySelector(".local-tracks").getBoundingClientRect().width >= 459, "song table keeps its recovered minimum");
    press("Home");
    await wait(() => groupWidth() === 155, "keyboard restores minimum");
    const x = separator().getBoundingClientRect().left + 4;
    pointer(separator(), "pointerdown", x);
    pointer(document, "pointermove", x + 40);
    pointer(document, "pointerup", x + 40);
    await wait(() => groupWidth() === 195, "pointer commits group width");
    pointer(separator(), "pointerdown", x + 40);
    pointer(document, "pointermove", x + 72);
    await wait(() => groupWidth() === 227, "pointer previews group width");
    press("Escape");
    await wait(() => groupWidth() === 195, "escape cancels group width preview");
    await capture("group_resized_dark", { groupWidth: groupWidth(), groupWidthMaximum: maxWidth });
    await resize(760, 620);
    await wait(() => !separator() && groupWidth() === 120, "compact group view keeps room for the table");
    await wait(
      () => document.activeElement === local().querySelector('.local-groups button[aria-pressed="true"]'),
      "compact layout restores group focus",
    );
    await capture("group_compact_dark", { groupWidth: groupWidth() });
    await resize(1120, 760);
    await wait(() => separator() && groupWidth() === 195, "preferred width survives compact view");
    await resize(900, 620);

    click("歌曲", local());
    await wait(
      () =>
        readyLocal() &&
        localRows().length === 3 &&
        local()
          .querySelector('.local-tabs [aria-pressed="true"]')
          ?.textContent.trim() === "歌曲",
      "all local tracks",
    );
    const beforeLocalSelection = resolvedSelections;
    click("播放全部", local());
    await wait(
      () => resolvedSelections > beforeLocalSelection,
      "local selection committed",
    );
    await pause(epoch);
    const initialQueue = await client.playerQueue(epoch);
    assert(initialQueue.tracks.length === 3, "full native local queue");
    const removedEntry = initialQueue.tracks.find(
      (row) => row.track.id === removedLocal,
    ).entryId;
    const beforePlayer = await client.playerSnapshot(epoch);
    assert(beforePlayer.track.id === cue, "CUE is selected and retained");
    await openQueue();
    await wait(
      () => all("[data-entry-id]", queue()).length === 3,
      "queue before delete",
    );
    await theme("light");
    await capture("queue_before_light");
    await closeQueue();
    await nav("下载管理");
    await wait(
      () => readyDownload() && downloadRows().length === 3,
      "downloads baseline",
    );
    input(
      downloads().querySelector('[aria-label="搜索下载的歌曲"]'),
      "006",
      true,
    );
    await wait(
      () =>
        readyDownload() &&
        downloadRows().length === 1 &&
        downloadRows()[0].dataset.downloadTask === removedTask,
      "download filter",
    );
    await theme("dark");
    await capture("downloads_before_dark");
    await nav("音乐云盘");
    await wait(
      () =>
        cloud()?.dataset.cloudReady === "true" &&
        cloud()?.dataset.cloudLoading === "false",
      "cloud",
    );
    await selectCloud("1006", true);
    await theme("light");
    await capture("cloud_confirm_light");
    click("删除歌曲", dialog());
    await wait(() => !dialog(), "composed deletion complete");
    await wait(
      () => cloud()?.dataset.cloudLoading === "false",
      "cloud refreshed",
    );
    await theme("dark");
    await capture("cloud_removed_dark");
    const removed = await control("cloud-removal-file", {
      songId: "1006",
      action: "observe",
    });
    assert(
      !removed.exists && !removed.downloadPresent,
      "file and download record removed",
    );
    await nav("下载管理");
    await wait(
      () =>
        readyDownload() &&
        downloads().textContent.includes("没有找到匹配的下载"),
      "filtered deleted download removed",
    );
    assert(downloadRows().length === 0, "no cached deleted download row");
    assert(
      downloads().querySelector('[aria-label="搜索下载的歌曲"]').value ===
        "006",
      "download filter preserved",
    );
    const nowDownloads = await client.downloads(epoch, downloadQuery);
    assert(nowDownloads.completedCount === 2, "global completed count updated");
    await capture("downloads_removed_dark");
    await nav("本地音乐");
    await wait(
      () => readyLocal() && localRows().length === 2,
      "local deletion reflected",
    );
    assert(
      !localRows().some((row) => row.dataset.localId === removedLocal),
      "deleted local row gone",
    );
    assert(
      local().querySelectorAll(".local-badge").length === 1,
      "CUE remains",
    );
    await theme("light");
    await capture("library_removed_light");
    await openQueue();
    await wait(
      () => all("[data-entry-id]", queue()).length === 2,
      "native queue reconciled",
    );
    assert(
      !all("[data-entry-id]", queue()).some(
        (row) => row.dataset.entryId === removedEntry,
      ),
      "removed queue entry gone",
    );
    const afterPlayer = await client.playerSnapshot(epoch);
    assert(
      afterPlayer.track.id === cue &&
        afterPlayer.playId === beforePlayer.playId &&
        afterPlayer.state === "paused",
      "retained CUE playback identity and pause preserved",
    );
    await theme("dark");
    await capture("queue_reconciled_dark");
    await closeQueue();
    await nav("音乐云盘");
    await wait(
      () =>
        cloud()?.dataset.cloudReady === "true" &&
        cloud()?.dataset.cloudLoading === "false",
      "cloud for retained file",
    );
    await selectCloud("1005", false);
    await theme("light");
    await capture("cloud_keep_light");
    click("删除歌曲", dialog());
    await wait(() => !dialog(), "cloud-only deletion complete");
    const kept = await control("cloud-removal-file", {
      songId: "1005",
      action: "observe",
    });
    assert(
      kept.exists && kept.downloadPresent,
      "unchecked local source and download record kept",
    );
    await nav("下载管理");
    await wait(readyDownload, "download return");
    click("清空下载搜索", downloads());
    await wait(
      () => readyDownload() && downloadRows().length === 2,
      "retained downloads shown",
    );
    const task = downloads().querySelector(
      '[data-download-task="' + keptTask + '"]',
    );
    assert(task, "retained cloud download still visible");
    const authBefore = (await control("cloud-state")).downloadAuths.length;
    task.scrollIntoView({ block: "nearest" });
    task.focus({ preventScroll: true });
    task.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    await wait(
      () => document.querySelector(".playerbar")?.dataset.state === "playing",
      "retained file playback",
    );
    await pause(epoch);
    const offline = await client.playerSnapshot(epoch);
    assert(
      offline.track.id === "1005" && offline.resourceKind === "downloaded",
      "retained cloud resource uses offline player",
    );
    assert(
      (await control("cloud-state")).downloadAuths.length === authBefore,
      "offline playback did not reauthorize removed cloud song",
    );
    await theme("dark");
    await capture("download_kept_playback_dark");
    await nav("本地音乐");
    await wait(
      () => readyLocal() && localRows().length === 2,
      "retained local files",
    );
    await theme("light");
    await capture("library_kept_light");
    readMode = "reject";
    await wait(
      () =>
        local()?.querySelector("[data-local-read-error]") &&
        localRows().length === 2,
      "cached read failure",
    );
    assert(
      localRows().every((row) =>
        all("button", row).every((node) => node.disabled),
      ),
      "read failure locks retained rows",
    );
    await theme("dark");
    await capture("read_failed_cached_dark", { injectedReadFailure: true });
    readMode = "normal";
    click("重试读取", local());
    await wait(readyLocal, "cached read recovered");
    assert(document.activeElement === localField(), "cached retry focus");
    await theme("light");
    await capture("library_recovered_light");
    commandFails = true;
    click("重新扫描", local());
    await wait(
      () => !!local()?.querySelector("[data-local-command-error]"),
      "command error",
    );
    const readsBefore = completedReads;
    await wait(
      () => completedReads > readsBefore && readyLocal(),
      "background read after command failure",
    );
    await sleep(25);
    assert(
      local()
        .querySelector("[data-local-command-error]")
        ?.textContent.includes("合成扫描失败"),
      "successful poll preserves command error",
    );
    await theme("dark");
    await capture("command_failure_persisted_dark", {
      injectedCommandFailure: true,
    });
    commandFails = false;
    click("重新扫描", local());
    await wait(
      () =>
        !local()?.querySelector("[data-local-command-error]") && readyLocal(),
      "explicit command retry",
    );
    await theme("light");
    await capture("rescan_recovered_light");
    await wait(async () => (await originalLocal(localQuery)).roots.every((root) =>
      !root.enabled || !["pending", "scanning"].includes(root.status)), "native scan idle");
    let staleHide = false;
    try {
      await originalCommand({kind:"hide_track", id:indexedCue.track.id, revision:"0".repeat(64)});
    } catch (error) { staleHide = error?.code === "snapshot_changed"; }
    assert(staleHide && localRows().length === 2, "stale remove rejected");
    const hideTrigger = localRows().find(row => row.dataset.localId === cue).querySelector("[data-local-track-menu]");
    const removeKey = (element, key, modifiers = {}) => {
      const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...modifiers });
      element.dispatchEvent(event);
      return event.defaultPrevented;
    };
    localField().focus();
    assert(!removeKey(localField(), "Backspace"), "text field retains native editing");
    const editor = document.createElement("span");
    editor.contentEditable = "true";
    hideTrigger.closest("td").appendChild(editor);
    editor.focus();
    assert(!removeKey(editor, "Delete"), "row editor retains native editing");
    editor.remove();
    hideTrigger.focus();
    assert(removeKey(hideTrigger, "Delete", { repeat:true }), "repeated row key is consumed");
    assert(!removeKey(hideTrigger, "Backspace", { ctrlKey:true }), "modified key not intercepted");
    assert(!removeKey(hideTrigger, "Backspace", { isComposing:true }), "composition key not intercepted");
    assert(hideCommands === 0, "repeat, modifiers and editing do not hide a row");
    assert(removeKey(hideTrigger, "Backspace"), "plain row Backspace consumed");
    removeKey(hideTrigger, "Backspace", { repeat:true });
    await wait(() => readyLocal() && localRows().length === 1 &&
      local().textContent.includes("源文件仍保留") && document.activeElement === localField(),
      "only local list row removed with focus fallback");
    assert(!localRows().some(row => row.dataset.localId === cue), "CUE is removed from displayed list");
    assert((await control("cloud-removal-file", {songId:"1004", action:"observe"})).exists,
      "CUE physical source remains");
    assert(hideCommands === 1, "one native list-only hide");
    await capture("local_hidden_light", {staleHide, keyboardHide:hideCommands === 1});
    click("重新扫描", local());
    await wait(() => readyLocal() && localRows().some(row => row.dataset.localId === cue),
      "explicit rescan restores local list row");
    await capture("local_restored_light");
    const state = await control("cloud-state");
    assert(
      state.writes.length === 2 &&
        state.writes[0].join() === "1006" &&
        state.writes[1].join() === "1005",
      "two exact intentional cloud deletes",
    );
    assert(state.realRequests === 0, "no real API writes");
    client.localPage = originalLocal;
    client.localCommand = originalCommand;
    client.playSelection = originalSelect;
    await emit({
      kind: "complete",
      stage: "complete",
      ok: true,
      localResourceUi: true,
      stageCount: count,
      cloudDeletes: state.writes,
      deletedLocalFiles: 1,
      remainingDownloads: 2,
      remainingLocalTracks: 2,
      realAccountWrites: 0,
      realMusicRequests: 0,
    });
  } catch (error) {
    held.splice(0).forEach((resolve) => resolve());
    if (client && originalLocal) client.localPage = originalLocal;
    if (client && originalCommand) client.localCommand = originalCommand;
    if (client && originalSelect) client.playSelection = originalSelect;
    await emit({
      kind: "failed",
      stage: "failed",
      last,
      message: String(error?.message ?? error),
      stack: error?.stack,
    });
  }
})();
