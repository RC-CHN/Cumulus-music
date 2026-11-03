// Actual Vue controls and production IPC. No fixture state is injected into Vue.
(async () => {
  if (window.__mainFlowStarted) return;
  window.__mainFlowStarted = true;
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, payload = {}) => invoke("plugin:event|emit", {
    event: "ux-proof", payload: { stage, ...payload },
  });
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])]
    .filter(n => !n.closest("[inert],.page-leave-active,.modal-leave-active") && n.getClientRects().length);
  const one = selector => all(selector)[0];
  const button = (label, root = document) => all("button", root).find(n =>
    n.getAttribute("aria-label") === label || n.textContent.trim() === label);
  const check = (result, label) => { if (!result) throw Error(label); };
  async function wait(test, label) {
    for (let i = 0; i < 700; i++) {
      const value = await test();
      if (value) return value;
      await sleep(30);
    }
    throw Error(label);
  }
  async function click(get, label) {
    const node = await wait(() => {
      const n = get();
      return n && !n.disabled && n.getAttribute("aria-disabled") !== "true" && n;
    }, label);
    node.click();
  }
  function input(node, value, event = "input") {
    check(node, "missing input");
    node.value = value;
    node.dispatchEvent(new Event(event, { bubbles: true }));
  }
  const state = () => one(".playerbar")?.dataset.state;
  const position = () => Number(one(".playerbar")?.dataset.position);
  const title = "Cumulus Synthetic Flow";
  const renamed = "Cumulus Synthetic Flow Edited";
  const description = "Synthetic main-flow readback";
  let last = "bootstrap";
  async function stage(name, extra) {
    last = name;
    check(document.documentElement.scrollWidth <= innerWidth, "viewport overflow");
    await wait(async () => {
      await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      return !document.getAnimations().some(animation =>
        Number.isFinite(animation.effect?.getComputedTiming().endTime) &&
        (animation.pending || animation.playState === "running"));
    }, "settled paint " + name);
    await emit(name === "complete" ? name : "ux_" + name, extra);
  }
  try {
    const account = await wait(async () => {
      const view = await invoke("account_snapshot");
      return view.status === "authenticated" && view.nickname === "合成主流程账号" && view;
    }, "recover account through app startup");
    const epoch = account.epoch;
    const playlists = async () => (await invoke("account_playlists")).value;
    const baseline = (await playlists()).map(p => p.id);
    check(baseline.length === 1, "unexpected playlist baseline");
    await click(() => button("我的歌单"), "restored account playlist library");
    await wait(() => one(".playlist-card")?.textContent.includes("合成基线歌单"), "restored account resources visible");
    await stage("recovered", { baselineCount: baseline.length });

    await click(() => button("每日推荐"), "daily navigation");
    await wait(() => one(".daily-heading-copy") && one(".track-row")?.textContent.includes("合成主流程歌曲"), "daily result");
    await stage("daily");
    async function search() {
      input(one(".searchbox input"), "合成主流程歌曲");
      one(".searchbox").dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
      await wait(() => one(".search-heading-copy h1")?.textContent === "合成主流程歌曲", "search title");
      await click(() => one('[data-category="songs"]'), "song results tab");
      await wait(() => one(".search-result-panel")?.getAttribute("aria-busy") === "false" &&
        one(".search-result-panel .track-row")?.textContent.includes("合成主流程歌曲"),
      "search song result");
    }
    await search();
    await stage("search");
    await click(() => one(".track-play"), "search play control");
    await wait(() => state() === "playing" && position() >= .1, "native playback clock");
    await click(() => button("暂停", one(".playerbar")), "pause");
    await wait(() => state() === "paused", "native paused");
    await stage("playback", { nativeProgress: true, paused: true, output: "null" });
    await click(() => button("停止播放", one(".playerbar")), "stop online playback");
    await wait(() => state() === "idle", "online player stopped");

    await click(() => button("我的歌单"), "playlist library");
    await click(() => button("新建歌单"), "create playlist");
    await wait(() => one('.playlist-dialog [aria-label="歌单名称"]'), "playlist name");
    input(one('.playlist-dialog [aria-label="歌单名称"]'), title);
    check(one('[aria-label="设为私密歌单"]').checked, "playlist must be private");
    await click(() => one('.playlist-dialog button[type="submit"]'), "create submit");
    await wait(() => one(".playlist-hero h1")?.textContent === title, "playlist created");
    const created = (await playlists()).find(p => p.name === title && p.owned && p.private && !baseline.includes(p.id));
    check(created, "disposable playlist readback");
    await stage("playlist_created");

    await click(() => button("编辑歌单"), "edit playlist");
    await wait(() => one('.playlist-editor-page [aria-label="歌单名称"]'), "playlist editor");
    input(one('.playlist-editor-page [aria-label="歌单名称"]'), renamed);
    input(one('.playlist-editor-page [aria-label="歌单简介"]'), description);
    await click(() => one('.playlist-editor-page button[type="submit"]'), "edit submit");
    await wait(() => !one(".playlist-editor-page") && one(".playlist-hero h1")?.textContent === renamed, "playlist edited");
    const edited = (await invoke("playlist_editor", { epoch, id: created.id })).value;
    check(edited.name === renamed && edited.description === description, "playlist edit readback");
    await stage("playlist_edited");
    await click(() => button("编辑歌单"), "return to editor");
    await click(() => button("其他歌单操作"), "playlist options");
    await click(() => button("删除歌单", one(".action-menu")), "delete disposable playlist");
    await click(() => one(".playlist-editor-confirm button.primary"), "confirm playlist deletion");
    await wait(() => one(".playlist-card") && !one(".playlist-editor-page"), "return after playlist deletion");
    check(JSON.stringify((await playlists()).map(p => p.id)) === JSON.stringify(baseline), "playlist baseline changed");
    await stage("playlist_cleaned", { fullBaselineRestored: true });

    await click(() => button("下载管理"), "downloads navigation");
    await click(() => button("下载设置"), "download settings");
    await wait(() => one('[aria-label="下载目录"]') && window.__mainFlowDownloadPath, "isolated download directory");
    input(one('[aria-label="下载目录"]'), window.__mainFlowDownloadPath);
    await click(() => one(".download-dialog button.primary"), "save isolated download settings");
    await wait(() => !one(".download-dialog"), "settings saved");
    await search();
    await click(() => one(".track-more"), "song menu");
    await click(() => button("下载", one(".song-action-menu")), "download song");
    await click(() => one(".download-dialog button.primary"), "submit download");
    const query = { tab: "completed", keyword: "", sort: "completed", descending: true, offset: 0, limit: 100 };
    const file = await wait(async () => {
      const result = await invoke("downloads", { epoch, query });
      return result.tasks.find(t => !t.cleanupPending);
    }, "published download");
    check(file.bytes > 0 && file.completedMs && !file.encrypted, "download file readback");
    await click(() => one("#download-tab-song"), "completed downloads");
    await wait(() => one('.download-row button[aria-label^="播放下载"]'), "download playback control");
    await stage("download_complete", { bytes: file.bytes });
    await emit("disconnect");
    await wait(() => window.__mainFlowOffline === true, "native offline acknowledgement");
    await click(() => one('.download-row button[aria-label^="播放下载"]'), "play downloaded file");
    await wait(() => state() === "playing" && position() >= .1, "offline local playback clock");
    await stage("local_offline_playback", { nativeProgress: true, output: "null" });
    await stage("complete", { ok: true, allInOneWebView: true });
  } catch (error) {
    await emit("failed", { ok: false, last, playerState: state(), playbackMessage: one(".playback-message")?.textContent?.trim(), reason: String(error?.message ?? error) });
  }
})();
