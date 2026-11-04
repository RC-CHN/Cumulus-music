// Actual shared-scroller sidebar, production IPC, isolated synthetic account.
(async () => {
  const one = (s) => document.querySelector(s),
    all = (s) => [...document.querySelectorAll(s)];
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const assert = (v, m) => {
    if (!v) throw Error(m);
  };
  const wait = async (fn, m) => {
    for (let i = 0; i < 220; i++) {
      if (fn()) return;
      await sleep(40);
    }
    throw Error(m);
  };
  const emit = (stage, data = {}) =>
    window.__TAURI_INTERNALS__.invoke("plugin:event|emit", {
      event: "ux-proof",
      payload: { stage, ...data },
    });
  const button = (label, root = document) =>
    [...root.querySelectorAll("button")].find(
      (n) =>
        n.getAttribute("aria-label") === label ||
        n.textContent.trim() === label,
    );
  const side = () => one(".sidebar"),
    scroll = () => one(".sidebar-scroll");
  const group = (k) => one(`[data-sidebar-group="${k}"]`);
  const row = (id) => one(`[data-sidebar-id="${id}"]`);
  const key = (node, name, extra = {}) =>
    node.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: name,
        bubbles: true,
        cancelable: true,
        ...extra,
      }),
    );
  async function click(node) {
    assert(node && !node.disabled, "enabled control");
    node.scrollIntoView({ block: "nearest" });
    await sleep(50);
    node.focus({ preventScroll: true });
    node.click();
  }
  const fixture = async (mode) => {
    window.__libraryFixtureAck = null;
    await emit("library-fixture", { mode });
    await wait(() => window.__libraryFixtureAck, "fixture");
    return window.__libraryFixtureAck;
  };
  async function expanded(k, value) {
    if (
      group(k)
        .querySelector(".sidebar-group-toggle")
        .getAttribute("aria-expanded") !== String(value)
    )
      await click(group(k).querySelector(".sidebar-group-toggle"));
    await wait(
      () =>
        group(k)
          .querySelector(".sidebar-group-toggle")
          .getAttribute("aria-expanded") === String(value) &&
        !one(".sidebar-fold-enter-active,.sidebar-fold-leave-active"),
      "fold transition",
    );
  }
  async function top(k) {
    group(k).querySelector("header").scrollIntoView({ block: "start" });
    await sleep(100);
  }
  function visible(node) {
    const a = node.getBoundingClientRect(),
      b = scroll().getBoundingClientRect();
    return a.top >= b.top - 1 && a.bottom <= b.bottom + 1;
  }
  const point = (node, offset = 0.5) => {
    assert(node, 'drag target is mounted');
    const b = node.getBoundingClientRect();
    const pos = { clientX: b.left + 28, clientY: b.top + offset * b.height };
    const viewport = scroll().getBoundingClientRect();
    assert(pos.clientY >= viewport.top && pos.clientY <= viewport.bottom,
      `drag point on ${node.dataset.sidebarId} is visible: ${pos.clientY} vs ${viewport.top}–${viewport.bottom}`);
    return pos;
  };
  const pointer = (node, name, pos) =>
    node.dispatchEvent(new PointerEvent(name, {
      ...pos, pointerType: 'mouse', pointerId: 41, button: 0,
      buttons: name === 'pointerup' ? 0 : 1, bubbles: true, cancelable: true,
    }));
  async function dragTo(source, target, offset, label) {
    pointer(row(source), 'pointerdown', point(row(source)));
    const end = point(row(target), offset);
    pointer(document, 'pointermove', end);
    await wait(() => one('.sidebar-drop-line'), 'visible insertion marker');
    if (label) await capture(label);
    pointer(document, 'pointerup', end);
    await wait(() => !one('.sidebar-row-dragging'), 'drag released');
  }
  let count = 0,
    maxRows = 0,
    step = "startup",
    reads = 0;
  const fetch0 = window.fetch;
  window.fetch = async function (url, options) {
    if (String(url).endsWith("/account_playlists")) reads++;
    return fetch0.call(window, url, options);
  };
  async function capture(label) {
    step = label;
    await sleep(180);
    const rows = all(".playlist-nav");
    maxRows = Math.max(maxRows, rows.length);
    assert(
      rows.length <= Math.ceil(scroll().clientHeight / 46) + 20,
      "bounded sidebar rows",
    );
    assert(
      scroll().scrollWidth <= scroll().clientWidth &&
        document.documentElement.scrollWidth <= innerWidth,
      "no horizontal overflow",
    );
    for (const n of rows) {
      assert(Math.round(n.getBoundingClientRect().height) === 46, "46px row");
      assert(
        Math.round(
          n.querySelector(".sidebar-playlist-cover").getBoundingClientRect()
            .width,
        ) === 32,
        "32px cover",
      );
      assert(
        n.querySelector(".sidebar-playlist-name").getBoundingClientRect()
          .height <= 32,
        "two line name",
      );
    }
    await emit(`ux_${String(++count).padStart(2, "0")}_${label}`, {
      width: innerWidth,
      height: innerHeight,
      theme: document.documentElement.dataset.theme,
      rendered: rows.length,
    });
    await sleep(420);
  }
  async function theme(value) {
    await click(button("外观主题"));
    await wait(() => one(".action-menu"), "theme menu");
    await click(button(value === "dark" ? "深色" : "浅色"));
    await wait(() => !one(".action-menu"), "theme close");
  }
  try {
    await wait(
      () => one(".track-more") && group("subscribed"),
      "loaded account",
    );
    assert(
      group("created").querySelector("header").textContent.includes("999+"),
      "created large group",
    );
    if (localStorage.getItem("cumulus.sidebar.proof.finished")) {
      assert(
        group("created")
          .querySelector(".sidebar-group-toggle")
          .getAttribute("aria-expanded") === "false",
        "created folding survives process restart",
      );
      assert(
        group("subscribed")
          .querySelector(".sidebar-group-toggle")
          .getAttribute("aria-expanded") === "true",
        "subscribed folding independent",
      );
      assert(
        !group("created").querySelector(".playlist-nav"),
        "collapsed list stays unmounted",
      );
      await top("created");
      await capture("reopened_groups");
      await theme("light");
      await capture("reopened_light");
      await emit("complete", {
        ok: true,
        sidebarUi: true,
        reopened: true,
        stageCount: count,
        maxRows,
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
      await expanded("created", true);
      await expanded("subscribed", true);
      for (const n of all(".sidebar .nav-item")) {
        n.scrollIntoView({ block: "nearest" });
        await sleep(20);
        assert(visible(n), "navigation item reachable");
      }
      scroll().scrollTop = 0;
      await capture(`navigation_${w}_${t}`);
      await top("created");
      await wait(() => row("2"), "first created row");
      const r = row("2").getBoundingClientRect();
      window.__searchPointerAck = false;
      await emit("sidebar-pointer", {
        x: r.x + r.width / 2,
        y: r.y + r.height / 2,
      });
      await wait(
        () => window.__searchPointerAck && row("2").matches(":hover"),
        "native pointer hover",
      );
      await capture(`created_hover_${w}_${t}`);
      await click(row("2"));
      await wait(
        () =>
          one(".playlist-title") &&
          row("2")?.getAttribute("aria-current") === "page",
        "selected row",
      );
      await capture(`selected_${w}_${t}`);
      key(row("2"), "End");
      await wait(
        () => document.activeElement?.dataset.sidebarId === "1200",
        "keyboard End materializes final created row",
      );
      assert(visible(row("1200")), "last created row fully visible");
      await capture(`created_end_${w}_${t}`);
      key(row("1200"), "F10", { shiftKey: true });
      await wait(
        () => one(".action-menu")?.style.visibility === "visible",
        "keyboard menu",
      );
      assert(button("下移歌单").disabled, "last playlist boundary");
      const menu = one(".action-menu").getBoundingClientRect();
      assert(
        menu.left >= 0 && menu.bottom <= innerHeight,
        "menu stays inside window",
      );
      await capture(`context_menu_${w}_${t}`);
      key(one(".action-menu"), "Escape");
      await wait(() => !one(".action-menu"), "context close");
      assert(
        document.activeElement === row("1200"),
        "context restores row focus",
      );
      const point = row("1200").getBoundingClientRect();
      row("1200").dispatchEvent(
        new MouseEvent("contextmenu", {
          bubbles: true,
          cancelable: true,
          button: 2,
          clientX: point.left + 20,
          clientY: point.top + 20,
        }),
      );
      await wait(
        () => one(".action-menu")?.style.visibility === "visible",
        "pointer context menu",
      );
      key(one(".action-menu"), "Escape");
      await wait(() => !one(".action-menu"), "pointer menu closes");
      assert(
        document.activeElement === row("1200"),
        "pointer menu restores focus",
      );
      key(row("1200"), "F10", { shiftKey: true });
      await wait(
        () => one(".action-menu")?.style.visibility === "visible",
        "editor entry menu",
      );
      await click(button("编辑歌单信息", one(".action-menu")));
      await wait(
        () => one(".playlist-editor-page input")?.value === "合成歌单 1200",
        "sidebar opens matching playlist editor",
      );
      await click(button("取消编辑歌单"));
      await wait(
        () =>
          !one(".playlist-editor-page") &&
          document.activeElement === row("1200"),
        "editor returns to sidebar entry",
      );
      await expanded("created", false);
      assert(
        document.activeElement ===
          group("created").querySelector(".sidebar-group-toggle"),
        "fold heading keeps focus",
      );
      await capture(`folded_${w}_${t}`);
      await top("subscribed");
      await wait(() => row("1201"), "first collected");
      row("1201").focus({ preventScroll: true });
      key(row("1201"), "End");
      await wait(
        () => document.activeElement?.dataset.sidebarId === "2400",
        "End reaches last subscription",
      );
      assert(visible(row("2400")), "last subscription visible");
      await click(row("2400"));
      await wait(
        () => row("2400")?.getAttribute("aria-current") === "page",
        "subscription selected",
      );
      await capture(`subscribed_end_${w}_${t}`);
      await click(button("新建歌单", side()));
      await wait(() => one(".playlist-dialog[open]"), "sidebar create");
      await capture(`create_${w}_${t}`);
      await click(button("关闭歌单操作"));
      await wait(() => !one(".playlist-dialog"), "create cancelled");
      assert(
        document.activeElement === button("新建歌单", side()),
        "create focus returns",
      );
    }
    await expanded("created", true);
    await top("created");
    // Preserve a keyboard entry point when passive scrolling recycles the focused row.
    row("2").focus({ preventScroll: true });
    scroll().scrollTop += 2000;
    await sleep(120);
    assert(
      document.activeElement ===
        group("created").querySelector(".sidebar-playlist-list"),
      "recycled focus retained on list",
    );
    key(document.activeElement, "ArrowDown");
    await wait(
      () =>
        document.activeElement?.dataset.sidebarId &&
        visible(document.activeElement),
      "arrow navigation resumes at visible rows",
    );
    key(document.activeElement, "Home");
    await wait(
      () => document.activeElement?.dataset.sidebarId === "2",
      "Home returns from recycled focus",
    );
    await click(button("我的歌单", side()));
    await wait(() => one(".playlist-library-page"), "library for playback");
    await top("created");
    await click(
      one('[data-library-open="2"]')
        .closest("article")
        .querySelector(".library-card-play"),
    );
    await wait(
      () => row("2")?.querySelector(".sidebar-playing"),
      "playing indicator",
    );
    await capture("playing");
    await click(
      one('[data-library-open="2"]')
        .closest("article")
        .querySelector(".library-card-play"),
    );
    await wait(
      () => !one(".sidebar-playing"),
      "pause removes playing indicator",
    );
    await capture("paused");
    const importTrigger = button("歌单导入预览");
    const importDialog = () => one(".external-playlist-preview[open]");
    const paste = (index, text) => {
      const input = importDialog().querySelector(`[data-import-input="${index}"]`);
      input.value = text;
      input.dispatchEvent(new Event("input", { bubbles: true }));
    };
    const readsBeforePreview = reads;
    await click(importTrigger);
    await wait(() => importDialog() && document.activeElement?.dataset.importInput === "0", "preview initial focus");
    assert(button("预览链接", importDialog()).disabled, "blank preview cannot run");
    paste(0, "分享歌单 https://y.qq.com/playlist/synthetic?share=PRIVATE");
    await click(button("添加链接", importDialog()));
    await wait(() => document.activeElement?.dataset.importInput === "1", "new field focus");
    paste(1, "file:///synthetic/path");
    await click(button("预览链接", importDialog()));
    await wait(() => importDialog()?.textContent.includes("已识别 1 个链接"), "native link preview");
    assert(importDialog().textContent.includes("QQ音乐 · y.qq.com") &&
      importDialog().textContent.includes("未识别到网页链接") &&
      !importDialog().textContent.includes("PRIVATE") && !importDialog().querySelector("a"),
      "only source and host are displayed, without navigation or raw parameters");
    await capture("external_playlist_preview");
    paste(1, "https://xhslink.com/synthetic");
    await wait(() => !importDialog().querySelector('[role="status"]'), "edit invalidates old preview");
    await click(button("预览链接", importDialog()));
    await wait(() => importDialog()?.textContent.includes("已识别 2 个链接"), "edited preview");
    importDialog().dispatchEvent(new Event("cancel", {cancelable:true}));
    await wait(() => !importDialog() && document.activeElement === importTrigger, "preview closes to trigger");
    await click(importTrigger);
    await wait(() => importDialog()?.querySelector("[data-import-input]"), "reopen preview");
    assert(importDialog().querySelectorAll("[data-import-input]").length === 1 &&
      !importDialog().querySelector("[data-import-input]").value && reads === readsBeforePreview,
      "local draft is discarded and no library read was needed");
    for (let i = 1; i < 10; i++) await click(button("添加链接", importDialog()));
    assert(button("添加链接", importDialog()).disabled, "official ten-entry ceiling");
    await click(button("关闭导入预览", importDialog()));
    await wait(() => !importDialog() && document.activeElement === importTrigger, "close bounded preview");
    await fixture("fail");
    await click(button("刷新当前页面"));
    await wait(() => one('.sidebar-status[role="alert"]'), "read failure");
    one('.sidebar-status[role="alert"]').scrollIntoView({ block: "start" });
    await capture("read_failed");
    await fixture("hold");
    const retry = button("重新加载歌单", side()),
      before = reads;
    retry.focus({ preventScroll: true });
    retry.click();
    retry.click();
    await capture("retry_wait");
    assert(reads === before + 1, "sidebar retry deduplicated");
    await fixture("success");
    await wait(() => !one('.sidebar-status[role="alert"]'), "retry recovered");
    await wait(
      () => document.activeElement === scroll(),
      "retry restores stable sidebar focus",
    );
    const colorCheck = document.createElement("span");
    colorCheck.style.color = "var(--accent)";
    document.body.append(colorCheck);
    assert(
      getComputedStyle(scroll()).outlineColor ===
        getComputedStyle(colorCheck).color,
      "recovered focus follows theme color",
    );
    colorCheck.remove();
    await capture("read_recovered");
    await top("created");
    row("2").focus({ preventScroll: true });
    await fixture("empty");
    // Invoke the existing refresh entry without moving keyboard focus off the row.
    button("刷新当前页面").click();
    await wait(
      () => !group("subscribed") && !one(".playlist-nav"),
      "empty account resources",
    );
    assert(
      document.activeElement === scroll(),
      "removed resources preserve keyboard focus",
    );
    await capture("empty_groups");
    await fixture("success");
    await click(button("刷新当前页面"));
    await wait(() => group("subscribed"), "resources restored");
    await top("created");
    const setter = Storage.prototype.setItem;
    Storage.prototype.setItem = function (k, v) {
      if (k === "cumulus.sidebar-playlists.v1")
        throw Error("synthetic storage refusal");
      return setter.call(this, k, v);
    };
    await expanded("created", false);
    await wait(
      () => side().textContent.includes("折叠状态未能保存"),
      "fold storage error",
    );
    await capture("preference_failed");
    Storage.prototype.setItem = setter;
    await expanded("created", true);
    window.__motionProof.system(true);
    await wait(
      () => document.documentElement.dataset.motion === "reduced",
      "reduced policy",
    );
    await expanded("created", false);
    await capture("reduced_motion");
    window.__motionProof.system(false);
    await expanded('created', true);
    await top('created');
    const base = await fixture('success');
    assert(base.writes === 0, 'no preexisting reorder writes');

    // Both adjacent boundaries of the source row are no-ops. Move into the
    // subscribed group before releasing; the original pointer remains owned.
    await dragTo('2', '2', 0.12);
    pointer(row('3'), 'pointerdown', point(row('3')));
    pointer(document, 'pointermove', point(row('2'), 0.12));
    await wait(() => one('.sidebar-drop-line'), 'active created drag');
    await top('subscribed');
    const cross = point(row('1201'), 0.12);
    pointer(document, 'pointermove', cross);
    await wait(() => !one('.sidebar-drop-line'), 'other group hides marker');
    pointer(document, 'pointerup', cross);
    assert((await fixture('success')).writes === 0, 'no-op and cross group do not write');
    await capture('drag_other_group_cancelled');

    await top('created');
    const startScroll = scroll().scrollTop;
    pointer(row('3'), 'pointerdown', point(row('3')));
    const box = scroll().getBoundingClientRect();
    const edge = { clientX: point(row('3')).clientX, clientY: box.bottom - 4 };
    pointer(document, 'pointermove', edge);
    await wait(() => scroll().scrollTop > startScroll + 520 && !row('3'), 'edge scroll recycles source');
    assert(one('.sidebar-drop-line'), 'marker survives recycling');
    await capture('drag_edge_scroll');
    key(document.body, 'Escape');
    await wait(() => !one('.sidebar-drop-line'), 'escape removes marker');
    pointer(document, 'pointerup', edge);
    assert((await fixture('success')).writes === 0, 'escape does not write');

    await top('created');
    await fixture('stale_order');
    await dragTo('4', '2', 0.12);
    await wait(() => one('[data-sidebar-order-notice]')?.textContent.includes('歌单列表已变化'),
      'changed remote order is rejected');
    assert((await fixture('success')).writes === 0, 'stale drop does not write');
    await capture('drag_stale_order');

    await top('created');
    await dragTo('3', '2', 0.12, 'drag_created_marker');
    await wait(() => row('3')?.dataset.sidebarIndex === '0', 'created readback');
    assert((await fixture('success')).createdOrder.join() === '1,3,2,4,5', 'favorite retained');
    await capture('drag_created_confirmed');
    await top('created');
    await dragTo('3', '2', 0.88);
    await wait(() => row('2')?.dataset.sidebarIndex === '0', 'created original restored');
    await expanded('created', false);
    step = 'drag_subscribed_setup';
    await top('subscribed');
    await wait(() => row('1201') && row('1202'), 'subscribed first rows mounted');
    await dragTo('1202', '1201', 0.12, 'drag_subscribed_marker');
    await wait(() => row('1202')?.dataset.sidebarIndex === '0', 'subscribed readback');
    assert((await fixture('success')).subscribedOrder.join() === '1202,1201,1203,1204,1205',
      'subscribed permutation');
    await capture('drag_subscribed_confirmed');
    await top('subscribed');
    await wait(() => row('1201') && row('1202'), 'subscribed reordered rows mounted');
    await dragTo('1202', '1201', 0.88);
    await wait(() => row('1201')?.dataset.sidebarIndex === '0', 'subscribed original restored');
    await fixture("expired");
    await click(button("刷新当前页面"));
    await wait(() => one(".welcome"), "session expired");
    assert(
      !one(".playlist-nav") && !group("subscribed"),
      "old account sidebar removed",
    );
    await capture("session_expired");
    localStorage.setItem("cumulus.sidebar.proof.finished", "1");
    await emit("complete", {
      ok: true,
      sidebarUi: true,
      stageCount: count,
      maxRows,
      realMusicRequests: 0,
      realAccountWrites: 0,
      focusRecovery: true,
      foldingVerified: true,
      reorderVerified: true,
      externalPlaylistPreviewVerified: true,
    });
  } catch (error) {
    await emit("failed", {
      step,
      count,
      message: String(error),
      focus: document.activeElement?.outerHTML?.slice(0, 500),
      rendered: all(".playlist-nav").length,
    });
  }
})();
