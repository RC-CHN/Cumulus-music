// Actual mini/main WebViews and native FM journal. Only backend/account/PCM are synthetic.
(async () => {
  const invoke = (command, args = {}) =>
    window.__TAURI_INTERNALS__.invoke(command, args);
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const assert = (ok, label) => {
    if (!ok) throw Error(label);
  };
  const wait = async (check, label) => {
    for (let n = 0; n < 240; n++) {
      if (await check()) return;
      await sleep(60);
    }
    throw Error(label);
  };
  const emit = (value) =>
    invoke("plugin:event|emit", { event: "mini-proof", payload: value });
  window.__miniAck = {};
  let seq = 0,
    count = 0,
    last = "initial",
    epoch,
    playId;
  const source = { kind: "personal" };
  async function fixture(kind, payload = {}) {
    const id = ++seq;
    await emit({ kind, id, ...payload });
    await wait(() => id in window.__miniAck, "fixture " + kind);
    const v = window.__miniAck[id];
    delete window.__miniAck[id];
    if (v.error) throw Error(v.error);
    return v.value ?? v;
  }
  const mini = (code) => fixture("eval", { source: code });
  const control = (action) => fixture("fm-feedback", { action });
  const panel = () =>
    mini(
      "return !!document.querySelector('.mini-feedback[open]:not(.modal-leave-active)');",
    );
  async function click(label) {
    return mini(
      `const d=document.querySelector('.mini-feedback[open]:not(.modal-leave-active)')||document;const n=[...d.querySelectorAll('button')].find(n=>n.getAttribute('aria-label')===${JSON.stringify(label)}||n.textContent.trim()===${JSON.stringify(label)});if(!n||n.disabled)throw Error('disabled '+${JSON.stringify(label)});n.scrollIntoView({block:'nearest'});n.focus({preventScroll:true});${label === "到主窗口查看" || label === "返回主窗口" ? "setTimeout(()=>n.click(),80)" : "n.click()"};return true;`,
    );
  }
  async function theme(value, main = false) {
    const code = `localStorage.setItem('cumulus.appearance.v1',${JSON.stringify(value)});window.dispatchEvent(new StorageEvent('storage',{key:'cumulus.appearance.v1',newValue:${JSON.stringify(value)}}));`;
    if (main) {
      localStorage.setItem("cumulus.appearance.v1", value);
      window.dispatchEvent(
        new StorageEvent("storage", {
          key: "cumulus.appearance.v1",
          newValue: value,
        }),
      );
    } else await mini(code + "return true;");
    await sleep(80);
  }
  async function prepare() {
    await invoke("player_action", {
      epoch,
      action: { kind: "pause", paused: true },
    });
    await invoke("player_action", {
      epoch,
      action: { kind: "seek", seconds: 10 },
    });
    await wait(() => {
      const b = document.querySelector(".playerbar");
      return (
        b?.dataset.state === "paused" &&
        Math.abs(Number(b.dataset.position) - 10) < 0.2
      );
    }, "paused10");
    playId = document.querySelector(".playerbar").dataset.playId;
  }
  async function openMini() {
    await invoke("mini_enter");
    await wait(async () => !(await fixture("windows")).main, "mini active");
    await wait(
      () =>
        mini(
          "return !!document.querySelector('[aria-label=\"减少推荐\"]:not(:disabled)');",
        ),
      "feedback entry",
    );
  }
  async function show(ready = true) {
    await click("减少推荐");
    await wait(panel, "dialog");
    if (ready)
      await wait(
        () =>
          mini(
            "const d=document.querySelector('.mini-feedback[open]');return d?.getAttribute('aria-busy')==='false'&&d.querySelector('[data-reason-key]');",
          ),
        "offered reasons",
      );
  }
  async function close() {
    await click("取消");
    await wait(async () => !(await panel()), "closed");
    await wait(
      () =>
        mini(
          "return document.activeElement===document.querySelector('.mini-transport [aria-label=\"减少推荐\"]');",
        ),
      "focus returned",
    );
  }
  async function readState() {
    await click("重新读取状态");
    await wait(
      () =>
        mini(
          "return document.querySelector('.mini-feedback')?.getAttribute('aria-busy')==='false';",
        ),
      "readonly settled",
    );
    await sleep(80);
  }
  const facts = `const d=document.querySelector('.mini-feedback[open]:not(.modal-leave-active)');const rect=n=>{const r=n.getBoundingClientRect();return {left:r.left,top:r.top,right:r.right,bottom:r.bottom,width:r.width,height:r.height}};const hit=n=>{const r=rect(n),p=document.elementFromPoint(r.left+r.width/2,r.top+r.height/2);return p===n||n.contains(p)};const nodes=[...(d?.querySelectorAll('header button,footer button')??document.querySelectorAll('.mini-transport button'))].filter(n=>!n.disabled);const b=d?.querySelector('.mini-feedback-body'),active=document.activeElement;
   return {windowKind:'mini',width:innerWidth,height:innerHeight,ratio:devicePixelRatio,visible:document.visibilityState==='visible',focused:document.hasFocus(),theme:document.documentElement.dataset.theme,overflow:document.documentElement.scrollWidth>innerWidth,dialog:!!d,entryId:d?.dataset.entryId??null,needsRead:d?.dataset.needsRead==='true',review:d?.dataset.review==='true',canSubmit:!!d&&!d.querySelector('.mini-feedback-submit').disabled,busy:d?.getAttribute('aria-busy')==='true',title:d?.querySelector('h2').textContent??null,text:d?.textContent??null,body:b?rect(b):null,box:d?rect(d):null,controls:nodes.map(n=>({label:n.getAttribute('aria-label')||n.textContent.trim(),hit:hit(n),rect:rect(n)})),reasons:[...(d?.querySelectorAll('[data-reason-key]')??[])].map(n=>({key:n.dataset.reasonKey,artist:n.dataset.artistId??null,disabled:n.disabled,title:n.title,ellipsis:n.scrollWidth>n.clientWidth})),active:active?.outerHTML.slice(0,300),focusInside:!d||d.contains(active),paintSamples:d?[{x:rect(d).left+3,y:rect(d).top+rect(d).height/2,color:getComputedStyle(d).backgroundColor}]:[{x:3,y:3,color:getComputedStyle(document.querySelector('.mini-shell')).backgroundColor}]};`;
  function recoveryVisible() {
    const region = document.querySelector(".fm-recovery"),
      root = document.querySelector(".main-content");
    if (!region || !root) return false;
    const a = region.getBoundingClientRect(),
      b = root.getBoundingClientRect();
    return (
      a.top >= b.top - 1 &&
      a.bottom <= b.bottom + 1 &&
      [...region.querySelectorAll("button:not(:disabled)")].every((n) => {
        const r = n.getBoundingClientRect(),
          p = document.elementFromPoint(
            r.left + r.width / 2,
            r.top + r.height / 2,
          );
        return p === n || n.contains(p);
      })
    );
  }
  async function capture(name, main = false) {
    last = name;
    await sleep(250);
    let data;
    if (main) {
      const b = document.querySelector(".playerbar");
      data = {
        windowKind: "main",
        width: innerWidth,
        height: innerHeight,
        ratio: devicePixelRatio,
        visible: document.visibilityState === "visible",
        focused: document.hasFocus(),
        theme: document.documentElement.dataset.theme,
        pageFm: !!document.querySelector(".fm-page"),
        recovery: document.querySelector(".fm-recovery")?.textContent ?? null,
        recoveryVisible: recoveryVisible(),
        mainFmTitle: document.querySelector(".fm-copy h2")?.textContent ?? null,
        stateIssue:
          document.querySelector(".fm-state-region")?.textContent ?? null,
        mainPlayerDom: {
          state: b.dataset.state,
          playId: b.dataset.playId,
          position: b.dataset.position,
        },
      };
      assert(data.pageFm, "handoff opens actual FM page");
    } else {
      data = await mini(facts);
      assert(!data.overflow && data.focusInside, "contained focus");
      assert(
        data.controls.every(
          (n) =>
            n.hit &&
            n.rect.left >= 0 &&
            n.rect.right <= data.width &&
            n.rect.top >= 0 &&
            n.rect.bottom <= data.height,
        ),
        "enabled controls visible",
      );
      if (data.box)
        assert(
          data.box.left >= 4 &&
            data.box.top >= 4 &&
            data.box.right <= data.width - 4 &&
            data.box.bottom <= data.height - 4,
          "dialog contained",
        );
    }
    const stage = `ux_${String(++count).padStart(2, "0")}_${name}`;
    window.__miniRecorded = null;
    await emit({
      kind: "snapshot",
      stage,
      miniFeedbackUi: true,
      fmSource: source,
      expectedPlayId: playId,
      ...data,
    });
    await wait(() => window.__miniRecorded === stage, "native " + name);
  }
  async function returnFm(name, appearance) {
    await click("到主窗口查看");
    await wait(async () => {
      const v = await fixture("windows");
      return v.main && !v.mini.length;
    }, "main restored");
    await wait(
      () =>
        recoveryVisible() && document.activeElement?.closest(".fm-recovery"),
      "FM journal visible",
    );
    await theme(appearance, true);
    await capture(name, true);
  }
  async function reconcile() {
    const b = [...document.querySelectorAll(".fm-recovery button")].find(
      (n) => n.textContent.trim() === "核对并继续",
    );
    assert(b && !b.disabled, "explicit reconcile");
    b.focus();
    b.click();
    await wait(() => !document.querySelector(".fm-recovery"), "reconciled");
    await prepare();
  }
  try {
    await wait(() => document.querySelector(".track-row"), "main");
    epoch = (await invoke("account_snapshot")).epoch;
    await invoke("fm_command", {
      epoch,
      source,
      command: { kind: "start", quality: "standard" },
    });
    await prepare();
    await openMini();
    await theme("light");
    assert(
      await mini(
        "try{await window.__TAURI_INTERNALS__.invoke('account_snapshot');return false;}catch{return true;}",
      ),
      "mini has no main account IPC",
    );
    const shown = await mini(
      "return {generation:location.hash.slice(6),ticket:document.querySelector('.mini-shell').dataset.controlTicket};",
    );
    let mainRejected = false;
    try {
      await invoke("mini_fm_feedback", { ...shown, reasonKey: null });
    } catch {
      mainRejected = true;
    }
    assert(mainRejected, "main cannot call dedicated mini feedback IPC");
    assert(
      await mini(
        "try{await window.__TAURI_INTERNALS__.invoke('mini_fm_feedback',{generation:'old-window',ticket:'1',reasonKey:null});return false;}catch(e){return e.code==='stale';}",
      ),
      "old lifetime rejected",
    );
    await capture("compact_light");
    await control("hold-reasons");
    await show(false);
    await wait(
      () =>
        mini(
          "return document.querySelector('.mini-feedback')?.textContent.includes('正在读取反馈选项');",
        ),
      "reason wait",
    );
    await wait(
      async () => (await control("report")).reasonReads === 1,
      "native reasons reached hold",
    );
    await capture("reason_wait_light");
    await close();
    await control("release-reasons");
    await capture("reason_cancelled_light");
    await show();
    await capture("ready_light");
    await theme("dark");
    await mini(
      "document.querySelector('.mini-feedback').dispatchEvent(new KeyboardEvent('keydown',{key:' ',code:'Space',bubbles:true,cancelable:true}));return true;",
    );
    await capture("ready_dark");
    await close();
    await click("播放队列");
    await wait(() => mini("return innerHeight===516;"), "expanded");
    await show();
    await capture("expanded_dark");
    await close();
    await control("reason-error");
    await show(false);
    await wait(
      () =>
        mini(
          "return !![...document.querySelectorAll('.mini-feedback button')].find(n=>n.textContent.trim()==='重新加载选项');",
        ),
      "reason error",
    );
    await capture("reason_error_dark");
    await click("重新加载选项");
    await wait(
      () =>
        mini(
          "return document.querySelectorAll('.mini-feedback [data-reason-key]').length===3;",
        ),
      "reason retry",
    );
    await capture("reason_retried_dark");
    await close();
    await theme("light");
    await control("reason-empty");
    await show(false);
    await wait(
      () =>
        mini(
          "return document.querySelector('.mini-feedback')?.textContent.includes('暂无更多反馈选项');",
        ),
      "empty options",
    );
    await capture("reason_empty_light");
    await close();
    await control("reason-normal");
    await show();
    await control("trash-rejected");
    await click("仅减少此歌曲");
    await wait(
      () =>
        mini(
          "return document.querySelector('.mini-feedback')?.dataset.needsRead==='true'&&document.querySelector('.mini-feedback').getAttribute('aria-busy')==='false';",
        ),
      "rejected stays",
    );
    await capture("rejected_light");
    await theme("dark");
    await capture("rejected_dark");
    await readState();
    await wait(
      () =>
        mini(
          "return !document.querySelector('.mini-feedback-submit').disabled;",
        ),
      "observed rejection",
    );
    await capture("observed_dark");
    await control("hold-write");
    await click("仅减少此歌曲");
    await wait(
      () =>
        mini(
          "return document.querySelector('.mini-feedback')?.getAttribute('aria-busy')==='true';",
        ),
      "write held",
    );
    await wait(
      async () => (await control("report")).trashWrites === 2,
      "native write reached hold",
    );
    let refusedNext = false;
    try {
      await invoke("player_action", { epoch, action: { kind: "next" } });
    } catch (error) {
      refusedNext = error.code === "busy";
    }
    assert(
      refusedNext,
      "concurrent Next is refused without cancelling accepted feedback",
    );
    await capture("write_pending_dark");
    await control("release-write");
    await wait(async () => !(await panel()), "success closes");
    await prepare();
    await wait(
      () =>
        mini(
          "return document.querySelectorAll('.mini-queue-rows li').length===30 && !document.querySelector('.mini-queue-empty[role=\"alert\"]');",
        ),
      "queue observes accepted feedback",
    );
    await capture("submitted_dark");
    await show();
    await invoke("player_action", { epoch, action: { kind: "next" } });
    await prepare();
    await wait(
      () =>
        mini(
          "return document.querySelector('.mini-feedback')?.textContent.includes('当前歌曲已切换');",
        ),
      "old popup",
    );
    await capture("changed_dark");
    await close();
    await capture("changed_closed_dark");
    await theme("light");
    await show();
    await control("artist-rejected");
    await mini(
      "const n=document.querySelector('.mini-feedback [data-artist-id]');n.focus();n.click();return true;",
    );
    await wait(
      () =>
        mini(
          "return document.querySelector('.mini-feedback')?.dataset.needsRead==='true'&&document.querySelector('.mini-feedback').getAttribute('aria-busy')==='false';",
        ),
      "partial",
    );
    await prepare();
    await capture("artist_partial_light");
    await returnFm("main_partial_light", "light");
    await invoke("fm_command", {
      epoch,
      source,
      command: { kind: "artist", id: "1", blocked: true },
    });
    await prepare();
    await openMini();
    await theme("light");
    await capture("reopened_light");
    await show();
    await control("trash-lost");
    await click("仅减少此歌曲");
    await wait(
      () =>
        mini(
          "return document.querySelector('.mini-feedback')?.dataset.needsRead==='true'&&document.querySelector('.mini-feedback').getAttribute('aria-busy')==='false';",
        ),
      "uncertain",
    );
    await capture("uncertain_light");
    await readState();
    await wait(
      () =>
        mini(
          "return document.querySelector('.mini-feedback')?.dataset.review==='true';",
        ),
      "journal readonly",
    );
    await capture("uncertain_observed_light");
    await close();
    await theme("dark");
    await show(false);
    await wait(
      () =>
        mini(
          "return document.querySelector('.mini-feedback')?.dataset.review==='true';",
        ),
      "journal reopen",
    );
    await capture("uncertain_reopened_dark");
    await returnFm("main_uncertain_dark", "dark");
    await reconcile();
    await capture("reconciled_main_dark", true);
    await openMini();
    await theme("light");
    await show();
    await control("corrupt-next-write");
    await control("trash-rejected");
    await click("仅减少此歌曲");
    await wait(
      () =>
        mini(
          "return document.querySelector('.mini-feedback')?.dataset.needsRead==='true'&&document.querySelector('.mini-feedback').getAttribute('aria-busy')==='false';",
        ),
      "cache broken",
    );
    await capture("unobserved_light");
    await control("refresh-error");
    await readState();
    await wait(
      () =>
        mini(
          "return document.querySelector('.mini-feedback')?.textContent.includes('状态读取：');",
        ),
      "read failure",
    );
    await capture("readonly_failed_light");
    await theme("dark");
    await readState();
    await wait(
      () =>
        mini(
          "return !document.querySelector('.mini-feedback-submit').disabled;",
        ),
      "cache repaired",
    );
    await capture("readonly_healed_dark");
    await close();
    await click("返回主窗口");
    await wait(async () => {
      const v = await fixture("windows");
      return v.main && !v.mini.length;
    }, "final return");
    await theme("dark", true);
    await wait(
      () =>
        !document.querySelector(".fm-state-region") &&
        !document.querySelector(".fm-page .error-state") &&
        document.querySelector(".fm-copy h2")?.textContent ===
          `song-${document.querySelector(".playerbar").dataset.trackId}`,
      "normal return refreshes repaired main state",
    );
    await capture("returned_main_dark", true);
    await emit({
      kind: "complete",
      stage: "complete",
      ok: true,
      miniFeedbackUi: true,
      ipcIsolationVerified: true,
      stageCount: count,
      realMusicRequests: 0,
      realAccountWrites: 0,
    });
  } catch (error) {
    await emit({
      kind: "failed",
      stage: "failed",
      ok: false,
      last,
      count,
      error: String(error),
      body: document.body.textContent.slice(-1800),
    });
  }
})();
