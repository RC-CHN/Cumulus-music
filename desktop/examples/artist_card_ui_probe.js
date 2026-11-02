// Production artist cards, synthetic APIs/artwork and native libmpv/null audio.
(async () => {
  const invoke = (name, args = {}) =>
    window.__TAURI_INTERNALS__.invoke(name, args);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (value, label) => {
    if (!value) throw Error(label);
  };
  const all = (selector, root = document) =>
    [...root.querySelectorAll(selector)].filter(
      (node) => !node.closest(".page-leave-active,.catalog-tab-leave-active"),
    );
  const one = (selector) => all(selector)[0];
  const button = (label, root = document) =>
    all("button", root).find(
      (node) =>
        node.textContent.trim() === label ||
        node.getAttribute("aria-label") === label,
    );
  const emit = (stage, value = {}) =>
    invoke("plugin:event|emit", {
      event: "ux-proof",
      payload: { stage, ...value },
    });
  const source = Object.getOwnPropertyDescriptor(
    HTMLImageElement.prototype,
    "src",
  );
  Object.defineProperty(HTMLImageElement.prototype, "src", {
    ...source,
    set(value) {
      source.set.call(
        this,
        String(value).startsWith("https://")
          ? String(value).includes("synthetic-missing")
            ? "data:image/png;base64,AAAA"
            : "data:image/svg+xml," +
              encodeURIComponent(
                '<svg xmlns="http://www.w3.org/2000/svg" width="320" height="320"><rect width="320" height="320" fill="#66528f"/><circle cx="160" cy="160" r="95" fill="#ce9aac"/></svg>',
              )
          : value,
      );
    },
  });
  window.__artistCardAck = {};
  let epoch,
    count = 0,
    command = 0,
    last = "boot";
  async function wait(test, label) {
    for (let i = 0; i < 600; i++) {
      if (await test()) return;
      await sleep(40);
    }
    throw Error(label);
  }
  async function settle() {
    await wait(async () => {
      await new Promise((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(resolve)),
      );
      return !document.getAnimations().some((animation) => {
        const end = animation.effect?.getComputedTiming().endTime;
        return (
          Number.isFinite(end) &&
          (animation.pending || animation.playState === "running")
        );
      });
    }, "finite transitions");
  }
  function bounds(node) {
    if (!(node instanceof HTMLElement) || !node.isConnected) return null;
    const rect = node.getBoundingClientRect();
    let left = Math.max(0, rect.left),
      right = Math.min(innerWidth, rect.right),
      top = Math.max(0, rect.top),
      bottom = Math.min(innerHeight, rect.bottom);
    for (let parent = node; parent; parent = parent.parentElement) {
      const style = getComputedStyle(parent),
        box = parent.getBoundingClientRect();
      if (
        style.display === "none" ||
        style.visibility === "hidden" ||
        Number(style.opacity) === 0
      )
        return null;
      if (["auto", "scroll", "hidden", "clip"].includes(style.overflowX)) {
        left = Math.max(left, box.left);
        right = Math.min(right, box.right);
      }
      if (["auto", "scroll", "hidden", "clip"].includes(style.overflowY)) {
        top = Math.max(top, box.top);
        bottom = Math.min(bottom, box.bottom);
      }
    }
    return right - left > 1 && bottom - top > 1
      ? { left, right, top, bottom }
      : null;
  }
  function click(node) {
    assert(node?.isConnected && !node.disabled, "enabled control");
    if (!bounds(node)) node.scrollIntoView({ block: "nearest" });
    node.focus({ preventScroll: true });
    node.click();
  }
  async function mode(value) {
    const id = ++command;
    await emit("artist-card-fixture", { id, mode: value });
    await wait(() => window.__artistCardAck[id], "fixture ACK");
    return window.__artistCardAck[id];
  }
  async function theme(value) {
    localStorage.setItem("cumulus.appearance.v1", value);
    dispatchEvent(
      new StorageEvent("storage", {
        key: "cumulus.appearance.v1",
        newValue: value,
      }),
    );
    await wait(() => document.documentElement.dataset.theme === value, "theme");
    await settle();
  }
  async function resize(width, height) {
    await emit("resize", { width, height, scale: devicePixelRatio });
    await wait(
      () => innerWidth === width && innerHeight === height,
      "CSS viewport",
    );
    await settle();
  }
  const card = (id = 13) => one(`[data-catalog-key="artist:${id}"]`)?.closest('.catalog-artist-card');
  const play = (id = 13) => card(id)?.querySelector('.catalog-artist-play');
  const opener = (id = 13) => card(id)?.querySelector('.catalog-artist-open');
  const followCard = (id = 13) => one(`[data-collection-key="artist:${id}"]`)?.closest('.collection-follow-card');
  async function menu(id = 13, keyboard = false, follow = false) {
    const root = follow ? followCard(id) : card(id);
    const anchor = root?.querySelector(keyboard ? (follow ? '.collection-follow-open' : '.catalog-artist-open') : '.menu-trigger');
    assert(anchor, 'menu anchor');
    if (keyboard) {
      anchor.focus({preventScroll:true});
      anchor.dispatchEvent(new KeyboardEvent('keydown',{key:'F10',shiftKey:true,bubbles:true,cancelable:true}));
    } else click(anchor);
    await wait(() => one('[role="menu"]'), 'menu open'); await settle();
  }
  async function action(key) { click(one(`[role="menu"] [data-menu-key="${key}"]`)); await settle(); }
  async function similar() { await wait(() => one('.catalog-card-scroll')?.dataset.catalogTotal === '43', 'similar artists'); await settle(); }
  async function membership() {
    await wait(() => one('.artist-membership-dialog[open]') && !one('.artist-membership-dialog [role="status"]'), 'fresh membership read');
    await settle();
  }
  async function closeMembership() { click(button('关闭歌手关注确认')); await wait(() => !one('.artist-membership-dialog'), 'closed membership'); await settle(); }
  async function follows() {
    click(button('专辑与关注')); await wait(() => one('.collections-page'), 'collections');
    click(button('关注歌手')); await wait(() => one('.collection-follow-card'), 'follow list'); await settle();
  }
  function filter(value) { const input = one('[aria-label="筛选收藏列表"]'); input.value=value; input.dispatchEvent(new Event('input',{bubbles:true})); }
  async function backSimilar() {
    click(button('返回上一页')); await similar();
    const grid = one('.catalog-card-scroll');grid.scrollTop=0;grid.dispatchEvent(new Event('scroll'));await settle();
  }
  const snapshot = () => invoke("player_snapshot", { epoch });
  async function native(state, id) {
    await wait(async () => {
      const current = await snapshot();
      return (
        current.state === state &&
        one(".playerbar")?.dataset.state === state &&
        (!id || current.origin?.id === String(id)) &&
        (!play(id) || !play(id).disabled) &&
        (state !== "playing" || current.position > 0.2)
      );
    }, "native " + state);
    await settle();
    return snapshot();
  }
  function same(actual, expected, queue = true) {
    for (const key of [
      "playId",
      "entryId",
      ...(queue ? ["queueRevision"] : []),
    ])
      assert(actual[key] === expected[key], "retained " + key);
    assert(
      actual.track.id === expected.track.id &&
        actual.position + 0.15 >= expected.position,
      "retained song/position",
    );
  }
  async function capture(label, extra = {}) {
    last = label;
    await settle();
    const dialog = one("dialog[open]"),
      menu = one('[role="menu"]');
    const feedback = one('.artist-action-feedback');
    // A visible feedback surface can legitimately occlude a lower virtual row.
    // Assert the foreground controls; background cards remain layout-bounded.
    const root =
      dialog ?? menu ?? feedback ?? one(".collections-page") ?? one(".catalog-page");
    const active = document.activeElement;
    const menuBox = menu?.getBoundingClientRect();
    const menuContained = !menuBox || menuBox.left >= 7.5 && menuBox.top >= 7.5 && menuBox.right <= innerWidth - 7.5 && menuBox.bottom <= innerHeight - 7.5;
    const controls = all("button", root)
      .filter((node) => !node.disabled && bounds(node))
      .map((node) => {
        // The independent center play control deliberately overlays the
        // artist opener. Test the opener's exposed padding, play at its center.
        const box = bounds(node), overlay = node.matches('.catalog-artist-open,.collection-follow-open');
        const hit = document.elementFromPoint(
          overlay ? box.left + Math.min(8, (box.right - box.left) / 2) : (box.left + box.right) / 2,
          overlay ? box.top + Math.min(8, (box.bottom - box.top) / 2) : (box.top + box.bottom) / 2,
        );
        return {
          label: node.getAttribute("aria-label") ?? node.textContent.trim(),
          hit: node === hit || node.contains(hit),
        };
      });
    const current = await snapshot(),
      queue = await invoke("player_queue", { epoch });
    const paint = dialog ?? menu ?? one(".playerbar"),
      box = paint.getBoundingClientRect();
    const value = {
      width: innerWidth,
      height: innerHeight,
      ratio: devicePixelRatio,
      screenX,
      screenY,
      captureTitle: "Cumulus",
      visible: document.visibilityState === "visible",
      focused: document.hasFocus(),
      focusValid: active !== document.body && !!bounds(active),
      focusKey: active?.dataset?.catalogKey ?? active?.dataset?.collectionKey ?? null,
      focusLabel:
        active?.getAttribute("aria-label") ??
        active?.textContent.trim().slice(0, 80),
      theme: document.documentElement.dataset.theme,
      overflow:
        document.documentElement.scrollWidth > innerWidth ||
        (!!one(".catalog-card-scroll") &&
          one(".catalog-card-scroll").scrollWidth >
            one(".catalog-card-scroll").clientWidth + 1),
      cards: all(".catalog-artist-card,.collection-follow-card").length,
      controls,
      menuContained,
      menuKeys: menu
        ? all("[data-menu-key]", menu).map((node) => node.dataset.menuKey)
        : [],
      metadata: all(".catalog-artist-card").map(node => ({
        name: node.querySelector(".catalog-artist-name")?.textContent.trim(),
        description: node.querySelector(".catalog-artist-description")?.textContent.trim(),
        fallback: !!node.querySelector(".catalog-card-placeholder"),
      })),
      playback: {
        state: current.state,
        track: current.track?.id,
        playId: current.playId,
        entryId: current.entryId,
        queueRevision: current.queueRevision,
        position: current.position,
        origin: current.origin,
        queueLen: current.queueLen,
      },
      queue: queue.tracks.map((entry) => ({
        entryId: entry.entryId,
        track: entry.track.id,
      })),
      error:
        one(".artist-action-feedback.failed")?.textContent.trim() ??
        one(".artist-membership-dialog .catalog-error")?.textContent.trim() ??
        "",
      empty: one(".collections-page .empty-state")?.textContent.trim() ?? "",
      keyword: one('[aria-label="筛选收藏列表"]')?.value ?? null,
      paintSamples: [
        {
          x: menu && !dialog ? box.left + box.width / 2 : box.left + 4,
          y: menu && !dialog ? box.top + 2 : box.top + box.height / 2,
          color: getComputedStyle(paint).backgroundColor,
        },
      ],
      ...extra,
    };
    if (!extra.diagnostic) {
      assert(
        value.visible && value.focused && value.focusValid && !value.overflow,
        "visible/focused layout",
      );
      assert(
        controls.every((control) => control.hit),
        "actual controls hit",
      );
      assert(value.cards <= 36, "bounded cards");
      if (menu) assert(menuContained && controls.length === all('button:not(:disabled)', menu).length, 'whole menu and every choice visible');
    }
    const stage = `ux_${String(++count).padStart(2, "0")}_${label}`;
    window.__CUMULUS_ALBUM_CAPTURE_ACK__ = null;
    await emit(stage, value);
    await wait(
      () => window.__CUMULUS_ALBUM_CAPTURE_ACK__ === stage,
      "capture ACK",
    );
  }
  try {
    await wait(() => one('.track-row'), 'startup');
    epoch=(await invoke('account_snapshot')).epoch;
    await resize(1120,760);await theme('light');
    click(one('.track-credit-links button'));await wait(() => one('.artist-header'), 'artist');
    click(one('.artist-tabs [data-catalog-tab="similar"]'));await similar();
    opener().focus({preventScroll:true});
    await wait(() => card(14)?.querySelector('.catalog-card-placeholder'), 'broken avatar fallback');
    assert(card().querySelector('.catalog-artist-description').textContent==='单曲：55','official song count');
    await capture('cards_light');
    const grid=one('.catalog-card-scroll');grid.scrollTop=grid.scrollHeight;grid.dispatchEvent(new Event('scroll'));
    await wait(() => opener(55),'virtual tail');opener(55).focus({preventScroll:true});await capture('tail_light');
    grid.scrollTop=0;grid.dispatchEvent(new Event('scroll'));await wait(()=>opener(),'virtual head');
    await theme('dark');opener().focus({preventScroll:true});await capture('long_dark');
    await menu(13,true);await capture('keyboard_menu_dark');
    await action('copy');await wait(()=>one('.artist-action-feedback')?.textContent.includes('链接已复制'),'copy result');await capture('copy_dark');
    click(button('关闭',one('.artist-action-feedback')));
    await menu();await action('collect');await membership();await capture('collect_confirm_dark');
    click(button('取消',one('.artist-membership-dialog')));await wait(()=>!one('.artist-membership-dialog'),'cancel');await settle();await capture('collect_canceled_dark');
    await mode('artist-state-offline');await menu();await action('collect');await membership();
    assert(one('.artist-membership-dialog .catalog-error'),'read failure');await capture('collect_read_failed_dark');
    await mode('none');click(button('重新读取',one('.artist-membership-dialog')));await membership();await capture('collect_read_recovered_dark');
    await mode('artist-sub-reject');click(button('确认',one('.artist-membership-dialog')));await membership();
    await wait(()=>one('.artist-membership-dialog .catalog-error'),'reject');await capture('collect_rejected_dark');
    await mode('none');click(button('确认',one('.artist-membership-dialog')));await wait(()=>!one('.artist-membership-dialog'),'explicit retry');await settle();await capture('collected_dark');
    await follows();await theme('light');
    await wait(()=>followCard(),'collected artist in follows');
    followCard().querySelector('.collection-follow-open').focus({preventScroll:true});await capture('follows_light');
    await menu(13,true,true);await capture('follow_menu_light');await action('copy');
    await wait(()=>one('.artist-action-feedback')?.textContent.includes('链接已复制'),'follow copy');await capture('follow_copy_light');click(button('关闭',one('.artist-action-feedback')));
    filter('长名称');await wait(()=>all('.collection-follow-card').length===1,'filter followed artist');
    await menu(13,false,true);await action('open');
    await wait(()=>one('.artist-header h1')?.textContent.startsWith('长名称'),'artist detail');await settle();await capture('followed_detail_light');
    click(button('已关注',one('.artist-header')));await wait(()=>button('关注',one('.artist-header')),'unfollow result');
    click(button('返回上一页'));await wait(()=>one('.collections-page .empty-state')?.textContent==='没有匹配的内容','return invalidates follows');await settle();await capture('unfollowed_return_light');
    click(button('返回上一页'));await similar();
    await mode('artist-sub-lost');await menu();await action('collect');await membership();click(button('确认',one('.artist-membership-dialog')));
    await wait(()=>button('核对结果',one('.artist-membership-dialog')),'uncertain follow');await settle();await capture('lost_write_light');
    await closeMembership();await menu();await action('collect');await membership();await capture('pending_reopened_light');
    await mode('none');click(button('核对结果',one('.artist-membership-dialog')));await wait(()=>!one('.artist-membership-dialog'),'readback confirmed');await settle();await capture('reconciled_light');
    click(play());let playing=await native('playing',13);
    await invoke('player_action',{epoch,action:{kind:'seek',seconds:3}});await wait(async()=> (await snapshot()).position>=3,'seek');playing=await snapshot();await capture('playing_light');
    click(play());let paused=await native('paused',13);same(paused,playing);await capture('paused_light');
    click(play());playing=await native('playing',13);same(playing,paused);await capture('resumed_light');
    await invoke('player_action',{epoch,action:{kind:'stop'}});await native('idle');await capture('stopped_light');
    click(play());const restarted=await native('playing',13);assert(restarted.playId!==playing.playId,'new load after Stop');await capture('restarted_light');
    click(play(14));await native('playing',14);click(play(14));paused=await native('paused',14);await capture('other_artist_paused_light');
    click(play(15));await wait(()=>one('.playback-message'),'empty artist error');await wait(()=>!play(15).disabled,'selection ended');
    same(await snapshot(),paused);await settle();await capture('empty_artist_failed_light');
    click(play());await native('playing',13);click(play());await native('paused',13);await capture('empty_artist_recovered_light');
    click(opener());await wait(()=>one('.artist-header h1')?.textContent.startsWith('长名称'),'view artist');await settle();await capture('view_artist_light');
    await backSimilar();await wait(()=>document.activeElement?.dataset.catalogKey==='artist:13','return source focus');await capture('view_return_light');
    await resize(760,540);await theme('dark');opener().focus({preventScroll:true});await capture('small_cards_dark');
    await menu(13,true);await capture('small_menu_dark');await action('collect');await membership();await capture('small_already_followed_dark');await closeMembership();await capture('small_confirm_closed_dark');
    await follows();await theme('light');filter('长名称');await wait(()=>followCard(),'small follow row');followCard().querySelector('.collection-follow-open').focus({preventScroll:true});await capture('small_follow_light');
    await menu(13,true,true);await capture('small_follow_menu_light');await action('open');
    await wait(()=>one('.artist-header h1')?.textContent.startsWith('长名称'),'small artist detail');await settle();await capture('small_detail_light');
    click(button('返回上一页'));await wait(()=>followCard(),'small return');await settle();await capture('small_follow_return_light');
    await emit('complete',{ok:true,artistCards:true,stageCount:count});
  } catch(error) {
    await capture('diagnostic',{diagnostic:true}).catch(()=>{});
    await emit('failed',{ok:false,error:String(error),last,active:document.activeElement?.outerHTML?.slice(0,800)});
  }
})();
