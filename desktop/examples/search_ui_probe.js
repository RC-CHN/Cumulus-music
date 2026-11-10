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
  const checks = [], activationReads = [];
  let activationSubmissions = 0;
  const nativeFetch = window.fetch;
  window.fetch = async function (url, options) {
    const match = String(url).endsWith("/search_discovery");
    const query = match ? JSON.parse(options.body).query : undefined;
    if (window.__cumulusSearchFocusActivationProof) {
      if (query && query.kind !== "defaults") activationReads.push(query);
      if (String(url).endsWith("/search_results")) activationSubmissions++;
    }
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
  async function capture(label, extra = {}) {
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
      if (toggle) {
        const buttonBounds = toggle.getBoundingClientRect();
        const glyph = toggle.querySelector('svg').getBoundingClientRect();
        const expected = history ? 16 : 12;
        assert(Math.abs(buttonBounds.width - 24) < 1 && Math.abs(buttonBounds.height - 24) < 1,
          'expand control keeps the 24px target');
        assert(Math.abs(glyph.width - expected) < 1 && Math.abs(glyph.height - expected) < 1,
          'history and recommendation expand glyphs use their own official sizes');
      }

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
      ...extra,
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
    if (window.__cumulusGuestSearchProof) {
      const keyword = "合成游客查询";
      const rows = () => all(".guest-search-result");
      const heading = () => one(".search-results-page h1")?.textContent.trim();
      const comprehensiveRows = () => one('[data-result-section^="search_block_song:"] .guest-search-result');
      await wait(
        () => input()?.placeholder === "发现合成好声音" && !input().disabled && document.hasFocus(),
        "native registered guest enables topbar search",
      );
      assert(!one(".search-results-page"), "guest registration does not navigate");
      assert(document.body.textContent.includes("可以直接在顶栏搜索"), "welcome acknowledges guest access");
      await capture("guest_search_ready");
      await suggestions(keyword);
      await capture("guest_search_suggestions");
      key(input(), "Enter");
      await wait(() => heading() === keyword && rows().length, "guest comprehensive results");
      assert(button("登录后播放和收藏"), "sign-in available without blocking the results");
      assert(!one(".search-section-play, .search-track-card, .search-result-card, .search-result-action-menu"), "no duplicate signed card or guest playback authority");
      await capture("guest_search_results");
      click(one('[data-category="songs"]'));
      await wait(
        () => one('[data-category="songs"][aria-selected="true"]') && rows().length &&
          one("[data-result-section='单曲']"),
        "guest song category",
      );
      assert(!button("播放全部") && !button("收藏全部"), "account controls are not emitted by guest rows");
      await capture("guest_search_songs");
      click(button("返回上一页"));
      await wait(
        () => heading() === keyword && one('[data-category="all"][aria-selected="true"]') && comprehensiveRows(),
        "Back to guest comprehensive results",
      );
      await capture("guest_search_back");
      input().focus();
      type("");
      key(input(), "Enter");
      await wait(() => heading() === "合成默认关键词" && comprehensiveRows(), "guest default context result");
      assert(!document.body.textContent.includes("synthetic-default-context-native-only"));
      await capture("guest_search_default");
      await emit("complete", { ok: true, stageCount: count, guestSearchVerified: true,
        realMusicRequests: 0, realAccountWrites: 0 });
      return;
    }
    if (window.__cumulusSearchDefaultRecoveryProof) {
      const missing = "搜索音乐、寻找好声音";
      await wait(
        () => one(".track-row") && input()?.placeholder === missing && document.hasFocus(),
        "signed-in search with an empty initial default response",
      );
      await capture("default_recovery_initial");
      input().focus({ preventScroll: true });
      await wait(() => !panel() && document.activeElement === input(), "focus does not open search");
      await sleep(5200);
      assert(input().placeholder === missing, "focused input did not retry after the popover closed");
      await capture("default_recovery_input_focused");

      button("开始搜索").focus({ preventScroll: true });
      assert(document.activeElement !== input(), "search input is blurred");
      // Inject only the notification; the DOM focus, IPC and service fixture
      // still exercise the production WebView. This is not a physical WM event.
      window.dispatchEvent(new Event("blur"));
      await sleep(5200);
      assert(input().placeholder === missing, "unfocused window did not retry");
      await capture("default_recovery_window_blurred");
      window.dispatchEvent(new Event("focus"));
      await wait(() => input().placeholder === "合成默认第一词", "default tick recovered");
      await capture("default_recovery_populated");
      await wait(() => input().placeholder === "合成默认第二词", "rotates without another default read");
      input().focus({ preventScroll: true });
      key(input(), "Enter");
      const heading = () => one(".search-results-page h1")?.textContent.trim();
      await wait(
        () => heading() === "合成恢复第二词" && one(".search-result-section"),
        "blank Enter submits the real recovered second word",
      );
      assert(!document.body.textContent.includes("synthetic-recovered-second"), "raw context remains native");
      await capture("default_recovery_submitted");
      type("合成恢复第二词");
      key(input(), "Enter");
      await wait(
        () => heading() === "合成恢复第二词" && one(".search-result-section"),
        "typed search for the same keyword",
      );
      await capture("default_recovery_typed");
      await emit("complete", { ok: true, stageCount: count, defaultRecoveryVerified: true,
        windowFocusNotificationSynthetic: true, realMusicRequests: 0, realAccountWrites: 0 });
      return;
    }
    await wait(
      () => one(".track-row") && input()?.placeholder === "发现合成好声音",
      "startup",
    );
    if (window.__cumulusSearchFocusActivationProof) {
      const typed = "合成键入激活", committed = "合成输入法激活";
      const expanded = () => input().getAttribute("aria-expanded") === "true";
      const focus = () => input().focus({ preventScroll: true });
      const compose = (phase, data) => input().dispatchEvent(
        new CompositionEvent(`composition${phase}`, { bubbles: true, data }),
      );
      const chartIds = ["HOT_SEARCH_SONG#@#", "HOT_SEARCH_STYLE_ROCK#@#", "HOT_SEARCH_STYLE_ACG#@#"];
      const state = async (label, expected, visible) => {
        await wait(() => expanded() === visible && !!panel() === visible, label);
        await sleep(350); // Observe beyond the production suggestion debounce.
        const reads = activationReads.map(({ kind, keyword, id }) => [kind, keyword ?? id ?? null]);
        const primary = reads.filter(([kind]) => kind !== "chart");
        const details = reads.filter(([kind]) => kind === "chart").map(([, id]) => id);
        const expectedDetails = expected.flatMap(([kind]) => kind === "charts" ? chartIds : []);
        assert(JSON.stringify(primary) === JSON.stringify(expected), `${label}: ${JSON.stringify(primary)}`);
        assert(JSON.stringify(details) === JSON.stringify(expectedDetails), `${label}: ${JSON.stringify(details)}`);
        assert(document.activeElement === input(), `${label}: input must retain focus`);
        assert(!one(".search-results-page") && activationSubmissions === 0, `${label}: no search submission`);
        await capture(label, { activationReads: reads, expanded: visible });
      };
      const discovery = [["charts", null], ["recommendations", null]];
      const typedReads = [["suggestions", typed]];
      const committedReads = [...typedReads, ["suggestions", committed]];
      const awaitHome = () => wait(
        () => panel() && one(".search-chart") && !panel().textContent.includes("正在加载"),
        "discovery content",
      );
      button("开始搜索").focus({ preventScroll: true });
      const inactiveBackground = getComputedStyle(input().form).backgroundImage;
      focus();
      assert(getComputedStyle(input().form).backgroundImage !== inactiveBackground,
        "focus still has a visible selected background");
      await state("focus_only", [], false);

      type(typed);
      await wait(() => panel()?.textContent.includes(typed + " · 联想结果 1"), "typing opens suggestions");
      await state("edit_opens_suggestions", typedReads, true);

      key(input(), "Escape");
      await wait(() => !panel(), "Escape removes the panel");
      button("开始搜索").focus({ preventScroll: true });
      focus();
      assert(input().value === typed, "refocus retains the entered text");
      await state("refocus_only", typedReads, false);

      compose("start", "");
      type("yin");
      key(input(), "Enter", { isComposing: true });
      await state("composing_closed", typedReads, false);
      type(committed);
      compose("end", committed);
      await wait(() => panel()?.textContent.includes(committed + " · 联想结果 1"), "commit opens suggestions");
      await state("composition_commit", committedReads, true);

      key(input(), "Escape");
      type("");
      await awaitHome();
      await state("empty_edit_opens_discovery", [...committedReads, ...discovery], true);
      key(input(), "Escape");
      await wait(() => !panel(), "close before keyboard open");
      key(input(), "ArrowDown");
      await awaitHome();
      await state("arrow_opens_discovery", [...committedReads, ...discovery, ...discovery], true);
      key(input(), "Escape");
      await wait(() => !panel(), "close before pointer open");
      input().click();
      await awaitHome();
      await state("click_opens_discovery", [...committedReads, ...discovery, ...discovery, ...discovery], true);
      await emit("complete", { ok: true, stageCount: count, searchFocusActivationVerified: true,
        activationSubmissions, realMusicRequests: 0, realAccountWrites: 0 });
      return;
    }
    if (window.__cumulusPodcastBrowseAroundProof) {
      await fixture("audio_ranks");
      const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
      const { epoch } = await invoke("account_snapshot");
      const snapshot = () => invoke("player_snapshot", { epoch });
      const queue = async () => (await invoke("player_queue", { epoch })).tracks.map(row => row.track.voiceId);
      const waitPlayer = (state, id) => wait(() =>
        one(".playerbar")?.dataset.state === state && one(".now-playing")?.textContent.includes(id),
        state + " " + id);
      const seed = await invoke("play_selection", { epoch, selection: { kind: "voice", id: "401" } });
      assert(seed.track?.voiceId === "401", "real retained baseline episode");
      await waitPlayer("playing", "401");
      await home();
      await wait(() => button("播客榜：更多"), "podcast discovery navigation");
      click(button("播客榜：更多"));
      const preview = () => one('.podcast-top[data-layout="updated"] [data-preview="podcast"]');
      const weekly = () => one('[data-home-voice="404"] button');
      const action = (label, scope) => scope && button(label, scope);
      const row = (prefix, id, scope) => action(`${prefix} 合成声音 ${id}`, scope);
      await wait(() => preview() && action("最热播客", preview()) && weekly(), "visible rank and weekly cards");
      click(action("最热播客", preview()));
      await wait(() => row("播放", 407, preview()) && !row("播放", 407, preview()).disabled, "ready rank preview");
      click(row("播放", 407, preview()));
      await waitPlayer("playing", "407");
      const previewPlayed = await snapshot();
      assert(previewPlayed.track?.voiceId === "407" && previewPlayed.playId !== seed.playId &&
        (await queue()).join() === "401,409,407,410", "around retains old row and selects clicked preview");
      await capture("around_preview");
      click(row("暂停", 407, preview()));
      await waitPlayer("paused", "407");
      const previewPaused = await snapshot();
      click(row("播放", 407, preview()));
      await waitPlayer("playing", "407");
      assert(previewPaused.playId === previewPlayed.playId && (await snapshot()).playId === previewPlayed.playId &&
        (await snapshot()).queueRevision === previewPlayed.queueRevision, "preview toggles the same decoder and queue");
      weekly().scrollIntoView({ block: "center" });
      click(weekly());
      await waitPlayer("playing", "404");
      const weeklyPlayed = await snapshot();
      assert(weeklyPlayed.track?.voiceId === "404" &&
        (await queue()).join() === "401,409,407,411,404,412,410", "weekly normalizes strict order and inserts after current");
      await capture("around_weekly");
      click(action("查看全部", preview()));
      const full = () => one(".podcast-rankings");
      await wait(() => row("播放", 407, full()) && !row("播放", 407, full()).disabled, "actual full rank");
      click(row("播放", 407, full()));
      await waitPlayer("playing", "407");
      const fullPlayed = await snapshot();
      assert(fullPlayed.track?.voiceId === "407" && fullPlayed.playId !== weeklyPlayed.playId &&
        (await queue()).join() === "401,411,404,409,407,410,412", "full-rank action moves duplicate rows as an ordered window");
      await capture("around_full_rank");
      await fixture("podcast_home_disabled");
      click(action("音乐声音", full()));
      await wait(() => row("播放", 408, full()) && !row("播放", 408, full()).disabled, "actual category chart");
      await invoke("player_action", { epoch, action: { kind: "pause", paused: true } });
      await waitPlayer("paused", "407");
      click(row("播放", 408, full()));
      await wait(() => document.body.textContent.includes("无法添加播放"), "successful missing list is explicit");
      const still = await snapshot();
      assert(still.playId === fullPlayed.playId && still.entryId === fullPlayed.entryId &&
        still.queueRevision === fullPlayed.queueRevision && still.state === "paused", "empty result leaves live decoder and queue");
      await capture("around_empty");
      click(action("今日最热", full()));
      const title = (id) => row("播放", id, full())?.closest("li")?.querySelector(".rank-episode strong");
      const double = async (id) => {
        await wait(() => title(id) && !row("播放", id, full()).disabled, "enabled voice rank row " + id);
        const node = title(id);
        assert(node?.isConnected, "voice rank row was replaced before double click " + id);
        node.dispatchEvent(new MouseEvent("dblclick", { bubbles: true, detail: 2 }));
      };
      await double(410);
      await waitPlayer("playing", "410");
      const replaced = await snapshot();
      assert(replaced.mode === "sequential" && (await queue()).join() === "407,410,409",
        "row double click replaces with the native chart order");
      await capture("rank_double_replace");
      const currentRow = row("暂停", 410, full()).closest("li");
      currentRow.focus();
      currentRow.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
      await waitPlayer("paused", "410");
      const toggled = await snapshot();
      assert(toggled.playId === replaced.playId && toggled.entryId === replaced.entryId &&
        toggled.queueRevision === replaced.queueRevision, "focused row toggles without replacing the loaded episode");
      await capture("rank_double_same");
      const preference = await invoke("queue_click_preferences", { epoch });
      await invoke("queue_click_preferences_command", { epoch, command: {
        kind: "change", expected_revision: preference.value.revision, replace_queue: false,
      } });
      await invoke("play_selection", { epoch, selection: { kind: "voice", id: "401" } });
      await waitPlayer("playing", "401");
      await double(407);
      await waitPlayer("playing", "407");
      assert((await queue()).join() === "401,409,407,410", "replace-off double click uses the surrounding episode order");
      await capture("rank_double_keep");
      await emit("complete", { ok: true, stageCount: count, podcastBrowseAroundVerified: true,
        rankDoubleClickVerified: true,
        realMusicRequests: 0, realAccountWrites: 0 });
      return;
    }
    if (window.__cumulusPodcastTopClassicProof) {
      await fixture("podcast_top_classic");
      await home();
      await wait(() => button("播客榜：更多"), "classic discovery entry");
      click(button("播客榜：更多"));
      const top = () => one('.podcast-top[data-layout="classic"]');
      const regular = () => top()?.querySelector('[data-preview="podcast"]');
      await wait(() => regular()?.querySelectorAll('.podcast-top-item').length === 8, "failed assignment falls back to classic scores");
      await wait(() => top()?.querySelector('[data-podcast-scene-fm] [data-scene-id="OP_1"]'), "fallback classic reads the inline Scene FM card");
      assert(!top().querySelector('[data-preview="book"]') && one(".playerbar")?.dataset.state === "idle", "fallback Classic displays the Scene FM directory without automatic playback or an updated audiobook card");
      await capture("podcast_top_classic_scores");
      click(button("今日最热", regular()));
      await wait(() => button("播放 合成声音 407", regular()), "legacy hot tab uses the voice preview endpoint");
      await capture("podcast_top_classic_hot");
      click(button("查看全部", regular()));
      await wait(() => one('.podcast-rankings')?.textContent.includes('合成声音 407') && one('.podcast-rankings [aria-pressed="true"]')?.textContent.trim() === '今日最热', "classic link selects the corresponding full chart");
      click(button("返回上一页"));
      await wait(() => regular()?.dataset.activeChart === 'hot', "classic detail Back preserves the chosen preview");
      await capture("podcast_top_classic_back");
      await emit("complete", { ok: true, stageCount: count, podcastTopClassicVerified: true, realMusicRequests: 0, realAccountWrites: 0 });
      return;
    }
    if (window.__cumulusSearchHistoryLastProof) {
      await fixture("good");
      await home();
      click(button("清空搜索历史"));
      await wait(() => one(".search-history-confirm[open]"), "initial history confirmation");
      click(button("确认清空搜索历史"));
      await wait(() => !one(".search-history-confirm") && panel()?.textContent.includes("暂无搜索历史"),
        "seed history cleared");
      async function remember(word) {
        await suggestions(word);
        key(input(), "Enter");
        await wait(() => !panel() && one(".search-heading-copy h1")?.textContent === word,
          "search remembered");
        await home();
        await wait(() => button("删除搜索历史：" + word), "remembered tag visible");
      }
      await remember("第一条");
      await remember("第二条");
      click(button("删除搜索历史：第二条"));
      await wait(() => panel() && !button("删除搜索历史：第二条"), "non-last deletion stays open");
      assert(button("删除搜索历史：第一条"), "remaining tag stays available");
      await capture("history_non_last_keeps_popup");
      click(button("删除搜索历史：第一条"));
      await wait(() => !panel(), "final deletion closes the popup");
      assert(document.activeElement === input(), "last deletion returns owned focus to input");
      await capture("history_last_dismisses_popup");

      await remember("迟到删除");
      const provides = one("#app").__vue_app__._context.provides;
      const core = provides[Reflect.ownKeys(provides).find((k) =>
        typeof k === "symbol" && k.description === "core-client")];
      const read = core.searchHistory;
      let finish;
      core.searchHistory = async (...args) => {
        const result = await read(...args);
        if (args[1].kind === "remove") await new Promise((resolve) => { finish = resolve; });
        return result;
      };
      try {
        click(button("删除搜索历史：迟到删除"));
        await wait(() => finish, "native delete succeeded while receipt is held");
        await suggestions("保留输入");
        finish();
        await sleep(400);
        assert(panel()?.textContent.includes("保留输入 · 联想结果 1") &&
          document.activeElement === input(), "late removal does not dismiss later typing");
        await capture("history_late_delete_keeps_suggestions");
      } finally {
        core.searchHistory = read;
        finish?.();
      }
      await emit("complete", { ok: true, historyLastVerified: true, stageCount: count,
        realMusicRequests: 0, realAccountWrites: 0 });
      return;
    }
    if (window.__cumulusSearchSuggestionMatchesProof) {
      await fixture("suggestion_matches");
      const row = (group, index = 0) =>
        panel()?.querySelector(`[data-suggestion-key="${group}:${index}"]`);
      const point = (node) =>
        node.dispatchEvent(new PointerEvent("pointerenter", { pointerType: "mouse" }));
      const marks = (node) => [...node.querySelectorAll("mark.search-match-text")];
      const verifyRow = (node, label, expected) => {
        assert(node?.title === label && node.textContent.trim() === label &&
          node.firstElementChild.textContent === label && !node.querySelector("plain"),
          "typed segments keep the exact label as text: " + label);
        const actual = marks(node).map((mark) => mark.textContent);
        assert(JSON.stringify(actual) === JSON.stringify(expected),
          `wrong marks for ${label}: ${JSON.stringify(actual)}`);
      };
      const open = async (query, ordinary) => {
        input().focus();
        input().click();
        type(query);
        await wait(() => row("suggestions")?.title === ordinary, "native suggestion for " + query);
      };
      const result = async (keyword) => {
        await wait(() => !panel() &&
          one(".search-heading-copy h1")?.textContent === keyword &&
          one("#search-result-panel")?.getAttribute("aria-busy") === "false" &&
          !one(".search-results-leave-active") && one(".search-track-card"),
          "native result for " + keyword);
      };

      const literal = "İ A+B 和 a+b <plain>";
      await open("A+B", literal);
      const first = row("suggestions");
      const unmatched = row("suggestions", 1);
      verifyRow(first, literal, ["A+B", "a+b"]);
      verifyRow(unmatched, "aab 不匹配", []);
      verifyRow(row("recommendations"), "推荐 a+b", ["a+b"]);
      const [upper, lower] = marks(first);
      assert(getComputedStyle(upper).color === getComputedStyle(lower).color,
        "both literal hits use the same color");
      await capture("suggestion_literal_matches");
      assert(hit(first) && hit(unmatched), "both suggestion rows accept pointer input");
      point(first);
      await wait(() => first.classList.contains("search-suggestion-pointed"), "pointer activation is visible");
      assert(document.activeElement === input(), "pointer does not steal text focus");
      key(input(), "ArrowDown");
      assert(document.activeElement === unmatched, "arrow navigation advances from the pointer's active row");
      input().focus();
      point(first);
      await wait(() => first.classList.contains("search-suggestion-pointed"), "hover before Enter");
      await capture("suggestion_pointer_active");
      key(input(), "Enter");
      await result("合成精确词");
      assert(input().value === "合成输入栏文案", "hover selection uses toUserWord in the input");
      await capture("suggestion_pointer_selection", {
        selection: { popup: literal, result: one(".search-heading-copy h1").textContent, input: input().value },
      });

      await open("音樂", "原字形 音樂 / 音乐");
      verifyRow(row("suggestions"), "原字形 音樂 / 音乐", ["音乐"]);
      verifyRow(row("recommendations"), "推荐 音乐与音樂", ["音乐"]);
      await capture("suggestion_traditional_query");

      await open("音乐", "反向 音樂");
      verifyRow(row("suggestions"), "反向 音樂", []);
      verifyRow(row("recommendations"), "推薦 音樂之聲", []);
      await capture("suggestion_simplified_query_not_reversed");

      await open("a b c", "A / B C / B / C / a b c");
      verifyRow(row("suggestions"), "A / B C / B / C / a b c", ["A", "B C", "a", "b c"]);
      const recommendation = row("recommendations");
      verifyRow(recommendation, "B / C / A / b c", ["A", "b c"]);
      await capture("suggestion_first_space");
      const ordinary = row("suggestions");
      ordinary.focus({ preventScroll: true });
      key(ordinary, "ArrowDown");
      assert(document.activeElement === recommendation, "keyboard advances to the recommendation");
      key(recommendation, "Enter");
      await result("合成分支推荐");
      assert(input().value === "合成分支推荐",
        "recommended keyboard selection without toUserWord fills the actual keyword");
      await capture("suggestion_recommendation_selection", {
        selection: { popup: "B / C / A / b c", result: one(".search-heading-copy h1").textContent, input: input().value },
      });

      await open("退出悬停", "退出候选");
      point(row("suggestions"));
      one(".search-suggestions").dispatchEvent(new PointerEvent("pointerleave", { pointerType: "mouse" }));
      key(input(), "Enter");
      await result("退出悬停");
      await open("第一词", "过期候选");
      point(row("suggestions"));
      type("更改后的原词");
      key(input(), "Enter");
      await result("更改后的原词");
      const invoke = (name, args = {}) => window.__TAURI_INTERNALS__.invoke(name, args);
      const { epoch } = await invoke("account_snapshot");
      let history;
      for (let n = 0; n < 50; n++) {
        history = (await invoke("search_history", { epoch, command: { kind: "read" } })).value.slice(0, 4);
        if (history[0] === "更改后的原词") break;
        await sleep(25);
      }
      assert(history.join("|") === "更改后的原词|退出悬停|合成分支推荐|合成精确词",
        "native history remembers only the selected search keywords");
      await emit("complete", {
        ok: true,
        suggestionMatchesVerified: true,
        suggestionPointerVerified: true,
        suggestionGlyphDirectionVerified: true,
        suggestionFirstSpaceVerified: true,
        suggestionRecommendationKeyboardVerified: true,
        suggestionHistory: history,
        stageCount: count,
        realMusicRequests: 0,
        realAccountWrites: 0,
      });
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
      assert(input().value === "合成推荐", "missing toUserWord leaves the actual keyword in the input");
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
    const compose = (kind, data) => input().dispatchEvent(
      new CompositionEvent(`composition${kind}`, { bubbles: true, data }),
    );
    const completedSearch = (word) =>
      one(".search-results-page h1")?.textContent === word &&
      one("#search-result-panel")?.getAttribute("aria-busy") === "false" && !panel();
    compose("start", "");
    type("lianxu");
    await wait(() => one(".search-exact")?.disabled, "partial composition is visible");
    key(input(), "Enter", { isComposing: true });
    input().form.requestSubmit();
    await Promise.resolve();
    assert(input().getAttribute("aria-expanded") === "true", "partially composed term is not submitted");
    type("连续回车");
    compose("end", "连续回车");
    key(input(), "Enter", { keyCode: 229 });
    assert(input().getAttribute("aria-expanded") === "true",
      "legacy composition-confirming key does not close the panel");
    key(input(), "Enter");
    await wait(() => completedSearch("连续回车"), "immediate separate Enter searches the committed text");
    assert(document.activeElement === input(), "IME keyboard search retains input focus");
    await capture("composition_then_enter");
    input().click();
    compose("start", "");
    type("dianji");
    await wait(() => button("开始搜索").disabled, "search button is inactive during composition");
    type("连续点击");
    compose("end", "连续点击");
    await Promise.resolve(); // Let Vue re-enable the submit button, with no arbitrary sleep.
    click(button("开始搜索"));
    await wait(() => completedSearch("连续点击"), "immediate explicit click searches the committed text");
    await capture("composition_then_click");
    input().click();
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
    type("合成输入来源");
    click(button("开始搜索"));
    await wait(() => ready("合成输入来源", "all", ".search-track-card"), "typed input uses typing channel on the native request");
    type("");
    assert(input().placeholder === "发现合成好声音", "default display phrase is shown on empty input");
    input().focus();
    key(input(), "Enter");
    await wait(() => ready("合成默认关键词", "all", ".search-track-card"), "default submit uses the real keyword and defaultquery native channel");
    click(one('[data-category="songs"]'));
    await wait(() => ready("合成默认关键词", "songs", ".track-row"), "category keeps the defaultquery channel");

    await fixture("selection_source");
    await home();
    await wait(() => one('.search-history-tags button[title="合成输入来源"]'), "previously submitted history keyword");
    click(one('.search-history-tags button[title="合成输入来源"]'));
    await wait(() => ready("合成输入来源", "all", ".search-track-card"), "history selection sends native history channel");
    input().focus(); input().click(); type("合成输入推荐来源");
    await wait(() => one('.search-suggestion[title="合成输入推荐来源 · 推荐结果"]'), "recommended input suggestion");
    click(one('.search-suggestion[title="合成输入推荐来源 · 推荐结果"]'));
    await wait(() => ready("合成输入推荐来源推荐", "all", ".search-track-card"), "recommended suggestion sends native rec_query channel");
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
    const invoke=(name,args={})=>window.__TAURI_INTERNALS__.invoke(name,args);
    const {epoch}=await invoke("account_snapshot");
    const snapshot=()=>invoke("player_snapshot",{epoch});
    const pause=()=>invoke("player_action",{epoch,action:{kind:"pause",paused:true}});
    const stillSong=await snapshot();
    assert(stillSong.track?.id==="178"&&stillSong.state==="playing",
      "reselecting the current radio does not interrupt already playing music");
    await pause();
    await wait(()=>one(".playerbar")?.dataset.state==="paused","normal song is paused");
    await home();
    click(row("合成播客榜条目"));
    await wait(()=>!panel()&&one(".playerbar")?.dataset.state==="playing"&&
      one(".now-playing")?.textContent.includes("401"),"same radio starts its programs when other audio is paused");
    const playingRadio=await snapshot();
    const nativeQueue=await invoke("player_queue",{epoch});
    assert(playingRadio.origin?.kind==="podcast"&&playingRadio.origin.id==="301"&&
      playingRadio.track?.voiceId==="401"&&nativeQueue.tracks.map(row=>row.track.voiceId).join(",")==="401,402,403",
      "replacement uses the active radio's authoritative episode queue");
    await capture("podcast_rank_play_from_detail");
    const pausedRadio=await pause();
    await wait(()=>one(".playerbar")?.dataset.state==="paused","radio is paused");
    await home();
    click(row("合成播客榜条目"));
    await wait(()=>!panel()&&one(".playerbar")?.dataset.state==="playing","same paused radio resumes");
    const resumedRadio=await snapshot();
    assert(resumedRadio.playId===pausedRadio.playId&&resumedRadio.entryId===pausedRadio.entryId&&
      resumedRadio.queueRevision===pausedRadio.queueRevision,
      "resume keeps the original native decoder and queue");
    await capture("podcast_rank_resume_in_place");
    click(button("返回上一页"));
    await wait(()=>ready("合成推荐词1", "all", ".search-track-card"),"audio rank Back restores search");
    await home();
    click(row("合成听书榜条目"));
    await wait(()=>!panel()&&one(".podcast-cover h2")?.textContent==="合成播客 302","audiobook rank opens its own detail");
    await capture("audiobook_rank_detail");
    click(button("返回上一页"));
    await wait(()=>ready("合成推荐词1", "all", ".search-track-card"),"audiobook Back restores search");
    await home();
    await wait(() => button("听书榜：更多"), "official audiobook chart advertises More");
    assert(button("播客榜：更多"), "the two official More destinations are independently available");
    click(button("听书榜：更多"));
    const bookPane = () => one('.podcast-catalogue[data-parent-category="10001"]');
    const book = (id) => one(`[data-category-radio="${id}"]`);
    const categoryRating = (id) => book(id)?.querySelector('.podcast-catalogue-rating');
    function checkRating(id, expected) {
      const rating = categoryRating(id);
      const heading = rating?.closest('.podcast-catalogue')?.querySelector('.podcast-catalogue-columns span:last-child');
      assert(rating?.textContent.trim() === expected && heading?.textContent === '评分' &&
        Math.abs(rating.getBoundingClientRect().left - heading.getBoundingClientRect().left) < 1,
        `directory rating ${id} should render ${expected} below the column heading`);
    }
    await wait(() => book(302) && !panel(), "More opens the public audiobook category");
    assert(button("全部", bookPane())?.getAttribute("aria-pressed") === "true" &&
      button("有声书", bookPane())?.getAttribute("aria-current") === "page" &&
      !button("乐评", bookPane()) && !one(".podcast-open"), "public category uses its own first subcategory, not the personal library");
    const bookArt = book(302).querySelector(".search-result-art").getBoundingClientRect();
    assert(bookArt.width === 48 && bookArt.height === 48 &&
      book(302).querySelector(".search-result-name").getBoundingClientRect().bottom < bookPane().getBoundingClientRect().bottom,
      "official 48px title cell keeps the audiobook title visible in a narrow window");
    const firstBooks = await invoke("podcast_catalogue", { epoch, query: { kind: "radios", parentId: "10001", subId: "000" } });
    const handle = firstBooks.value.continuation;
    checkRating(302, '9.50分');
    assert(firstBooks.value.cards[0].score === '9.50', 'directory score keeps service text precision');
    assert(/^[\da-f]{32}$/.test(handle) && !JSON.stringify(firstBooks).includes("internal"), "wire cursor stays in native code");
    for (const [targetEpoch, targetParent, targetSub] of [[epoch, "10001", "3076096"], [epoch, "2", "000"], ["0", "10001", "000"]]) {
      const accepted = await invoke("podcast_catalogue", { epoch: targetEpoch, query: { kind: "radios", parentId: targetParent, subId: targetSub, continuation: handle } }).then(() => true, () => false);
      assert(!accepted, "continuation cannot cross accounts, parents or subcategories");
    }
    await capture("audiobook_more_category");
    click(button("加载更多有声书", bookPane()));
    await wait(() => button("重试", bookPane()), "continuation error is retryable");
    assert(book(302) && !book(303), "the first page stays visible after the continuation fails");
    await capture("audiobook_more_retry");
    click(button("重试", bookPane()));
    await wait(() => book(303) && !button("加载更多有声书", bookPane()), "retry appends the next official page");
    assert(all('[data-category-radio="302"]').length === 1, "overlap does not duplicate cards in the directory");
    checkRating(302, '9.50分');
    checkRating(303, '-');
    await capture("audiobook_more_next");
    click(button("文学", bookPane()));
    await wait(() => book(304) && !book(302) && !book(303), "subcategory replaces the previous directory");
    click(book(304).querySelector("button"));
    await wait(() => one(".podcast-cover h2")?.textContent === "合成播客 304", "directory book opens the real detail");
    click(button("返回上一页"));
    await wait(() => book(304) && !book(302), "detail Back restores the chosen audiobook subcategory");
    await capture("audiobook_more_back");
    click(button("返回上一页"));
    await wait(() => ready("合成推荐词1", "all", ".search-track-card"), "catalogue Back restores prior search");
    await home();
    await wait(() => button("播客榜：更多"), "official podcast chart advertises More");
    click(button("播客榜：更多"));
    const podcastHome = () => one(".podcast-discovery");
    const homeRadio = (id) => one(`[data-home-radio="${id}"]`);
    const homeVoice = (id) => one(`[data-home-voice="${id}"]`);
    await wait(() => homeRadio(305) && homeVoice(404) && !panel(), "podcast More opens the public recommendation homepage");
    assert(!bookPane() && !one(".podcast-open") && !homeVoice(405) &&
      !podcastHome().textContent.includes("不显示推广"), "homepage rejects other destinations, promotion and mismatched voice identity");
    assert(all(".podcast-home-group").map((g) => g.firstElementChild.dataset.homeVoice).join(",") === "404,406",
      "weekly voices retain their nested creative groups and order");
    await capture("podcast_more_home");
    const updatedTop = () => one('.podcast-top[data-layout="updated"]');
    const preview = (name) => updatedTop()?.querySelector(`[data-preview="${name}"]`);
    await wait(() => preview('podcast')?.querySelectorAll('.podcast-top-item').length === 8 &&
      preview('book') && button("打开 合成播客 365", preview('book')), "t1 displays eight score previews and the opted-in book recommendations");
    assert(!button("打开私人漫游", updatedTop()) && button("高分播客", preview('podcast')).getAttribute("aria-pressed") === "true",
      "t1 displays two ranking cards instead of the legacy roam card");
    await capture("podcast_top_updated");
    click(button("查看全部", preview('podcast')));
    await wait(() => one('.podcast-rankings .podcast-rank-groups [aria-current="page"]')?.textContent.trim() === '播客榜' &&
      one('.podcast-rankings .podcast-subtabs [aria-pressed="true"]')?.textContent.trim() === '高分必听' &&
      all('.podcast-rank-row').length === 10, "View All deep links to the same score chart, beyond eight preview items");
    await capture("podcast_top_deep_link");
    click(button("返回上一页"));
    await wait(() => preview('podcast')?.dataset.activeChart === 'score', "Back restores the top preview");
    click(button("最热播客", preview('podcast')));
    const previewVoice = (action) => {
      const lane = preview('podcast');
      const target = lane && button(`${action} 合成声音 407`, lane);
      return target && !target.disabled ? target : null;
    };
    await wait(() => previewVoice("播放"), "updated hot preview uses the ordinary voice chart");
    click(previewVoice("播放"));
    await wait(() => previewVoice("暂停") && one('.playerbar')?.dataset.state === 'playing', "preview row starts native playback");
    const previewPlaying = await snapshot();
    assert(previewPlaying.track?.voiceId === '407' && previewPlaying.playId !== resumedRadio.playId,
      "preview plays the selected episode in a new native entry");
    click(previewVoice("暂停"));
    await wait(() => previewVoice("播放") && one('.playerbar')?.dataset.state === 'paused',
      "preview pauses the same native episode");
    click(previewVoice("播放"));
    await wait(() => previewVoice("暂停") && one('.playerbar')?.dataset.state === 'playing',
      "preview resumes the same native episode");
    const previewResumed = await snapshot();
    assert(previewResumed.playId === previewPlaying.playId && previewResumed.entryId === previewPlaying.entryId &&
      previewResumed.queueRevision === previewPlaying.queueRevision,
      "preview toggles preserve the decoder, selected entry and queue");
    click(button("免费听书", preview('book')));
    await wait(() => preview('book') && button("打开 合成播客 363", preview('book')), "free book preview has independent rows");
    click(button("查看全部", preview('book')));
    await wait(() => one('.podcast-rankings .podcast-rank-groups [aria-current="page"]')?.textContent.trim() === '免费听书榜' &&
      one('.podcast-rank-row')?.textContent.includes('合成播客 363'), "book preview deep links to the configured free chart");
    click(button("返回上一页"));
    await wait(() => preview('book')?.dataset.activeChart === 'free', "Back retains the free selection");
    click(button("好书推荐", preview('book')));
    await wait(() => preview('book')?.dataset.activeChart === 'recommended', "recommendation selected before opt-out proof");
    click(button("加载更多播客", podcastHome()));
    await wait(() => button("重试", podcastHome()), "homepage continuation exposes retry");
    assert(homeRadio(305) && homeVoice(404) && !homeRadio(306), "a failed page retains the existing blocks");
    button("重试", podcastHome()).scrollIntoView({ block: "nearest" });
    await capture("podcast_more_retry");
    click(button("重试", podcastHome()));
    await wait(() => homeRadio(306) && homeRadio(305), "retry appends a second block even if the server repeats its ID");
    assert(all(".podcast-home-block h2").map((h) => h.textContent).join(",") === "合成首页首批,合成新节目,合成首页后续",
      "continued feed retains all original block boundaries and order");
    await capture("podcast_more_next");
    const voiceButton = homeVoice(404).querySelector("button");
    voiceButton.scrollIntoView({ block: "center" });
    click(voiceButton);
    await wait(() => voiceButton.getAttribute("aria-label").startsWith("暂停") && one(".playerbar")?.dataset.state === "playing",
      "a weekly episode reaches actual native playback");
    assert((await snapshot()).track?.voiceId === "404", "playing media belongs to the chosen episode");
    await capture("podcast_more_episode_playing");
    click(homeRadio(305));
    await wait(() => one(".podcast-cover h2")?.textContent === "合成播客 305", "homepage item opens podcast detail");
    click(button("返回上一页"));
    await wait(() => homeRadio(305) && homeRadio(306), "detail Back restores already loaded homepage blocks");
    await capture("podcast_more_back");
    const firstHome = await invoke("podcast_home", { epoch });
    const homeHandle = firstHome.value.continuation;
    assert(/^[\da-f]{32}$/.test(homeHandle) && !JSON.stringify(firstHome).includes("homePrivate") &&
      !JSON.stringify(firstHome).includes("ignored.synthetic.invalid"), "homepage withholds the wire cursor and raw resource URL");
    for (const [command, args] of [
      ["podcast_home", { epoch, continuation: handle }],
      ["podcast_home", { epoch: "0", continuation: homeHandle }],
      ["podcast_catalogue", { epoch, query: { kind: "radios", parentId: "10001", subId: "000", continuation: homeHandle } }],
    ]) assert(!await invoke(command, args).then(() => true, () => false), "native handles cannot cross account or discovery route");
    click(button("刷新播客发现", podcastHome()));
    await wait(() => homeRadio(305) && !homeRadio(306) && button("加载更多播客", podcastHome()),
      "refresh starts a new recommendation feed");
    await fixture("podcast_home_disabled");
    click(button("加载更多播客", podcastHome()));
    await wait(() => podcastHome()?.textContent.includes("个性化推荐已关闭") && button("前往设置", podcastHome()),
      "disabled personalization stops a retained continuation and shows Settings");
    assert(!homeRadio(305) && !homeVoice(404) && !button("加载更多播客", podcastHome()), "old personalized content is cleared");
    await wait(() => preview('book')?.dataset.activeChart === 'free' && button("打开 合成播客 363", preview('book')),
      "opt-out immediately switches the updated card to ordinary free books");
    assert(!button("好书推荐", preview('book')) && !button("打开 合成播客 365", preview('book')), "recommendation tab and its previous card disappear");
    assert(!await invoke("podcast_home", { epoch, continuation: homeHandle }).then(() => true, () => false),
      "the native continuation is retired after opt-out");
    await capture("podcast_more_disabled");
    click(button("前往设置", podcastHome()));
    await wait(() => document.activeElement?.id === "settings-privacy", "Settings opens the focused privacy section");
    click(button("返回上一页"));
    await wait(() => podcastHome()?.textContent.includes("个性化推荐已关闭"), "Back keeps recommendations disabled");
    const categoryStrip = () => one(".podcast-category-strip");
    await wait(() => categoryStrip() && button("音乐播客", categoryStrip()) && button("更多分类", categoryStrip()), "ordinary categories remain when personalized feed is disabled");
    const taxonomy = await invoke("podcast_catalogue", { epoch, query: { kind: "categories" } });
    const ordered = taxonomy.value.categories.map((category) => category.id);
    const bookPosition = ordered.indexOf('10001');
    if (bookPosition > 1) ordered.splice(1, 0, ordered.splice(bookPosition, 1)[0]);
    const stripItems = [...categoryStrip().querySelectorAll("button[data-category-id]:not(.category-more)")];
    const firstHidden = ordered[stripItems.length];
    assert(stripItems.map((node) => node.dataset.categoryId).join(",") === ordered.slice(0, stripItems.length).join(",") &&
      stripItems.every((node) => Math.abs(node.getBoundingClientRect().height - 40) < 1) &&
      categoryStrip().scrollWidth <= categoryStrip().clientWidth &&
      button("更多分类", categoryStrip()).dataset.categoryId === firstHidden,
      "t1 moves audiobooks to the second category and sends More to the first hidden parent");
    click(button("音乐播客", categoryStrip()));
    const catalogue = () => one(".podcast-catalogue");
    await wait(() => catalogue()?.dataset.parentCategory === "2" && book(351), "ordinary parent loads without personalization");
    click(button("加载更多播客", catalogue()));
    await wait(() => book(351) && book(352), "non-audiobook catalogue appends its native continuation");
    checkRating(351, '8.75分');
    checkRating(352, '-');
    await capture("podcast_category_more");
    click(button("乐评", catalogue()));
    await wait(() => book(353) && !book(351) && !book(352), "category subtab uses its own first page");
    checkRating(353, '0分');
    await capture("podcast_category_child");
    click(button("精品播客", catalogue()));
    await wait(() => catalogue()?.dataset.parentCategory === "0020141111" && book(354), "changing parent preserves the official leading-zero ID");
    checkRating(354, '-');
    assert(!catalogue().textContent.includes('category-private'), 'invalid wire score cannot enter the directory');
    await capture("podcast_category_parent");
    click(button("返回上一页"));
    await wait(() => podcastHome()?.textContent.includes("个性化推荐已关闭") && categoryStrip() &&
      button("更多分类", categoryStrip())?.dataset.categoryId === firstHidden,
      "switching parent replaces the catalogue route rather than adding an extra Back step");
    click(button("更多分类", categoryStrip()));
    const firstHiddenRecord = { "2": 351, "0020141111": 354, "10001": 302 }[firstHidden] ?? 355;
    await wait(() => catalogue()?.dataset.parentCategory === firstHidden && book(firstHiddenRecord), "More navigates directly to the first hidden parent category");
    await capture("podcast_category_overflow");
    click(button("返回上一页"));
    await wait(() => podcastHome()?.textContent.includes("个性化推荐已关闭"), "category Back preserves homepage personalization state");
    click(button("排行榜", categoryStrip()));
    const rankPage = () => one(".podcast-rankings");
    const ranked = (kind) => rankPage()?.querySelector(`[data-resource="${kind}"]`);
    await wait(() => ranked("voice")?.textContent.includes("合成声音 407"), "public ranking defaults to the official voice chart");
    assert(button("声音榜", rankPage()).getAttribute("aria-current") === "page" &&
      ranked("voice").querySelector('[aria-label="上升 2 名"]'), "chart advertises the correct movement and group");
    click(button("播放 合成声音 407", rankPage()));
    await wait(() => button("暂停 合成声音 407", rankPage()) && one(".playerbar")?.dataset.state === "playing", "ranked voice starts native playback");
    const rankedPlaying = await snapshot();
    assert(rankedPlaying.track?.voiceId === "407", "rank entry plays the chosen episode, not the radio identity");
    assert(!await invoke("play_selection", { epoch: "0", selection: { kind: "search_result_voice", id: "404" } })
      .then(() => true, () => false), "a retired account epoch cannot select a different episode");
    const afterStaleSelection = await snapshot();
    assert(afterStaleSelection.playId === rankedPlaying.playId && afterStaleSelection.entryId === rankedPlaying.entryId &&
      afterStaleSelection.queueRevision === rankedPlaying.queueRevision,
      "a stale selection does not replace the ranked episode or its queue");
    await capture("podcast_rankings_voice");
    click(button("音乐声音", rankPage()));
    await wait(() => ranked("voice")?.textContent.includes("合成声音 408"), "voice category uses categoryId rather than chartType");
    click(button("播客榜", rankPage()));
    await wait(() => ranked("radio")?.textContent.includes("合成播客 361"), "podcast rank loads its own chart menu and rows");
    assert(ranked("radio").querySelector('.rank-score')?.textContent.trim() === "9.3 分", "rank score is displayed once");
    click(ranked("radio").querySelector("button.rank-resource"));
    await wait(() => one(".podcast-cover h2")?.textContent === "合成播客 361", "podcast rank entry opens the canonical detail");
    click(button("返回上一页"));
    await wait(() => ranked("radio")?.textContent.includes("合成播客 361"), "detail Back preserves the selected chart");
    await capture("podcast_rankings_podcasts");
    click(button("音乐播单", rankPage()));
    await wait(() => ranked("radio")?.textContent.includes("合成播客 364"), "podcast category uses its recovered endpoint");
    click(button("主播榜", rankPage()));
    await wait(() => ranked("host")?.textContent.includes("合成主播榜"), "host default uses the fixed hours rank");
    assert(ranked("host").querySelector('[aria-label="上升 3 名"]'), "host rank movement uses the host payload");
    await capture("podcast_rankings_hosts");
    await wait(() => button("听书总榜", rankPage()) && button("免费听书榜", rankPage()), "book chart groups load independently of preference");
    const groupLabels = [...rankPage().querySelectorAll('.podcast-rank-groups button')].map((node) => node.textContent.trim());
    assert(groupLabels.slice(0,5).join(",") === "声音榜,播客榜,主播榜,听书总榜,免费听书榜", "book tabs use the recovered order");
    click(button("听书总榜", rankPage()));
    await wait(() => ranked("radio")?.textContent.includes("合成播客 362"), "ordinary audiobook chart still loads with opt-out");
    assert(!button("个性听书榜", rankPage()) && !await invoke("podcast_rankings", {
      epoch, query: { kind: "entries", group: "overall_charts", id: "RECOMMEND_CHARTS", chartType: "0" },
    }).then(() => true, () => false), "recommendation tab is absent and native direct access is blocked when disabled");
    await capture("podcast_rankings_book_optout");
    click(button("免费听书榜", rankPage()));
    await wait(() => ranked("radio")?.textContent.includes("合成播客 363"), "free audiobook chart still loads with opt-out");
    await capture("podcast_rankings_free");
    click(button("返回上一页"));
    await wait(() => podcastHome()?.textContent.includes("个性化推荐已关闭"), "rank Back returns directly to discovery");
    click(button("返回上一页"));
    await wait(() => ready("合成推荐词1", "all", ".search-track-card"), "public homepage Back restores prior search");
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
      searchInputChannelVerified: true,
      searchSelectionChannelsVerified: true,
      searchDisplayLabelVerified: true,
      cyclicSuggestionsVerified: true,
      immediateCompositionSubmitVerified: true,
      audioRankNavigationVerified: true,
      audioRankPlaybackVerified: true,
      audiobookMoreVerified: true,
      podcastMoreVerified: true,
      podcastCategoryVerified: true,
      podcastCategoryRatingsVerified: true,
      podcastRankingsVerified: true,
      podcastTopUpdatedVerified: true,
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
