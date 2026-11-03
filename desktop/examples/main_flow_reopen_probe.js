// The reopened process must restore on startup without a scripted login or refresh.
(async () => {
  if (window.__mainFlowReopenStarted) return;
  window.__mainFlowReopenStarted = true;
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const emit = (stage, payload = {}) => invoke("plugin:event|emit", {
    event: "ux-proof", payload: { stage, ...payload },
  });
  const all = (selector, root = document) => [...(root?.querySelectorAll(selector) ?? [])]
    .filter(node => !node.closest("[inert],.page-leave-active,.modal-leave-active") &&
      node.getClientRects().length);
  async function wait(test, label) {
    for (let i = 0; i < 700; i++) {
      const value = test();
      if (value) return value;
      await new Promise(resolve => setTimeout(resolve, 30));
    }
    throw Error(label);
  }

  let last = "automatic_restore";
  try {
    const nickname = "合成主流程账号";
    await wait(() => {
      const entry = all(".account-button")[0];
      return entry?.title === nickname && entry.textContent.trim() === nickname &&
        !entry.disabled && !all(".login-dialog[open]").length;
    }, "authenticated header after automatic restore");
    const account = await invoke("account_snapshot");
    if (account.status !== "authenticated" || account.nickname !== nickname ||
        typeof account.epoch !== "string" || !account.epoch) {
      throw Error("native snapshot does not match the restored header");
    }

    last = "owned_baseline";
    const library = await wait(() => all("button").find(node =>
      (node.getAttribute("aria-label") === "我的歌单" || node.textContent.trim() === "我的歌单") &&
      !node.disabled && node.getAttribute("aria-disabled") !== "true"),
    "playlist library entry");
    library.focus({ preventScroll: true });
    library.click();
    const baselineCount = await wait(() => {
      const page = all(".playlist-library-page")[0];
      if (!page) return false;
      const created = all('[data-group="created"] [data-library-open]', page);
      const allCards = all("[data-library-open]", page);
      return created.length === 1 && allCards.length === 1 &&
        created[0].getAttribute("aria-label") === "合成基线歌单" &&
        created[0].querySelector(".card-title")?.textContent.trim() === "合成基线歌单" &&
        all('[data-group="created"] h2 small', page)[0]?.textContent.trim() === "1" &&
        all('[data-group="subscribed"] h2 small', page)[0]?.textContent.trim() === "0" &&
        created.length;
    }, "only the baseline is visible in owned playlists");

    last = "reopened_paint";
    await new Promise((resolve, reject) => {
      const timeout = setTimeout(() => reject(Error("reopened paint did not advance")), 4000);
      requestAnimationFrame(() => requestAnimationFrame(() => {
        clearTimeout(timeout);
        resolve();
      }));
    });
    await wait(() => !document.getAnimations().some(animation =>
      Number.isFinite(animation.effect?.getComputedTiming().endTime) &&
      (animation.pending || animation.playState === "running")), "settled reopen paint");
    const current = await invoke("account_snapshot");
    if (current.epoch !== account.epoch || current.status !== "authenticated") {
      throw Error("account changed during reopened library proof");
    }
    await emit("ux_reopened", {
      ok: true, epoch: current.epoch, status: current.status, baselineCount, baselineOwned: true,
    });
  } catch (error) {
    await emit("failed", { ok: false, last, reason: String(error?.message ?? error) });
  }
})();
