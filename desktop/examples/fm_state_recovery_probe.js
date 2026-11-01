// Corrupt a synthetic local cache after a failed write, then recover by real readonly refresh.
(async () => {
  const p = window.createFmProof();
  const {
    one,
    all,
    sleep,
    invoke,
    emit,
    assert,
    wait,
    dialog,
    bar,
    button,
    click,
    fixture,
    prepare,
    show,
    close,
    theme,
    size,
    capture,
  } = p;
  async function blocked() {
    await wait(
      () =>
        one(".fm-page")?.dataset.fmStateError &&
        dialog()?.querySelector(".fm-state-recovery") &&
        dialog().getAttribute("aria-busy") === "false",
      "unobserved write is blocked",
    );
    assert(
      dialog().querySelector("footer .primary").disabled,
      "cannot resend unseen outcome",
    );
    assert(
      [...dialog().querySelectorAll(".fm-reason")].every((n) => n.disabled),
      "reason choices disabled",
    );
    assert(
      document.activeElement === button("重新读取状态", dialog()),
      "readonly recovery focus",
    );
  }
  async function read() {
    click(button("重新读取状态", dialog() || one(".fm-state-region")));
    await new Promise(requestAnimationFrame);
    await wait(
      () =>
        !one('.fm-state-recovery [aria-busy="true"]') &&
        !one('.fm-state-recovery[aria-busy="true"]'),
      "readonly refresh settled",
    );
    await sleep(80);
  }
  try {
    await wait(
      () =>
        one(".account-button") &&
        !one(".account-button").disabled &&
        !one(".welcome"),
      "account",
    );
    p.setEpoch((await invoke("account_snapshot")).epoch);
    await theme("light");
    click(button("私人漫游"));
    await wait(() => button("开始漫游"), "FM");
    click(button("开始漫游"));
    await prepare();
    await capture("ready_light");
    await show();
    await fixture("corrupt-next-write");
    await fixture("trash-lost");
    click(button("仅减少此歌曲", dialog()));
    await blocked();
    await capture("unobserved_light");
    dialog().querySelector("footer .primary").click();
    await theme("dark");
    await capture("unobserved_dark");
    await size(760, 540);
    await capture("unobserved_narrow_dark");
    await fixture("fail-refresh");
    await read();
    await blocked();
    await capture("readonly_failed_dark");
    await read();
    await wait(
      () =>
        !one(".fm-page").dataset.fmStateError &&
        button("查看反馈状态", dialog()),
      "state refresh reveals unresolved journal",
    );
    assert(
      dialog().querySelector("footer .primary").disabled,
      "unresolved journal is not a retry permission",
    );
    await capture("readonly_recovered_dark");
    click(button("查看反馈状态", dialog()));
    await wait(
      () =>
        !one(".fm-dialog[open]") &&
        document.activeElement === one(".fm-recovery"),
      "recovery navigation",
    );
    await capture("journal_dark");
    click(button("核对并继续", one(".fm-recovery")));
    await wait(() => !one(".fm-recovery"), "explicit journal reconciliation");
    await prepare();
    await capture("confirmed_dark");
    await theme("light");
    await show();
    await fixture("corrupt-next-write");
    await fixture("trash-rejected");
    click(button("仅减少此歌曲", dialog()));
    await blocked();
    await capture("refused_unobserved_light");
    await read();
    await wait(
      () =>
        !one(".fm-page").dataset.fmStateError &&
        !dialog().querySelector("footer .primary").disabled,
      "known refusal permits a new explicit decision only after reading",
    );
    await capture("refused_observed_light");
    await close();
    click(button("减少推荐列表"));
    await wait(() => one(".fm-trash input"), "existing trash");
    click(one(".fm-trash input"));
    await fixture("corrupt-next-write");
    await fixture("restore-lost");
    click(one(".fm-trash-toolbar .primary"));
    await wait(
      () =>
        one(".fm-state-region .fm-state-recovery") &&
        !one(".fm-trash").getAttribute("aria-busy").includes("true"),
      "restoration plus unreadable state",
    );
    assert(
      one(".fm-trash").dataset.selectedCount === "1",
      "selected identity retained",
    );
    assert(
      one(".fm-trash-toolbar .primary").disabled,
      "restore blocked before observing",
    );
    await capture("restore_unobserved_light");
    await fixture("fail-refresh");
    await read();
    assert(
      one(".fm-trash-toolbar .primary").disabled,
      "failed reread keeps restore blocked",
    );
    await capture("restore_readonly_failed_light");
    await theme("dark");
    await read();
    await wait(
      () =>
        !one(".fm-page").dataset.fmStateError &&
        button("核对恢复状态", one(".fm-recovery") || document),
      "restoration journal discovered",
    );
    assert(
      one(".fm-trash-toolbar .primary").disabled,
      "unknown restoration cannot be repeated",
    );
    await capture("restore_observed_dark");
    click(button("核对恢复状态", one(".fm-recovery")));
    await wait(
      () => !one(".fm-trash [data-song-id]") && !one(".fm-recovery"),
      "restored and refreshed",
    );
    await capture("restore_confirmed_dark");
    await emit("complete", {
      ok: true,
      fmStateRecoveryUi: true,
      stageCount: p.count,
      realMusicRequests: 0,
      realAccountWrites: 0,
    });
  } catch (error) {
    await emit("failed", {
      ok: false,
      last: p.last,
      count: p.count,
      error: String(error),
      active: document.activeElement?.outerHTML.slice(0, 300),
      body: document.body.textContent.slice(-2200),
    });
  }
})();
