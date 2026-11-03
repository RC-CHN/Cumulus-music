// Use actual Vue controls. All observations of playback come from production IPC.
(async () => {
  if (window.__coldOfflineStarted) return;
  window.__coldOfflineStarted = true;
  const fixture = window.__coldOfflineFixture;
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, values = {}) =>
    invoke("plugin:event|emit", { event: "cold-offline-proof", payload: { stage, ...values } });
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const visible = node =>
    node && !node.closest("[inert],.page-leave-active,.modal-leave-active") && node.getClientRects().length;
  const one = selector => [...document.querySelectorAll(selector)].find(visible);
  const button = text => [...document.querySelectorAll("button")].find(
    node => visible(node) && (node.textContent.trim() === text || node.getAttribute("aria-label") === text)
  );
  const check = (value, label) => { if (!value) throw Error(label); };
  async function wait(test, label) {
    for (let i = 0; i < 260; i++) {
      const result = await test();
      if (result) return result;
      await sleep(30);
    }
    throw Error(label);
  }
  async function click(get, label) {
    const target = await wait(() => {
      const node = get();
      return node && !node.disabled && node.getAttribute("aria-disabled") !== "true" && node;
    }, label);
    target.focus({ preventScroll: true });
    target.click();
  }
  async function denied(command, args) {
    try { await invoke(command, args); } catch { return true; }
    return false;
  }
  let step = "launch";
  try {
    check(!one("[data-cold-offline-panel]"), "offline mode unexpectedly opened automatically");
    const unknown = await invoke("account_snapshot");
    check(unknown.status === "unknown", "launch must not authenticate the saved account");

    step = "explicit open";
    await click(() => button("下载管理"), "download navigation");
    await click(() => one("[data-cold-offline-open]"), "offline download entry after restore failure");
    const panel = await wait(() => one("[data-cold-offline-panel]"), "offline download panel");
    await wait(() => panel.textContent.includes(fixture.title), "published title");
    if (fixture.capture) {
      await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      await wait(() => !document.getAnimations().some(animation =>
        Number.isFinite(animation.effect?.getComputedTiming().endTime) &&
        (animation.pending || animation.playState === "running")), "offline panel settled paint");
      await emit("panel_ready");
      await wait(() => window.__coldOfflineCaptured, "offline panel capture acknowledgement");
    }

    step = "negative scope";
    check(await denied("account_playlists"), "online library available without authentication");
    check(await denied("cold_offline_play", { id: "11111111111111111111111111111111" }),
      "unknown offline ID was accepted");
    check((await invoke("cold_offline_status")).phase === "idle", "unexpected playback");

    step = "local native playback";
    await click(() => [...panel.querySelectorAll("[data-cold-offline-play]")]
      .find(node => visible(node) && node.dataset.coldOfflinePlay === fixture.id), "published download play");
    const playing = await wait(async () => {
      const status = await invoke("cold_offline_status");
      return status.phase === "playing" && status.playingId === fixture.id &&
        Number.isFinite(status.position) && status.position >= .12 && status;
    }, "native playback position advances");
    const later = await wait(async () => {
      const status = await invoke("cold_offline_status");
      return status.phase === "playing" && status.playingId === fixture.id &&
        Number.isFinite(status.position) && status.position > playing.position && status;
    }, "native playback position advances twice");
    check(!JSON.stringify(later).includes("/proof"), "native file path escaped through IPC");

    step = "stop and close";
    await click(() => one("[data-cold-offline-stop]"), "offline stop");
    await wait(async () => (await invoke("cold_offline_status")).phase === "idle", "native stop");
    await click(() => one("[data-cold-offline-close]"), "offline close");
    await wait(() => !one("[data-cold-offline-panel]"), "offline panel closed");
    check((await invoke("account_snapshot")).status === "unknown", "offline playback promoted account");
    await emit("complete", {
      nativePosition: later.position,
      nativeStartPosition: playing.position,
      publishedIdMatched: true,
      unknownIdDenied: true,
      onlineLibraryDenied: true,
      stoppedAndClosed: true,
    });
  } catch (error) {
    await emit("failed", { step, issue: error instanceof Error ? error.message : "proof failed" });
  }
})();
