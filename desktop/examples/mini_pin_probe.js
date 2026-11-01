// Native window state and device-local settings may differ; neither is an acknowledgement of the other.
(async () => {
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const assert = (value, message) => { if (!value) throw Error(message); };
  const wait = async (check, message) => {
    for (let n = 0; n < 180; n++) { if (await check()) return; await sleep(80); }
    throw Error(message);
  };
  const emit = payload => invoke('plugin:event|emit', { event: 'mini-proof', payload });
  window.__miniAck = {};
  let serial = 0, count = 0, last = 'startup', playId;
  async function fixture(kind, payload = {}) {
    const id = ++serial;
    await emit({ kind, id, ...payload });
    await wait(() => id in window.__miniAck, `fixture ${kind}`);
    const reply = window.__miniAck[id]; delete window.__miniAck[id];
    if (reply.error) throw Error(reply.error);
    return reply.value ?? reply;
  }
  const mini = source => fixture('eval', { source });
  const read = () => mini("return await window.__TAURI_INTERNALS__.invoke('mini_read',{generation:location.hash.slice(6)});");
  async function theme(value) {
    await mini(`localStorage.setItem('cumulus.appearance.v1',${JSON.stringify(value)});window.dispatchEvent(new StorageEvent('storage',{key:'cumulus.appearance.v1',newValue:${JSON.stringify(value)}}));return true;`);
  }
  const facts = `const b=document.querySelector('[data-mini-pin]')||document.querySelector('.mini-header button'),r=b.getBoundingClientRect(),hit=document.elementFromPoint(r.left+r.width/2,r.top+r.height/2),a=document.activeElement;
    return {width:innerWidth,height:innerHeight,screenX,screenY,ratio:devicePixelRatio,visible:document.visibilityState==='visible',focused:document.hasFocus(),theme:document.documentElement.dataset.theme,
      overflow:document.documentElement.scrollWidth>innerWidth,pinPressed:b.getAttribute('aria-pressed'),pinSelected:b.getAttribute('aria-pressed')==='true',pinLabel:b.getAttribute('aria-label'),pinHit:b===hit||b.contains(hit),pinFocus:a===b,alert:document.querySelector('.mini-message[role="alert"]')?.textContent?.trim()??null,
      pinRect:{left:r.left,top:r.top,right:r.right,bottom:r.bottom},paintSamples:[{x:3,y:3,color:getComputedStyle(document.querySelector('.mini-shell')).backgroundColor}]};`;
  async function clickPin() {
    await mini("const b=document.querySelector('[data-mini-pin]')||document.querySelector('.mini-header button');if(b.disabled)throw Error('pin unavailable');b.focus({preventScroll:true});b.click();return true;");
    await wait(() => mini("return !document.querySelector('.mini-header button').disabled;"), 'pin completed');
  }
  async function sync(expected, applied) {
    await wait(async () => {
      const view = await read(), ui = await mini(facts);
      return view.preferences.alwaysOnTop === expected && view.pinApplied === applied && ui.pinSelected === applied;
    }, 'native and displayed pin agree');
  }
  async function capture(name, selected, label, problem = false) {
    last = name;
    await wait(async () => {
      const value = await mini(facts);
      return value.pinSelected === selected && value.pinLabel === label && !!value.alert === problem;
    }, 'pin state painted ' + name);
    await sleep(450);
    const data = await mini(facts), native = await fixture('pin-native'), shown = await read();
    assert(data.pinSelected === selected && data.pinSelected === native.applied, 'pin icon must show actual window state');
    assert(data.pinLabel === label, 'pin action must preserve saved intent');
    assert(!!data.alert === problem && !data.overflow && data.pinHit, 'pin feedback and hit area');
    const stage = `ux_${String(++count).padStart(2, '0')}_${name}`;
    window.__miniRecorded = null;
    await emit({ kind: 'snapshot', stage, miniPinUi: true, expectedPlayId: playId, pinNative: native, pinShown: shown, ...data });
    await wait(() => window.__miniRecorded === stage, 'native snapshot ' + name);
  }
  async function returnMain() {
    await mini("const b=document.querySelector('[aria-label=\"返回主窗口\"]');setTimeout(()=>b.click(),80);return true;");
    await wait(async () => { const v = await fixture('windows'); return v.main && !v.mini.length; }, 'main returned');
  }
  async function open() {
    await invoke('mini_enter');
    await wait(async () => !(await fixture('windows')).main, 'mini opened');
    await wait(() => mini("return !!document.querySelector('.mini-header button:not(:disabled)');"), 'mini ready');
  }
  try {
    await wait(() => document.querySelector('.track-row'), 'main startup');
    const account = await invoke('account_snapshot');
    await invoke('play_selection', { epoch: account.epoch, selection: { kind: 'songs', ids: ['1','2','3'], index: 0, quality: 'standard' } });
    await invoke('player_action', { epoch: account.epoch, action: { kind: 'pause', paused: true } });
    await invoke('player_action', { epoch: account.epoch, action: { kind: 'seek', seconds: 10 } });
    await wait(() => Math.abs(Number(document.querySelector('.playerbar')?.dataset.position) - 10) < .2, 'paused10');
    playId = document.querySelector('.playerbar').dataset.playId;
    await open(); await theme('light');
    if (window.__pinUnmanagedProof) {
      const initial = (await read()).preferences;
      for (const appearance of ['light', 'dark']) {
        await theme(appearance); await clickPin();
        await wait(async () => (await mini(facts)).pinFocus, 'readonly focus restored');
        const data = await mini(facts), shown = await read();
        assert(shown.pinApplied === null && data.pinPressed === null, 'unmanaged state stays unknown');
        assert(data.pinLabel === '重新读取置顶状态' && data.alert && !data.overflow && data.pinHit, 'readonly unmanaged affordance');
        assert(JSON.stringify(shown.preferences) === JSON.stringify(initial), 'readonly retry changed settings');
        const stage = `ux_${String(++count).padStart(2,'0')}_unmanaged_${appearance}`;
        window.__miniRecorded = null;
        await emit({kind:'snapshot',stage,miniPinUnmanaged:true,expectedPlayId:playId,pinShown:shown,...data});
        await wait(() => window.__miniRecorded === stage, 'native unmanaged snapshot');
      }
      await returnMain();
      await emit({kind:'complete',stage:'complete',ok:true,miniPinUnmanaged:true,stageCount:count,realMusicRequests:0,realAccountWrites:0});
      return;
    }
    await capture('pinned_light', true, '始终置顶');
    const initial = await read();
    const request = await fixture('pin-native', { set: false });
    assert(request.requested === false, 'native unpin request reached fixture');
    await capture('unapplied_light', false, '重试应用置顶设置', true);
    await theme('dark');
    await capture('unapplied_dark', false, '重试应用置顶设置', true);
    await clickPin(); await sync(true, true);
    assert((await read()).preferences.revision === initial.preferences.revision, 'retry must not write preferences');
    await capture('reapplied_dark', true, '始终置顶');
    await clickPin(); await sync(false, false);
    const unpinned = await read();
    assert(unpinned.preferences.revision !== initial.preferences.revision, 'explicit new preference committed');
    await capture('unpinned_dark', false, '始终置顶');
    await fixture('pin-native', { set: true }); await theme('light');
    await capture('unpin_unapplied_light', true, '重试应用置顶设置', true);
    await clickPin(); await sync(false, false);
    assert((await read()).preferences.revision === unpinned.preferences.revision, 'retry must preserve unpin intent');
    await capture('unpin_reapplied_light', false, '始终置顶');
    await fixture('pin-storage', { reject: true });
    await clickPin();
    await wait(() => mini("return !!document.querySelector('.mini-message[role=\"alert\"]');"), 'storage failure visible');
    assert((await read()).preferences.revision === unpinned.preferences.revision, 'failed save kept revision');
    await capture('save_rejected_light', false, '始终置顶', true);
    await fixture('pin-storage', { reject: false }); await theme('dark');
    await clickPin(); await sync(true, true);
    await capture('save_retried_dark', true, '始终置顶');
    const stale = await read();
    await fixture('pin-native', { save: false }); await theme('light');
    const rejected = await mini(`try{await window.__TAURI_INTERNALS__.invoke('mini_preferences_apply',{generation:${JSON.stringify(stale.generation)},ticket:${JSON.stringify(stale.ticket)},expectedRevision:${JSON.stringify(stale.preferences.revision)}});return false}catch{return true}`);
    assert(rejected, 'old preference revision accepted');
    await capture('stale_intent_light', true, '重试应用置顶设置', true);
    await clickPin(); await sync(false, false);
    await capture('stale_intent_healed_light', false, '始终置顶');
    await returnMain(); await open(); await theme('dark');
    await sync(false, false);
    await capture('reopened_dark', false, '始终置顶');
    await fixture('pin-native', { corrupt: true }); await theme('light');
    await wait(async () => (await read()).preferences.damaged, 'damaged settings observed');
    await capture('damaged_light', false, '修复置顶设置', true);
    await clickPin(); await sync(true, true);
    await capture('repaired_light', true, '始终置顶');
    const beforeAccount = await read();
    assert(await invoke('mini_preferences_apply',{generation:beforeAccount.generation,ticket:beforeAccount.ticket,expectedRevision:beforeAccount.preferences.revision}).then(()=>false,()=>true),'main gained mini pin IPC');
    assert(await mini("try{await window.__TAURI_INTERNALS__.invoke('account_snapshot');return false}catch{return true}"),'mini gained main account IPC');
    await fixture('account');
    await wait(async () => (await read()).media.epoch !== beforeAccount.media.epoch, 'account changed');
    assert(await mini(`try{await window.__TAURI_INTERNALS__.invoke('mini_preferences_apply',{generation:${JSON.stringify(beforeAccount.generation)},ticket:${JSON.stringify(beforeAccount.ticket)},expectedRevision:${JSON.stringify(beforeAccount.preferences.revision)}});return false}catch{return true}`),'old account pin accepted');
    await returnMain();
    await emit({ kind: 'complete', stage: 'complete', ok: true, miniPinUi: true, stageCount: count, readonlyApplyVerified: true, staleIntentRejected: true, ipcIsolationVerified: true, realMusicRequests: 0, realAccountWrites: 0 });
  } catch (error) {
    await emit({ kind: 'failed', stage: 'failed', message: String(error), stack: error?.stack, last, stageCount: count,
      pinFailureUi: await mini(facts).catch(() => null), pinFailureNative: await fixture('pin-native').catch(() => null) });
  }
})();
