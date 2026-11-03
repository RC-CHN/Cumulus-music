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
    if (window.__cumulusSearchSuggestionMatchesProof) {
      await fixture("good");
      const provides = one("#app").__vue_app__._context.provides;
      const core = provides[Reflect.ownKeys(provides).find((k) =>
        typeof k === "symbol" && k.description === "core-client")];
      const read = core.searchDiscovery;
      const label = "İ A+B 和 a+b <plain>";
      const noMatch = "aab 不匹配";
      core.searchDiscovery = async (...args) => {
        const result = await read(...args);
        if (args[1].kind !== "suggestions") return result;
        return { ...result, value: { ...result.value, suggestions: [
          { keyword: "合成精确词", label, tag: null },
          { keyword: "合成另一个词", label: noMatch, tag: null },
        ] } };
      };
      try {
        input().focus();
        input().click();
        type("A+B");
        await wait(() => panel()?.textContent.includes(label), "synthetic suggestion labels");
        const row = one(".search-suggestion[title='" + label + "']");
        const unmatched = one(".search-suggestion[title='" + noMatch + "']");
        assert(row.textContent.trim() === label && !row.querySelector("plain"),
          "suggestion keeps its exact text without treating markup as HTML");
        const marks = [...row.querySelectorAll("mark.search-match-text")];
        assert(marks.map((mark) => mark.textContent).join("|") === "A+B|a+b",
          "both literal hits retain original offsets after İ");
        assert(unmatched && !unmatched.querySelector("mark"), "no empty or regex wildcard mark");
        assert(getComputedStyle(marks[0]).color === getComputedStyle(marks[1]).color,
          "both hits use the same color");
        await capture("suggestion_literal_matches");
        key(input(), "ArrowDown");
        assert(document.activeElement === row, "highlight markup leaves keyboard navigation intact");
        click(row);
        await wait(() => !panel() && one(".search-heading-copy h1")?.textContent === "合成精确词",
          "selection submits the canonical keyword");
        assert(input().value === label, "selected input preserves the display label");
      } finally {
        core.searchDiscovery = read;
      }
      await emit("complete", { ok: true, suggestionMatchesVerified: true, stageCount: count,
        realMusicRequests: 0, realAccountWrites: 0 });
      return;
    }
    if (window.__cumulusSearchExactFallbackProof) {
      await fixture("good");
      await suggestions("保留原词");
      assert(!one(".search-exact"), "ordinary suggestions replace exact fallback");
      assert(panel().textContent.includes("保留原词 · 推荐结果"), "recommendations remain");
      await capture("exact_fallback_hidden");
      key(input(), "Enter");
      await wait(() => !panel() && one(".search-heading-copy h1")?.textContent === "保留原词",
        "raw Enter still submits the exact term");

      const provides = one("#app").__vue_app__._context.provides;
      const core = provides[Reflect.ownKeys(provides).find((k) =>
        typeof k === "symbol" && k.description === "core-client")];
      const read = core.searchDiscovery;
      core.searchDiscovery = async (...args) => {
        const result = await read(...args);
        if (args[1].kind !== "suggestions") return result;
        // Synthetic native response is unchanged apart from its empty suggestions list.
        return { ...result, value: { ...result.value, suggestions: [] } };
      };
      try {
        await home();
        type("仅有推荐");
        await wait(() => panel()?.textContent.includes("仅有推荐 · 推荐结果"),
          "recommendation-only settled reply");
        assert(one(".search-exact")?.textContent.trim() === "搜索“仅有推荐”" &&
          !panel().textContent.includes("联想结果"), "empty suggestion group keeps exact fallback");
        await capture("exact_fallback_with_recommendation");
        click(one(".search-exact"));
        await wait(() => !panel() && one(".search-heading-copy h1")?.textContent === "仅有推荐",
          "exact fallback submits displayed text");
      } finally {
        core.searchDiscovery = read;
      }

      await fixture("empty");
      await home();
      type("没有匹配");
      await wait(() => panel()?.textContent.includes("暂无建议"), "fully empty response");
      assert(one(".search-exact")?.textContent.trim() === "搜索“没有匹配”",
        "empty response retains exact fallback");
      await capture("exact_fallback_empty");
      await emit("complete", { ok: true, exactFallbackVerified: true, stageCount: count,
        realMusicRequests: 0, realAccountWrites: 0 });
      return;
    }
    for (const [w, h, t] of [
      [1120, 760, "light"],
      [760, 540, "light"],
      [760, 540, "dark"],
      [1120, 760, "dark"],
    ]) {
      await theme(w, h, t);
      await fixture("good");
      await home();
      await wait(() => one(".search-chart-items button"), "rank layout loaded");
      const rankCells = [...one(".search-chart-items").querySelectorAll("button")];
      assert(rankCells.length === 9, "first nine ranked results");
      const byPosition = [...rankCells].sort((left, right) => {
        const a = left.getBoundingClientRect(), b = right.getBoundingClientRect();
        return Math.abs(a.top - b.top) > 1 ? a.top - b.top : a.left - b.left;
      });
      assert(
        byPosition.map((cell) => Number(cell.querySelector(".search-rank").textContent)).join(",") ===
          (w === 1120 ? "1,4,7,2,5,8,3,6,9" : "1,2,3,4,5,6,7,8,9"),
        "official rank positions at normal width and linear order in narrow windows",
      );
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
        document.activeElement.textContent.includes("联想结果 1"),
        "Down from input enters the first ordinary suggestion",
      );
      key(document.activeElement, "ArrowUp");
      assert(
        document.activeElement.textContent.includes("推荐结果"),
        "Up from first ordinary suggestion wraps to last recommendation",
      );
      key(document.activeElement, "ArrowDown");
      assert(
        document.activeElement.textContent.includes("联想结果 1"),
        "Down from last recommendation wraps to first suggestion",
      );
      key(document.activeElement, "Escape");
      await wait(() => !panel(), "escape from suggestion");
      assert(document.activeElement === input(), "Escape restores input focus");
      input().click();
      await wait(() => panel()?.textContent.includes("合成 · 联想结果 1"), "reopened suggestions");
      key(input(), "ArrowUp");
      assert(
        document.activeElement.textContent.includes("推荐结果"),
        "Up from input enters the last recommendation",
      );
      click(document.activeElement);
      await wait(
        () =>
          !panel() &&
          one(".search-heading-copy h1")?.textContent === "合成推荐",
        "search navigation",
      );
      assert(input().value.includes("推荐结果"), "suggestion label stays in input, not the search term");
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
    await fixture("hot_source");
    await home();
    await wait(() => one(".search-chart-items button"), "hot source chart row");
    click(one(".search-chart-items button"));
    const ready = (word, category, item) =>
      one(".search-results-page h1")?.textContent === word &&
      one('[data-category="' + category + '"]')?.getAttribute("aria-selected") === "true" &&
      one("#search-result-panel")?.getAttribute("aria-busy") === "false" &&
      !one(".search-results-leave-active") && one(item);
    await wait(() => ready("合成热搜来源", "all", ".search-track-card"), "hot comprehensive source");
    assert(input().value === "合成榜单条目", "chart label stays in input");
    click(one('[data-category="songs"]'));
    await wait(() => ready("合成热搜来源", "songs", ".track-row"), "hot songs source");
    click(one('[data-category="mvs"]'));
    await wait(() => ready("合成热搜来源", "mvs", ".search-result-card"), "hot MV source");
    click(button("返回上一页"));
    await wait(() => ready("合成热搜来源", "songs", ".track-row"), "hot search Back");
    assert(input().value === "合成榜单条目", "chart label stays after category changes and Back");
    await home();
    await wait(() => all(".search-chart-items button").length >= 2, "unmarked source chart row");
    click(all(".search-chart-items button")[1]);
    await wait(() => ready("合成普通来源", "all", ".search-track-card"), "unmarked source defaults");
    await fixture("good");
    await home();
    const homeRecommendation = one(".search-tags-section:not(.search-history-tags) button[data-search-choice]");
    assert(homeRecommendation, "home recommendation is available");
    const recommendedLabel = homeRecommendation.getAttribute("title");
    click(homeRecommendation);
    await wait(() => ready("合成推荐词1", "all", ".search-track-card"), "home recommendation uses actual term");
    assert(input().value === recommendedLabel, "home recommendation keeps its label");
    await fixture("chart_play");
    await home();
    await wait(() => button("播放热搜榜") && !button("播放热搜榜").disabled, "conditional hot chart play");
    assert(!button("播放摇滚榜") && !button("播放ACG榜"), "only the advertised chart has Play");
    click(button("播放热搜榜"));
    await wait(() => !panel() && one(".playerbar")?.dataset.state === "playing" &&
      one(".now-playing")?.textContent.includes("178"), "chart play selects first returned song");
    assert(document.activeElement === input(), "chart play returns search focus");
    await fixture("audio_ranks");
    await home();
    const row=(label)=>one(`.search-chart-items button[title^="${label}"]`);
    await wait(()=>row("合成播客榜条目")&&row("合成听书榜条目"),"both audio ranks ready");
    for(const name of ["外链不可打开","无目标不可打开","重复编号不可打开"])
      assert(row(name)?.disabled&&row(name).textContent.includes("暂不可打开"),"unsafe audio row disabled: "+name);
    assert(!button("播放播客榜")&&!button("播放听书榜"),"audio ranks never send song-play commands");
    click(row("合成播客榜条目"));
    await wait(()=>!panel()&&one(".podcast-cover h2")?.textContent==="合成播客 301","podcast rank opens actual detail");
    assert(!input().value && one(".now-playing")?.textContent.includes("178"), "podcast rank neither submits a keyword nor changes playback");
    await capture("podcast_rank_detail");
    await home();
    click(row("合成播客榜条目"));
    await wait(()=>!panel(),"same podcast closes search");
    assert(one(".podcast-cover h2")?.textContent==="合成播客 301", "same target retains the current detail");
    click(button("返回上一页"));
    await wait(()=>ready("合成推荐词1", "all", ".search-track-card"),"audio rank Back restores search");
    await home();
    click(row("合成听书榜条目"));
    await wait(()=>!panel()&&one(".podcast-cover h2")?.textContent==="合成播客 302","audiobook rank opens its own detail");
    await capture("audiobook_rank_detail");
    click(button("返回上一页"));
    await wait(()=>ready("合成推荐词1", "all", ".search-track-card"),"audiobook Back restores search");
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
      conditionalChartPlayVerified: true,
      hotqueryChannelVerified: true,
      searchDisplayLabelVerified: true,
      cyclicSuggestionsVerified: true,
      audioRankNavigationVerified: true,
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
