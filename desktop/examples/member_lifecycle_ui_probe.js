// Real WebView consumes the synthetic native worker's membership observations.
// This script never requests an online rights read.
(async () => {
  const one = (selector) => document.querySelector(selector);
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const assert = (value, message) => { if (!value) throw Error(message); };
  const invoke = (...args) =>
    (window.__CUMULUS_PROOF_INVOKE__ ?? window.__TAURI_INTERNALS__.invoke)(...args);
  const emit = (stage, info = {}) =>
    invoke('plugin:event|emit', { event: 'ux-proof', payload: { stage, ...info } });
  const awaitValue = async (fn, label) => {
    for (let i = 0; i < 170; i++) {
      const value = await fn();
      if (value) return value;
      await sleep(60);
    }
    throw Error(label);
  };
  const QUERY = '合成会员刷新';
  const card = () => one('[data-result-key="song:601"]');
  const mv = () => card()?.querySelector('[data-result-action="mv"]')?.getAttribute('aria-label');
  const rights = async (epoch) => {
    const result = await invoke('member_rights', { epoch, refresh: false }).catch(() => null);
    if (result?.epoch !== epoch) return null;
    const all = result.value.entitlements;
    const active = (kind) => all.find((item) => item.kind === kind)?.active === true;
    return { vinyl: active('vinyl'), plus: active('plus') };
  };
  let step = 'startup', count = 0;
  const capture = async (name, extra = {}) => {
    step = name;
    await emit(`ux_${String(++count).padStart(2, '0')}_${name}`, {
      mv: mv() ?? null, vip: card()?.querySelector('.vip-tag')?.textContent ?? null, ...extra,
    });
  };
  try {
    const account = await awaitValue(async () => {
      const value = await invoke('account_snapshot');
      return value.status === 'authenticated' && value;
    }, 'synthetic account/get completes through the shell');
    await awaitValue(() => one('.track-row') &&
      one('.playerbar button[aria-label="列表循环"]')?.disabled === false,
      'signed-in shell finished starting');
    const input = await awaitValue(() => one('[aria-label="搜索音乐"]'), 'search input');
    input.value = QUERY;
    input.dispatchEvent(new Event('input', { bubbles: true }));
    input.form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    const initialCard = await awaitValue(() => one('.search-results-page h1')?.textContent === QUERY &&
      mv()?.startsWith('查看替代 MV') &&
      !one('.page-enter-active,.page-leave-active,.search-results-enter-active') && card(),
      'mounted search card while membership startup reply is held');
    assert(await rights(account.epoch) === null, 'no cached rights before the automatic read commits');
    await capture('login_verified');

    await emit('member-lifecycle-release');
    step = 'first automatic observation';
    const active = await awaitValue(async () => {
      const cached = await rights(account.epoch);
      return cached?.vinyl && cached.plus && mv()?.startsWith('播放 MV') &&
        card()?.querySelector('.vip-tag') && cached;
    }, 'startup read feeds cache and real search card');
    assert(card() === initialCard, 'native event updates the existing card');
    await capture('active', { cached: active });

    step = 'automatic expiry observation';
    const atExpiry = await awaitValue(async () => {
      const cached = await rights(account.epoch);
      return cached && !cached.vinyl && cached.plus && mv()?.startsWith('查看替代 MV') &&
        !card()?.querySelector('.vip-tag') && cached;
    }, 'automatic expiry recheck removed first right while second still active');
    // With the returned absolute dates unchanged, the original client does
    // not schedule the remaining group. Observe past that would-be deadline.
    await sleep(2_950);
    assert(card() === initialCard && mv()?.startsWith('查看替代 MV'), 'expiry leaves the same search card');
    await capture('expired', { cachedAtExpiry: atExpiry });
    await sleep(180);
    await emit('complete', { ok: true, stageCount: count, realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit('failed', {
      ok: false, step, error: error?.message ?? String(error), stack: error?.stack,
      mv: mv() ?? null, body: document.body.innerText.slice(0, 1700),
    });
  }
})();
