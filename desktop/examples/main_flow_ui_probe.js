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
    node.focus({ preventScroll: true });
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
    last = "guest";
    const guest = await wait(async () => {
      const view = await invoke("account_snapshot");
      const entry = one(".account-button");
      return ["unknown", "signed_out"].includes(view.status) && entry?.title === "点击登录" && !entry.disabled && view;
    }, "signed-out startup");
    await stage("guest", { epoch: guest.epoch, status: guest.status });

    last = "qr_pending";
    await click(() => one(".account-button"), "guest login entry");
    await wait(() => one('.login-dialog[open] svg[aria-label="网易云音乐登录二维码"]'), "rendered login QR");
    await wait(() => one('.login-dialog[open] .qr-confirmation'), "QR scanned, awaiting confirmation");
    const pending = await invoke("account_snapshot");
    check(pending.status === guest.status && pending.epoch === guest.epoch, "QR confirmation must retain guest account");
    await stage("qr_pending", { epoch: pending.epoch, status: pending.status });

    last = "login_adopted";
    await emit("approve_login");
    const account = await wait(async () => {
      let view;
      try {
        view = await invoke("account_snapshot");
      } catch (error) {
        // The read can straddle guest→authenticated adoption; let the next
        // observation confirm the new epoch without replaying approval.
        if (error?.code === "stale") return false;
        throw error;
      }
      return view.status === "authenticated" && view.epoch !== guest.epoch &&
        view.nickname === "合成主流程账号" && one(".account-button")?.title === view.nickname &&
        !one(".login-dialog[open]") && view;
    }, "QR login adopted by the app");
    await stage("login_adopted", { epoch: account.epoch, previousEpoch: guest.epoch, status: account.status });
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
    const songPanel = '[aria-labelledby="search-tab-songs"]';
    const songRow = '[data-track-id="1"]';
    async function search() {
      input(one(".searchbox input"), "合成主流程歌曲");
      one(".searchbox").dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
      await wait(() => one(".search-heading-copy h1")?.textContent === "合成主流程歌曲", "search title");
      await click(() => one('[data-category="songs"]'), "song results tab");
      await wait(() => one(songPanel)?.getAttribute("aria-busy") === "false" &&
        one(`${songPanel} ${songRow}`)?.textContent.includes("合成主流程歌曲"),
      "search song result");
    }
    await search();
    await stage("search");
    const decoderGate = !window.__mainFlowOrdinaryHttps && !window.__mainFlowLoadingStep;
    const cover = (action) => one(`.search-track-card[data-result-key="song:1"] .search-track-cover[aria-label^="${action} "]`);
    if (window.__mainFlowHttpsEof || window.__mainFlowLoadingStep)
      await click(() => one(".search-result-actions .primary"), "search play all for the two-song queue");
    else if (decoderGate) {
      await click(() => one('[data-category="all"]'), "comprehensive search results");
      await click(() => cover("播放"), "direct play from a comprehensive search cover");
    }
    else
      await click(() => one(`${songPanel} ${songRow} .track-play`), "search play control");
    let loadingSeekVerified = false;
    let loadingPauseVerified = false;
    let loadingStepVerified = false;
    let loadingSearchCoverVerified = false;
    if (window.__mainFlowLoadingStep) {
      const held = await wait(async () => {
        const player = await invoke("player_snapshot", { epoch });
        return player.state === "loading" && player.track?.id === "1" && player.queueLen === 2 && player;
      }, "first decoder blocked in a two-song queue");
      await wait(() => {
        const previous = button("上一首", one(".playerbar"));
        return previous && !previous.disabled;
      }, "previous also stays enabled");
      await click(() => button("下一首", one(".playerbar")), "skip a blocked decoder from the footer");
      await wait(async () => {
        const player = await invoke("player_snapshot", { epoch });
        return player.state === "playing" && player.track?.id === "2" &&
          player.playId !== held.playId && player.queueLen === 2 && player.position >= .1;
      }, "native next track advances without releasing the first decoder gate");
      loadingStepVerified = true;
    } else if (!window.__mainFlowOrdinaryHttps) {
      const slider = await wait(() => {
        const range = one('.playerbar input[aria-label="播放进度"]');
        return state() === "loading" && range && !range.disabled && range;
      }, "authorized loading progress accepts a seek");
      const id = one(".playerbar")?.dataset.playId;
      const identity = await invoke("player_snapshot", { epoch });
      const sameSource = (playback) => playback.playId === id && playback.entryId === identity.entryId &&
        playback.queueRevision === identity.queueRevision && playback.track?.id === "1";
      await wait(() => cover("暂停") && !cover("暂停").disabled,
        "the search cover is enabled and matches the loading pause intent");
      input(slider, ".45", "change");
      await wait(() => state() === "loading" && position() >= .45,
        "native actor accepted position while decode is gated");
      await click(() => button("暂停", one(".playerbar")), "pause control remains available during loading");
      await wait(async () => {
        const playback = await invoke("player_snapshot", { epoch });
        const resume = button("播放", one(".playerbar"));
        return playback.state === "loading" && sameSource(playback) &&
          playback.pendingPaused === true && resume && !resume.disabled;
      }, "native actor accepted pause before decoder completion");
      await click(() => cover("播放"), "continue the same loading decoder directly from search");
      await wait(async () => {
        const playback = await invoke("player_snapshot", { epoch });
        return playback.state === "loading" && sameSource(playback) &&
          playback.pendingPaused === false && cover("暂停") && !cover("暂停").disabled;
      }, "search cover continues the same selection during decoder loading");
      await click(() => cover("暂停"), "pause the same loading decoder directly from search");
      await wait(async () => {
        const playback = await invoke("player_snapshot", { epoch });
        return playback.state === "loading" && sameSource(playback) &&
          playback.pendingPaused === true && cover("播放") && !cover("播放").disabled;
      }, "search cover retains the same paused decoder and queue");
      loadingSearchCoverVerified = true;
      await stage("search_loading_cover", { samePendingSelection: true });
      await emit("release_loading_seek");
      await wait(() => state() === "paused" && one(".playerbar")?.dataset.playId === id &&
        position() >= .45, "same decoder first presents paused at the accepted position");
      loadingSeekVerified = true;
      await click(() => button("播放", one(".playerbar")), "resume the same decoded entry");
      await wait(() => state() === "playing" && one(".playerbar")?.dataset.playId === id &&
        position() > .48, "same decoded entry progresses after explicit resume");
      loadingPauseVerified = true;
    }
    if (window.__mainFlowHttpsRetry) {
      const retry = () => button("重试", one(".playback-message"));
      const openFailure = () => state() === "error" &&
        one(".playback-message")?.textContent.includes("音源已选定，但无法打开或解码") &&
        retry() && !retry().disabled;
      await wait(openFailure, "ordinary HTTPS failure and explicit retry");
      check(one(".playerbar")?.dataset.trackId === "1", "failed HTTPS selection retained");
      await sleep(600);
      check(openFailure(), "no automatic HTTPS replay");
      await stage("https_failure", { failedBeforeExplicitRetry: true, rawLocatorVisible:
        document.body.innerText.includes("cumulus-local-fixture.music.126.net") });
      await wait(() => window.__mainFlowHttpsFailureRecorded, "native failure receipt");
      await click(retry, "explicit HTTPS retry");
      await wait(() => !one(".playback-message"), "retry clears playback feedback");
    }
    const expectedTrack = window.__mainFlowLoadingStep ? "2" : "1";
    await wait(() => state() === "playing" && one(".playerbar")?.dataset.trackId === expectedTrack && position() >= .1, "native playback clock");
    let advancedByEof = false;
    if (window.__mainFlowHttpsEof) {
      const firstId = one(".playerbar")?.dataset.playId;
      check(firstId, "first native HTTPS entry");
      await emit("wait_https_prefetch");
      await wait(() => window.__mainFlowHttpsPrefetched, "next authorization prefetched without loading");
      check(state() === "playing" && one(".playerbar")?.dataset.playId === firstId, "prefetch retained first entry");
      await stage("https_before_eof", { firstTrackPlaying: true });
      await wait(() => window.__mainFlowBeforeEofRecorded, "first native HTTPS receipt");
      await wait(() => state() === "playing" && one(".playerbar")?.dataset.trackId === "2" &&
        one(".playerbar")?.dataset.playId && one(".playerbar").dataset.playId !== firstId &&
        position() >= .1, "native EOF advances to next HTTPS entry");
      advancedByEof = true;
    }
    await click(() => button("暂停", one(".playerbar")), "pause");
    await wait(() => state() === "paused", "native paused");
    const playId = one(".playerbar")?.dataset.playId;
    let resumedSameEntry = false;
    if (window.__mainFlowOrdinaryHttps) {
      const pausedPosition = position();
      check(playId && Number.isFinite(pausedPosition), "paused HTTPS entry");
      await sleep(100);
      check(Math.abs(position() - pausedPosition) < .05, "HTTPS clock remains paused");
      await click(() => button("播放", one(".playerbar")), "resume online playback");
      await wait(() => state() === "playing" && one(".playerbar")?.dataset.playId === playId &&
        position() > pausedPosition + .08, "same native HTTPS clock resumed");
      resumedSameEntry = true;
      await click(() => button("暂停", one(".playerbar")), "pause resumed online playback");
      await wait(() => state() === "paused", "resumed native entry paused");
    }
    await stage("playback", { nativeProgress: true, paused: true, resumedSameEntry, advancedByEof,
      loadingSeekVerified, loadingPauseVerified, loadingStepVerified, loadingSearchCoverVerified, output: "null" });
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
    await search();
    await click(() => one(`${songPanel} ${songRow} .track-more`), "open exact search result menu");
    await click(() => button("收藏", one(".song-action-menu")), "choose playlist destination");
    await click(() => one(`.playlist-collect-dialog[open] [data-playlist-id="${created.id}"]`), "add search song to exact private playlist");
    await wait(() => !one(".playlist-dialog"), "confirmed add closes picker");
    const privateSongs = async () => {
      const reply = await invoke("playlist_page", { id: created.id, page: { offset: 0, limit: 10 } });
      check(reply.epoch === epoch, "playlist readback changed account");
      return reply.value.tracks.map(track => track.id);
    };
    check(JSON.stringify(await privateSongs()) === JSON.stringify(["1"]), "private playlist add readback");
    await click(() => button("我的歌单"), "open playlist library after add");
    await click(() => one(`.playlist-card[data-library-open="${created.id}"]`), "open exact private playlist");
    await wait(() => one(".playlist-hero h1")?.textContent === renamed && one(`.track-row${songRow}`), "added track displayed");
    await stage("playlist_song_added", { playlistId: created.id, trackIds: await privateSongs() });
    await click(() => one(`.track-row${songRow} .track-more`), "private playlist song menu");
    await click(() => button("从歌单中删除", one(".song-action-menu")), "remove only the added song");
    await click(() => button("删除", one(".playlist-dialog")), "confirm track removal");
    await wait(() => !one(".playlist-dialog") && !one(`.track-row${songRow}`), "removed track no longer displayed");
    check((await privateSongs()).length === 0, "private playlist remove readback");
    await stage("playlist_song_removed", { playlistId: created.id, trackIds: await privateSongs() });
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
    await click(() => one(`${songPanel} ${songRow} .track-more`), "song menu");
    await click(() => button("下载", one(".song-action-menu")), "download song");
    await click(() => one(".download-dialog button.primary"), "submit download");
    await click(() => button("下载管理"), "download queue navigation");
    await click(() => one("#download-tab-active"), "active download tab");
    await emit("wait_download_prefix");
    await wait(() => window.__mainFlowPrefixWaiting && one(".download-progress"), "partial download barrier");
    await click(() => button("全部暂停"), "pause active download");
    const active = { tab: "active", keyword: "", sort: "completed", descending: true, offset: 0, limit: 100 };
    const partial = await wait(async () => {
      const result = await invoke("downloads", { epoch, query: active });
      return one(".download-progress-status")?.textContent.trim() === "已暂停" &&
        result.tasks.find(task => task.status === "paused" && task.bytes > 0 && task.bytes < task.total);
    }, "paused task retains a partial transfer");
    await stage("download_paused", { taskId: partial.id, bytes: partial.bytes });
    await wait(() => window.__mainFlowPauseVerified === true, "native partial file checkpoint");
    await click(() => button("全部下载"), "resume paused download");
    const query = { tab: "completed", keyword: "", sort: "completed", descending: true, offset: 0, limit: 100 };
    const file = await wait(async () => {
      const result = await invoke("downloads", { epoch, query });
      return result.tasks.find(t => !t.cleanupPending);
    }, "published download");
    check(file.id === partial.id && file.bytes > partial.bytes && file.completedMs && !file.encrypted, "download resumed same task");
    await click(() => one("#download-tab-song"), "completed downloads");
    await wait(() => one('.download-row button[aria-label^="播放下载"]'), "download playback control");
    await stage("download_complete", { bytes: file.bytes });
    await wait(() => window.__mainFlowDownloadVerified === true, "native file hash and ledger checkpoint");
    await emit("disconnect");
    await wait(() => window.__mainFlowOffline === true, "native offline acknowledgement");
    await click(() => one('.download-row button[aria-label^="播放下载"]'), "play downloaded file");
    await wait(() => state() === "playing" && position() >= .1, "offline local playback clock");
    await stage("local_offline_playback", { nativeProgress: true, output: "null" });
    await click(() => button("停止播放", one(".playerbar")), "stop offline playback");
    await wait(() => state() === "idle", "offline player stopped before disposal");
    const manage = () => {
      const rows = all(".download-row[data-download-index]");
      return rows.length === 1 && rows[0].textContent.includes("合成主流程歌曲") &&
        rows[0].querySelector('[data-download-action="manage"]');
    };
    await click(manage, "manage exact synthetic download");
    await click(() => button("删除下载", one('.action-menu[aria-label="下载资源操作"]')), "delete synthetic download");
    await click(() => button("确认删除本地文件"), "confirm exact local disposal");
    await wait(async () => {
      const result = await invoke("downloads", { epoch, query });
      return !one(".download-dialog") && !result.tasks.some(t => t.id === file.id) &&
        result.operations.length === 0 && result.jobs.some(job => !job.running && !job.error);
    }, "native disposal completed");
    await stage("download_disposed", { stoppedBeforeDelete: true });
    await wait(() => window.__mainFlowDownloadDisposed === true, "native disposal ledger checkpoint");
    await stage("complete", { ok: true, allInOneWebView: true });
  } catch (error) {
    await emit("failed", { ok: false, last, playerState: state(), playbackMessage: one(".playback-message")?.textContent?.trim(), reason: String(error?.message ?? error) });
  }
})();
