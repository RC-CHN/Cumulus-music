// Production search UI/IPC with official-shaped synthetic responses. No HTTP service calls.
(async () => {
  const one = (s) => document.querySelector(s),
    all = (s) => [...document.querySelectorAll(s)],
    sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const assert = (v, m) => {
    if (!v) throw Error(m);
  };
  const wait = async (f, m) => {
    for (let n = 0; n < 200; n++) {
      if (f()) return;
      await sleep(100);
    }
    throw Error(m);
  };
  const button = (label, root = document) =>
    [...root.querySelectorAll("button")].find(
      (n) =>
        n.getAttribute("aria-label") === label ||
        n.textContent.trim() === label,
    );
  const click = (n) => {
    assert(n && !n.disabled, "unavailable " + n?.outerHTML);
    n.focus({ preventScroll: true });
    n.click();
  };
  const input = () => one('[aria-label="搜索音乐"]'),
    panel = () => one(".search-panel");
  const emit = (stage, data = {}) =>
    window.__TAURI_INTERNALS__.invoke("plugin:event|emit", {
      event: "ux-proof",
      payload: { stage, ...data },
    });
  const key = (target, name, extra = {}) =>
    target.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: name,
        bubbles: true,
        cancelable: true,
        ...extra,
      }),
    );
  const type = (value) => {
    input().value = value;
    input().dispatchEvent(new Event("input", { bubbles: true }));
  };
  const hit = (n) => {
    const r = n.getBoundingClientRect(),
      at = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
    return at === n || n.contains(at);
  };
  let count = 0,
    gate,
    release,
    holdKind,
    held = false;
  const checks = [];
  const nativeFetch = window.fetch;
  window.fetch = async function (url, options) {
    const match = String(url).endsWith("/search_discovery");
    const query = match ? JSON.parse(options.body).query : undefined;
    const delay = query?.kind === holdKind ? gate : undefined;
    const response = await nativeFetch.call(window, url, options);
    if (delay) {
      held = true;
      await delay;
    }
    return response;
  };
  function hold(kind) {
    held = false;
    holdKind = kind;
    gate = new Promise((resolve) => {
      const timer = setTimeout(resolve, 10000);
      release = () => {
        clearTimeout(timer);
        gate = undefined;
        holdKind = undefined;
        resolve();
      };
    });
  }
  async function fixture(mode) {
    window.__searchFixtureAck = false;
    await emit("search-fixture", { mode });
    await wait(() => window.__searchFixtureAck, "fixture");
  }
  async function capture(label) {
    await wait(
      () =>
        all(".search-panel,.search-history-confirm").every((n) =>
          n
            .getAnimations()
            .every((a) => a.playState !== "running" && !a.pending),
        ),
      "animations",
    );
    await sleep(150);
    const tagLayout = all('.search-tags-section').map(section => {
      const list = section.querySelector('.search-tags');
      if (!list) return null;
      const history = section.classList.contains('search-history-tags');
      const visible = [...list.querySelectorAll('.search-tag:not([inert])')];
      const bounds = list.getBoundingClientRect();
      const toggle = list.querySelector('.search-expand');
      const folded = toggle?.getAttribute('aria-expanded') === 'false';
      const rowTops = new Set(visible.map(n => Math.round(n.getBoundingClientRect().top)));
      assert(visible.every(n => Math.abs(n.firstElementChild.getBoundingClientRect().height - 28) < 1), 'official 28px search tags');
      assert(parseFloat(getComputedStyle(list).rowGap) === (history ? 2 : 8), 'history/recommendation row spacing');
      assert(!folded || (rowTops.size <= 2 && toggle.getBoundingClientRect().bottom <= bounds.bottom + 1), 'inline folding control must remain within two rows');
      assert([...list.querySelectorAll('[inert]')].every(n => getComputedStyle(n).visibility === 'hidden'), 'folded labels must be hidden');
      return {history, visible: visible.length, rows: rowTops.size, height: bounds.height, folded};
    }).filter(Boolean);
    const p = panel(),
      r = p?.getBoundingClientRect(),
      d = one(".search-history-confirm"),
      dr = d?.getBoundingClientRect();
    const state = {
      label,
      noOverflow: document.documentElement.scrollWidth <= innerWidth,
      panelContained:
        !p ||
        (r.x >= 11 &&
          r.right <= innerWidth - 11 &&
          r.bottom <= innerHeight - 79),
      panelScroll: !p || p.scrollWidth <= p.clientWidth,
      modalContained:
        !d ||
        (dr.x >= 11 &&
          dr.right <= innerWidth - 11 &&
          dr.y >= 11 &&
          dr.bottom <= innerHeight - 11),
      theme: document.documentElement.dataset.theme,
    };
    assert(
      Object.entries(state).every(
        ([k, v]) => ["label", "theme"].includes(k) || v,
      ),
      JSON.stringify(state),
    );
    checks.push(state);
    await emit(`ux_${String(++count).padStart(2, "0")}_${label}`, {
      checks: state,
      width: innerWidth,
      height: innerHeight,
      searchPanelText: p?.textContent,
      tagLayout,
      focused: document.activeElement?.getAttribute("aria-label"),
    });
    await sleep(700);
  }
  async function theme(w, h, t) {
    await emit("resize", { width: w, height: h });
    await wait(() => innerWidth === w && innerHeight === h, "resize");
    click(button("外观主题"));
    await wait(() => one(".action-menu"), "appearance");
    click(button(t === "dark" ? "深色" : "浅色"));
    await wait(
      () =>
        !one(".action-menu") && document.documentElement.dataset.theme === t,
      "theme",
    );
  }
  async function home() {
    input().focus();
    input().click();
    if (input().value) click(button("清空搜索"));
    await wait(
      () => panel() && !panel().textContent.includes("正在加载"),
      "home",
    );
  }
  async function suggestions(q) {
    input().focus();
    input().click();
    type(q);
    await wait(
      () => panel()?.textContent.includes(q + " · 联想结果 1"),
      "suggestions",
    );
  }
  try {
    await wait(
      () => one(".track-row") && input()?.placeholder === "发现合成好声音",
      "startup",
    );
    for (const [w, h, t] of [
      [1120, 760, "light"],
      [760, 540, "light"],
      [760, 540, "dark"],
      [1120, 760, "dark"],
    ]) {
      await theme(w, h, t);
      await fixture("good");
      await home();
      await capture(`home_${w}_${t}`);
      const history = one(".search-tags-section");
      click(button("展开搜索历史", history));
      await capture(`history_expanded_${w}_${t}`);
      click(button("收起搜索历史", history));
      click(button("展开猜你喜欢"));
      await capture(`recommendations_expanded_${w}_${t}`);
      click(button("收起猜你喜欢"));
      await wait(() => button("展开猜你喜欢"), "recommendations collapsed before scrolling");
      panel().scrollTop = panel().scrollHeight;
      await sleep(100);
      assert(
        hit(all(".search-chart button").at(-1)),
        "last chart remains reachable",
      );
      await capture(`hot_scroll_${w}_${t}`);
      panel().scrollTop = 0;
      hold("suggestions");
      type("合成");
      await wait(
        () => held && panel().textContent.includes("正在查找搜索建议"),
        "loading suggestions",
      );
      await capture(`suggestions_loading_${w}_${t}`);
      release();
      await wait(
        () => panel().textContent.includes("合成 · 联想结果 1"),
        "suggestion loaded",
      );
      await capture(`suggestions_${w}_${t}`);
      key(input(), "ArrowDown");
      assert(
        document.activeElement?.hasAttribute("data-search-choice"),
        "keyboard enters suggestions",
      );
      key(document.activeElement, "ArrowDown");
      assert(
        document.activeElement.textContent.includes("推荐结果"),
        "keyboard moves to recommended result",
      );
      click(document.activeElement);
      await wait(
        () =>
          !panel() &&
          one(".search-heading-copy h1")?.textContent === "合成推荐",
        "search navigation",
      );
      await capture(`results_${w}_${t}`);
      await home();
      click(button("清空搜索历史"));
      await wait(
        () => one(".search-history-confirm[open]"),
        "clear confirmation",
      );
      await capture(`clear_confirm_${w}_${t}`);
      click(button("取消清空搜索历史"));
      await wait(() => !one(".search-history-confirm"), "cancel clear");
      assert(one(".search-tag"), "cancel preserves history");
      key(input(), "Escape");
      await wait(() => !panel(), "escape");
      assert(document.activeElement === input(), "Escape focus");
      await capture(`closed_${w}_${t}`);
    }
    await theme(760, 540, "dark");
    await home();
    const remove = one(".search-tag-remove");
    const removed = remove.getAttribute("aria-label");
    click(remove);
    await wait(() => !button(removed), "delete keyword");
    assert(
      document.activeElement?.hasAttribute("data-search-choice"),
      "delete returns focus to next keyword",
    );
    await capture("history_removed");
    click(button("清空搜索历史"));
    await wait(() => one(".search-history-confirm[open]"), "confirm");
    click(button("确认清空搜索历史"));
    await wait(
      () =>
        !one(".search-history-confirm") &&
        panel()?.textContent.includes("暂无搜索历史"),
      "history cleared",
    );
    await capture("history_empty");
    key(input(), "Escape");
    await fixture("home_error");
    await home();
    await wait(() => button("重试热搜榜"), "visible home failure");
    await capture("home_error");
    await fixture("good");
    click(button("重试热搜榜"));
    await wait(() => one(".search-chart"), "home recovery");
    click(button("重试推荐词"));
    await wait(() => !button("重试推荐词"), "recommendation recovery");
    await capture("home_recovered");
    await fixture("suggest_error");
    type("失败");
    await wait(() => button("重试搜索建议"), "suggest error");
    await capture("suggestion_error");
    await fixture("good");
    click(button("重试搜索建议"));
    await wait(
      () => panel().textContent.includes("失败 · 联想结果 1"),
      "suggest retry",
    );
    await capture("suggestion_recovered");
    await fixture("empty");
    click(button("清空搜索"));
    await wait(() => panel().textContent.includes("暂无热搜榜"), "empty home");
    await capture("home_empty");
    type("没有匹配");
    await wait(() => panel().textContent.includes("暂无建议"), "empty suggest");
    await capture("suggestions_empty");
    await fixture("good");
    hold("suggestions");
    type("旧词");
    await wait(() => held, "old response held");
    type("");
    release();
    await wait(() => one(".search-chart"), "home after old query");
    assert(
      !panel().textContent.includes("旧词 ·"),
      "late suggestion must not replace home",
    );
    await capture("late_query_ignored");
    input().dispatchEvent(
      new CompositionEvent("compositionstart", { bubbles: true }),
    );
    type("zhong");
    key(input(), "Enter", { isComposing: true });
    await sleep(250);
    assert(
      panel() && !panel().textContent.includes("联想结果"),
      "composition does not search",
    );
    assert(one('.search-exact')?.disabled && panel()?.textContent.includes('输入完成后显示搜索建议'), 'composition has a waiting-input state, not an active search');
    await capture("composing");
    type("中文");
    input().dispatchEvent(
      new CompositionEvent("compositionend", { bubbles: true, data: "中文" }),
    );
    await wait(
      () => panel().textContent.includes("中文 · 联想结果 1"),
      "composition settled",
    );
    await capture("composition_complete");
    hold("suggestions");
    type("关闭前");
    await wait(() => held, "pending close");
    key(input(), "Escape");
    release();
    await sleep(250);
    assert(
      !panel() && document.activeElement === input(),
      "late closed reply does not reopen",
    );
    await capture("closed_late_reply");
    await fixture("expired");
    input().click();
    type("失效");
    await wait(
      () => one(".welcome h1")?.textContent.includes("登录已失效") && !panel(),
      "expired login and finished search exit",
    );
    assert(
      !panel() && input().disabled,
      "expired account closes and disables search",
    );
    await capture("session_expired");
    await emit("complete", {
      ok: true,
      stageCount: count,
      checks,
      responseTimingInjected: true,
      realMusicRequests: 0,
      realAccountWrites: 0,
      officialPixelMatchVerified: false,
    });
  } catch (error) {
    release?.();
    await emit("ux_failed", {
      message: String(error),
      text: panel()?.textContent,
      focus: document.activeElement?.outerHTML.slice(0, 400),
    });
    await sleep(750);
    await emit("failed", {
      message: String(error),
      stack: error?.stack,
      stageCount: count,
    });
  } finally {
    window.fetch = nativeFetch;
  }
})();
